"""Regenerate offline golden fixtures using the locked native vLLM renderer.

No model weights or inference server are needed. A byte tokenizer makes every
rendered character observable; BOS postprocessing exercises special-token policy.
"""

import asyncio
import json
from pathlib import Path
import subprocess
import tempfile

from native_reference import ROOT, native_renderer, native_result, configuration_for_test


def fixture_model(directory: Path):
    from tokenizers import Tokenizer, models, pre_tokenizers, decoders, processors
    from transformers import PreTrainedTokenizerFast

    vocab = {c: i + 1 for i, c in enumerate(sorted(pre_tokenizers.ByteLevel.alphabet()))}
    vocab["<s>"] = 0
    tokenizer = Tokenizer(models.BPE(vocab, []))
    tokenizer.pre_tokenizer = pre_tokenizers.ByteLevel(add_prefix_space=False, use_regex=False)
    tokenizer.decoder = decoders.ByteLevel()
    tokenizer.post_processor = processors.TemplateProcessing(single="<s> $A", special_tokens=[("<s>", 0)])
    fast = PreTrainedTokenizerFast(tokenizer_object=tokenizer, bos_token="<s>")
    fast.save_pretrained(directory)
    (directory / "config.json").write_text(json.dumps({"model_type":"llama", "architectures":["LlamaForCausalLM"],
        "hidden_size":64,"intermediate_size":128,"num_hidden_layers":1,"num_attention_heads":2,
        "num_key_value_heads":2,"vocab_size":257,"max_position_embeddings":8192}))


async def generate():
    subprocess.run(["cargo", "build", "--locked", "--package", "infergate"], cwd=ROOT, check=True)
    cases = json.loads(Path(__file__).with_name("cases.json").read_text())
    cases.extend([
        {"name":"tool-result-empty-parts", "request":{"input":[{"role":"user","content":"Q"},{"type":"function_call_output","call_id":"call_1","output":[{"type":"input_text","text":""},{"type":"input_text","text":"B"}]}]}},
        {"name":"text-cache-breakpoint", "request":{"input":[{"role":"user","content":[{"type":"input_text","text":"prefix", "prompt_cache_breakpoint":{"mode":"explicit"}}]}]}},
        {"name":"salt-unicode", "request":{"input":"The same prompt", "cache_salt":"isolated 🦀"}},
        {"name":"reasoning-effort", "request":{"input":"Think", "reasoning":{"effort":"high"}}},
        {"name":"empty-parts", "request":{"input":[{"role":"user","content":[{"type":"input_text","text":""},{"type":"input_text","text":"B"}]}]}},
    ])
    with tempfile.TemporaryDirectory(prefix="infergate-fixture-model-") as temp:
        model = Path(temp)
        fixture_model(model)
        for content_format in ("string", "openai"):
            template = "{{ bos_token }}{% for m in messages %}[{{ m.role }}]{{ m.content | tojson }}{% if m.tool_calls %}{{ m.tool_calls | tojson }}{% endif %}{% if m.reasoning is defined %}{{ m.reasoning | tojson }}{% endif %}{% if m.tool_call_id is defined %}{{ m.tool_call_id }}{% endif %}{% endfor %}{% if tools %}{{ tools | tojson }}{% endif %}{% if reasoning_effort is defined %}{{ reasoning_effort }}{% endif %}{% if add_generation_prompt %}[assistant]{% endif %}"
            state = native_renderer(str(model), "fixture", template, content_format)
            directory = ROOT / "services/gateway/src/testdata" / f"input-{content_format}"
            configuration = configuration_for_test(state)
            directory.mkdir(parents=True, exist_ok=True)
            (directory / "model-config.json").write_text(json.dumps(configuration, ensure_ascii=False) + "\n")
            prefix = configuration["prefix"]
            block_size, parent = prefix["block_size"], bytes(prefix["initial_parent"])
            from verify import prepare_cases
            prepared = prepare_cases(cases, "fixture", configuration)
            results = []
            try:
                for case, (wire, _) in zip(cases, prepared, strict=True):
                    request = case["request"] | {"model":"fixture"}
                    from vllm.exceptions import VLLMValidationError
                    try:
                        expected = await native_result(state, wire if wire is not None else request, block_size, parent)
                    except VLLMValidationError:
                        if not case.get("template_may_reject"):
                            raise
                        expected = None
                    results.append({"name":case["name"],"request":request,"expected":expected})
            finally:
                state.engine_client.renderer.shutdown()
            (directory / "parity.json").write_text(json.dumps(results, ensure_ascii=False, indent=2)+"\n")
            print(f"{content_format}: {len(results)} native token/hash fixtures")


if __name__ == "__main__":
    asyncio.run(generate())
