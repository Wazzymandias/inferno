"""Compare gateway rendering with vLLM rendering of its actual forwarded JSON."""

import argparse
import os
import tempfile
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path

from native_reference import native_hashes
import subprocess
import threading
import urllib.error
import urllib.request


ROOT = Path(__file__).resolve().parents[2]


def prepare_cases(cases: list[dict], model: str, configuration: dict):
    """Capture forwarded JSON, then inspect every input with the backend offline."""
    # Tool-schema key order becomes template tokens. Obtain canonical JSON from
    # the forwarding owner rather than reproducing its serializer in Python.
    discovery_calls = []
    class Capture(BaseHTTPRequestHandler):
        def do_GET(self):
            assert self.headers.get("Authorization") == "Bearer configuration-test-token"
            parsed = urllib.parse.urlsplit(self.path)
            assert parsed.path == "/v1/inferno/model-config"
            assert urllib.parse.parse_qs(parsed.query) == {"model": [model]}
            discovery_calls.append(self.path)
            body = json.dumps(configuration).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            assert self.headers.get("Authorization") == "Bearer configuration-test-token"
            assert self.path == "/v1/responses", "request preparation must never call render"
            body = self.rfile.read(int(self.headers["Content-Length"]))
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_):
            pass

    with tempfile.TemporaryDirectory(prefix="inferno-cache-test-") as cache, ThreadingHTTPServer(("127.0.0.1", 0), Capture) as backend:
        environment = os.environ | {"XDG_CACHE_HOME": cache, "INFERENCE_API_KEY": "configuration-test-token"}
        endpoint = f"http://127.0.0.1:{backend.server_port}/v1"
        threading.Thread(target=backend.serve_forever, daemon=True).start()
        gateway = subprocess.Popen([
            str(ROOT / "target/debug/inferno"), "serve", "--api-address", "127.0.0.1",
            "--api-port", "0", "--inference-endpoint",
            endpoint, "--model", model,
        ], stdout=subprocess.PIPE, text=True, env=environment)
        try:
            ready = gateway.stdout.readline().strip()
            if not ready.startswith("inferno listening on "):
                raise RuntimeError("gateway failed to discover model configuration")
            address = ready.removeprefix("inferno listening on ")
            requests = []
            for case in cases:
                body = json.dumps(case["request"] | {"model": model, "store": False}).encode()
                request = urllib.request.Request(
                    f"http://{address}/v1/responses", data=body,
                    headers={"Content-Type": "application/json"},
                )
                try:
                    with urllib.request.urlopen(request, timeout=10) as response:
                        requests.append(json.load(response))
                except urllib.error.HTTPError as error:
                    if error.code != 400:
                        raise
                    requests.append(None)
        finally:
            gateway.terminate()
            gateway.wait(timeout=10)
            backend.shutdown()
            backend.server_close()
        assert len(discovery_calls) == 1, "configuration must be discovered once before serving"
        results = []
        for case, wire in zip(cases, requests, strict=True):
            rendered = subprocess.run([
                str(ROOT / "target/debug/inferno"), "render", "--model", model,
                "--inference-endpoint", endpoint,
            ], input=json.dumps(case["request"] | {"model":model, "store":False}),
                text=True, capture_output=True, env=environment, timeout=30)
            results.append((wire, rendered))
        return results


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inference-endpoint", required=True, help="running native vLLM /v1 endpoint")
    parser.add_argument("--model", required=True, help="model ID served by that backend")
    arguments = parser.parse_args()
    subprocess.run(["cargo", "build", "--package", "inferno", "--locked"], cwd=ROOT, check=True)
    cases = json.loads(Path(__file__).with_name("cases.json").read_text())
    headers = {"Content-Type": "application/json"}
    if os.environ.get("INFERENCE_API_KEY"):
        headers["Authorization"] = "Bearer " + os.environ["INFERENCE_API_KEY"]
    url = arguments.inference_endpoint.rstrip("/") + "/inferno/model-config?" + urllib.parse.urlencode({"model": arguments.model})
    with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=30) as response:
        configuration = json.load(response)
    prepared = prepare_cases(cases, arguments.model, configuration)
    prefix = configuration["prefix"]
    matched = rejected = 0
    for case, (wire, rendered) in zip(cases, prepared, strict=True):
        request = urllib.request.Request(
            arguments.inference_endpoint.rstrip("/") + "/responses/render",
            data=json.dumps(wire if wire is not None else case["request"] | {"model":arguments.model}).encode(), headers=headers,
        )
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                expected = json.load(response)["token_ids"]
        except urllib.error.HTTPError as error:
            if error.code != 400:
                raise
            expected = None
        if expected is None:
            if not case.get("template_may_reject") or rendered.returncode == 0:
                raise AssertionError(f"{case['name']}: unexpected or inconsistent backend rejection")
            rejected += 1
        else:
            expected_result = {
                "token_ids": expected,
                "prefix_hashes": native_hashes(expected, prefix["block_size"], bytes(prefix["initial_parent"]), case["request"].get("cache_salt")),
            }
            if rendered.returncode != 0 or json.loads(rendered.stdout) != expected_result:
                raise AssertionError(f"{case['name']}: local tokens/hashes differ from native preprocessing")
            matched += 1
    print(f"{matched} exact native token/hash matches, {rejected} matching backend rejections", flush=True)


if __name__ == "__main__":
    main()
