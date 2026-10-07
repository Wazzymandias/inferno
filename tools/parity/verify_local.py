"""Compare local Rust preparation with native vLLM preprocessing, without weights.

Uses the supplied local checkpoint's tokenizer and native template detection.
The cache block size in this test configuration is a fixture, not a deployed cache size.
"""

import argparse
import asyncio
import json
from pathlib import Path
import subprocess

from native_reference import ROOT, configuration_for_test, native_renderer, native_result
from verify import prepare_cases


async def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model-path", type=Path, required=True)
    parser.add_argument("--model", required=True, help="served model name for this comparison")
    arguments = parser.parse_args()
    subprocess.run(["cargo", "build", "--locked", "--package", "inferno"], cwd=ROOT, check=True)
    cases = json.loads(Path(__file__).with_name("cases.json").read_text())
    state = native_renderer(str(arguments.model_path.resolve()), arguments.model)
    matched = rejected = 0
    try:
        configuration = configuration_for_test(state)
        prefix = configuration["prefix"]
        block_size, parent = prefix["block_size"], bytes(prefix["initial_parent"])
        prepared = prepare_cases(cases, arguments.model, configuration)
        for case, (wire, result) in zip(cases, prepared, strict=True):
            from vllm.exceptions import VLLMValidationError
            request = case["request"] | {"model":arguments.model, "store":False}
            try:
                expected = await native_result(state, wire if wire is not None else request, block_size, parent)
            except VLLMValidationError:
                if not case.get("template_may_reject"):
                    raise
                expected = None
            if expected is None:
                if result.returncode == 0:
                    raise AssertionError(f"{case['name']}: local preparation accepted a native rejection")
                rejected += 1
            else:
                if result.returncode:
                    raise AssertionError(f"{case['name']}: local preparation rejected a native-supported fixture: {result.stderr}")
                actual = json.loads(result.stdout)
                if actual != expected:
                    tokens = actual["token_ids"]
                    native = expected["token_ids"]
                    first = next((i for i, (a,b) in enumerate(zip(tokens,native)) if a!=b), min(len(tokens),len(native)))
                    raise AssertionError(f"{case['name']}: token/hash mismatch; token lengths {len(tokens)}/{len(native)}, first difference {first}")
                matched += 1
    finally:
        state.engine_client.renderer.shutdown()
    print(f"{matched} exact native token/hash matches, {rejected} matching native rejections")


if __name__ == "__main__":
    asyncio.run(main())
