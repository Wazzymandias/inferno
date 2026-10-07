"""Start native vLLM Metal and the Compose app as one development session."""

import argparse
import os
from pathlib import Path
import platform
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request


def port_number(value: str) -> int:
    try:
        port = int(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("port must be an integer") from error
    if not 1 <= port <= 65535:
        raise argparse.ArgumentTypeError("port must be between 1 and 65535")
    return port


def boolean(value: str) -> bool:
    if value not in ("true", "false"):
        raise argparse.ArgumentTypeError("use true or false")
    return value == "true"


def wait_until_ready(server: subprocess.Popen, port: int) -> None:
    # Model loading may take minutes. The process and Ctrl-C, rather than an
    # arbitrary model-size-dependent deadline, bound startup.
    while server.poll() is None:
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=1) as response:
                if response.status == 200:
                    return
        except (urllib.error.URLError, TimeoutError):
            pass
        time.sleep(0.25)
    raise RuntimeError(f"vLLM exited during startup (status {server.returncode})")


def handle_termination(*_) -> None:
    raise KeyboardInterrupt


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dev", action="store_true", default=True, help="use the development deployment (default: true)")
    parser.add_argument("-d", "--detached", type=boolean, nargs="?", const=True, default=True, metavar="true|false", help="run in the background (default: true); use --detached=false for live terminal output")
    parser.add_argument("--model", help="override INFERENCE_MODEL from the root .env")
    parser.add_argument("--port", type=port_number, help="override INFERENCE_PORT from the root .env")
    parser.add_argument("vllm_args", nargs=argparse.REMAINDER, help="extra native vLLM options after --")
    arguments = parser.parse_args()
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        parser.error("vLLM Metal requires an Apple Silicon Mac")
    root = Path(__file__).resolve().parents[2]
    if arguments.detached:
        log_path = root / "tools/deploy/deploy.log"
        # The parser owns the boundary between deployment and native options.
        # Override detached mode after the caller's flags, before native options,
        # so an explicit -d cannot make the child detach recursively.
        launcher_arguments = sys.argv[1:len(sys.argv) - len(arguments.vllm_args)]
        # A new session survives terminal closure. The same interpreter and
        # arguments run the existing lifecycle, with both output streams logged.
        with log_path.open("a") as log:
            deployment = subprocess.Popen(
                [sys.executable, str(Path(__file__).resolve()), *launcher_arguments, "--detached=false", *arguments.vllm_args],
                cwd=root, stdin=subprocess.DEVNULL, stdout=log,
                stderr=subprocess.STDOUT, start_new_session=True,
            )
        print(f"Deployment is starting in the background (PID {deployment.pid}).", flush=True)
        print(f"Follow startup progress and live logs: tail -f {shlex.quote(str(log_path))}", flush=True)
        print("Use just deploy down to stop it.", flush=True)
        return 0
    if not root.joinpath(".env").exists():
        print("Creating .env from .env.example...", flush=True)
        shutil.copyfile(root / ".env.example", root / ".env")
    # Compose owns .env loading, interpolation, and shell overrides. Capture its
    # configuration without logging it: it can contain service credentials.
    print("Reading Compose configuration...", flush=True)
    try:
        configured = subprocess.run(
            ["docker", "compose", "config", "--environment"],
            cwd=root, text=True, capture_output=True, check=True,
        )
    except subprocess.CalledProcessError as error:
        raise RuntimeError(
            f"Could not read Compose configuration (exit status {error.returncode}). "
            "Check Docker availability and .env settings."
        ) from error
    settings = dict(
        line.split("=", 1) for line in configured.stdout.splitlines() if "=" in line
    )
    model = arguments.model if arguments.model is not None else settings.get("INFERENCE_MODEL", "")
    if not model.strip():
        parser.error("set INFERENCE_MODEL in .env or supply --model")
    try:
        port = arguments.port if arguments.port is not None else port_number(settings.get("INFERENCE_PORT", ""))
        configured_args = shlex.split(settings.get("VLLM_ARGS", ""))
    except (argparse.ArgumentTypeError, ValueError):
        parser.error("set a valid INFERENCE_PORT and shell-quoted VLLM_ARGS in .env")
    extra = arguments.vllm_args
    if extra[:1] == ["--"]:
        extra = extra[1:]
    extra = configured_args + extra
    # These flags belong to this command. Overrides would disconnect the app
    # from the process or change the verification API's availability.
    owned = {"--host", "--port", "--model", "--served-model-name", "--enable-scale-out", "--no-enable-scale-out", "--scheduler-cls", "--prefix-caching-hash-algo"}
    if any(option.split("=", 1)[0] in owned for option in extra):
        parser.error("model, listener, served name, render API, and model configuration discovery are owned by this command")
    environment = os.environ.copy()
    # The process being launched owns Compose's model and backend connection.
    environment["INFERENCE_MODEL"] = model
    environment["INFERENCE_PORT"] = str(port)
    environment["INFERENCE_ENDPOINT"] = f"http://host.docker.internal:{port}/v1"
    # Both native hooks are installed with the release; they publish resolved
    # model configuration for gateway startup, without shared asset paths.
    environment["PYTHONPATH"] = os.pathsep.join(filter(None, [str(root), environment.get("PYTHONPATH")]))
    # Never mistake another server's health endpoint for this child's startup.
    print(f"Starting native vLLM on port {port}...", flush=True)
    with socket.socket() as listener:
        # Match vLLM's listener so recently closed connections allow a restart.
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            listener.bind(("0.0.0.0", port))
        except OSError as error:
            raise RuntimeError(
                f"Cannot use native vLLM port {port}: {error.strerror}. "
                "Stop its current server or choose another --port."
            ) from error
    command = [
        str(Path(sys.executable).with_name("vllm")), "serve", model,
        "--served-model-name", model,
        "--host", "0.0.0.0", "--port", str(port),
        "--enable-auto-tool-choice", "--tool-call-parser", "hermes",
        # Exposes /v1/responses/render on this same inference server.
        "--enable-scale-out",
        "--prefix-caching-hash-algo", "sha256_cbor",
        "--scheduler-cls", "tools.deploy.model_config.ExportingScheduler",
        "--middleware", "tools.deploy.model_config.ModelConfigMiddleware",
        *extra,
    ]
    # Child processes inherit stdout and stderr, keeping their output live.
    server = subprocess.Popen(command, start_new_session=True, env=environment)
    compose_started = False
    try:
        signal.signal(signal.SIGTERM, handle_termination)
        print("Waiting for vLLM readiness. Model loading may take several minutes...", flush=True)
        wait_until_ready(server, port)
        print("Native vLLM is ready.", flush=True)
        # Include partial startup failures in cleanup; volumes are preserved.
        compose_started = True
        print("Building and starting Compose services...", flush=True)
        subprocess.run(
            ["docker", "compose", "up", "--wait"],
            cwd=root, env=environment, check=True,
        )
        print("Deployment ready. Use just deploy down to stop and remove containers.", flush=True)
        status = server.wait()
        print(f"vLLM exited (status {status}). Stopping the deployment...", flush=True)
        return status
    finally:
        try:
            if compose_started:
                print("Stopping Compose services...", flush=True)
                subprocess.run(
                    ["docker", "compose", "stop"],
                    cwd=root, env=environment, check=True,
                )
                print("Compose services stopped. Containers and persistent volumes were retained.", flush=True)
        finally:
            # Include workers even if the server leader has already exited.
            print("Stopping native vLLM and its workers...", flush=True)
            try:
                os.killpg(server.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            if server.poll() is None:
                try:
                    server.wait(timeout=10)
                except subprocess.TimeoutExpired as error:
                    print(f"vLLM did not stop within {error.timeout:g} seconds; forcing shutdown...", flush=True)
                    os.killpg(server.pid, signal.SIGKILL)
                    server.wait()
            print("Native vLLM server stopped.", flush=True)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        print("Deployment interrupted.", flush=True)
        raise SystemExit(130)
    except subprocess.CalledProcessError as error:
        print(f"Docker Compose failed (exit status {error.returncode}); see its output above.", flush=True)
        raise SystemExit(1)
    except OSError as error:
        # Only the OS reason is safe to print; filenames may contain private data.
        print(f"Deployment failed: {error.strerror}.", flush=True)
        raise SystemExit(1)
    except RuntimeError as error:
        # These messages contain context from the owning deployment step only.
        print(f"Deployment failed: {error}", flush=True)
        raise SystemExit(1)
