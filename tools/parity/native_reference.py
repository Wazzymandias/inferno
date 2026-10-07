"""Native reference operations shared by fixture generation and parity checks.

These call the locked vLLM implementation rather than reproducing its algorithms.
No inference weights are loaded by the offline renderer.
"""

import copy
import json
from pathlib import Path
import sys
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))


def native_renderer(model: str, name: str, template=None, content_format="auto"):
    from vllm.config import ModelConfig, VllmConfig, DeviceConfig
    from vllm.renderers.hf import HfRenderer
    from vllm.renderers.online_renderer import OnlineRenderer
    from vllm.tokenizers import get_tokenizer

    config = VllmConfig(model_config=ModelConfig(
        model=model, tokenizer=model, max_model_len=8192, language_model_only=True,
    ), device_config=DeviceConfig(device="cpu"))
    renderer = HfRenderer(config, get_tokenizer(model))
    online = OnlineRenderer(config.model_config, renderer, request_logger=None,
        chat_template=template, chat_template_content_format=content_format,
        enable_auto_tools=True, tool_parser="hermes")
    return SimpleNamespace(vllm_config=config, engine_client=SimpleNamespace(renderer=renderer),
        online_renderer=online, args=SimpleNamespace(model=model, served_model_name=[name]))


def native_hashes(tokens, block_size: int, parent: bytes, salt=None):
    from vllm.utils.hashing import sha256_cbor
    from vllm.v1.core.kv_cache_utils import hash_block_tokens

    hashes = []
    for index in range(0, len(tokens) - block_size + 1, block_size):
        extras = (("cache_salt", salt),) if index == 0 and salt else None
        parent = hash_block_tokens(sha256_cbor, parent, tokens[index:index + block_size], extras)
        hashes.append(parent.hex())
    return hashes


async def native_result(state, wire: dict, block_size: int, parent: bytes):
    from vllm.entrypoints.openai.responses.protocol import ResponsesRequest

    result = await state.online_renderer.render_responses(ResponsesRequest.model_validate(copy.deepcopy(wire)))
    tokens = result.engine_input["prompt_token_ids"]
    return {"token_ids": tokens, "prefix_hashes": native_hashes(tokens, block_size, parent, wire.get("cache_salt"))}


def configuration_for_test(state):
    from tools.deploy.model_config import model_config
    from vllm.utils.hashing import sha256_cbor
    from vllm.v1.core.kv_cache_utils import resolve_none_hash_seed

    parent = sha256_cbor(resolve_none_hash_seed(sha256_cbor))
    # Deliberately small test granularity exercises chains and partial tails.
    return model_config(state, {
        "algorithm": "sha256_cbor", "block_size": 8, "initial_parent": list(parent),
    })
