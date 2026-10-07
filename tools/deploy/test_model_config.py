"""Check startup discovery against native scheduler objects, without model weights."""

import asyncio
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "tools/parity"))

from model_config import ExportingScheduler, ModelConfigMiddleware, prefix_path
from fixtures import fixture_model


class ModelConfigTests(unittest.TestCase):
    def test_native_scheduler_selection_and_resolved_hash_granularity(self):
        from vllm.config import ModelConfig, VllmConfig, DeviceConfig, CacheConfig, SchedulerConfig
        from vllm.v1.kv_cache_interface import KVCacheConfig, KVCacheGroupSpec, FullAttentionSpec, SlidingWindowSpec
        import torch
        from vllm.v1.core.sched.scheduler import Scheduler
        from vllm.v1.core.sched.async_scheduler import AsyncScheduler
        from vllm.utils.hashing import sha256_cbor

        with tempfile.TemporaryDirectory(prefix="infergate-export-test-") as temp:
            directory = Path(temp)
            fixture_model(directory)
            config = VllmConfig(
                model_config=ModelConfig(model=temp, tokenizer=temp, max_model_len=128),
                device_config=DeviceConfig(device="cpu"),
                cache_config=CacheConfig(block_size=32, prefix_caching_hash_algo="sha256_cbor"),
                scheduler_config=SchedulerConfig(max_model_len=128, is_encoder_decoder=False, max_num_batched_tokens=128, max_num_seqs=1),
            )
            config.cache_config.num_gpu_blocks = 32
            kv_config = KVCacheConfig(num_blocks=32, kv_cache_tensors=[], kv_cache_groups=[
                KVCacheGroupSpec(layer_names=["full"], kv_cache_spec=FullAttentionSpec(block_size=16, num_kv_heads=1, head_size=8, dtype=torch.float16)),
                KVCacheGroupSpec(layer_names=["sliding"], kv_cache_spec=SlidingWindowSpec(block_size=32, num_kv_heads=1, head_size=8, dtype=torch.float16, sliding_window=64)),
            ])
            from vllm.v1.core.kv_cache_utils import resolve_kv_cache_block_sizes
            block_size, hash_block_size = resolve_kv_cache_block_sizes(kv_config, config)
            with patch.dict(os.environ, PYTHONHASHSEED="42"), patch("model_config.tempfile.gettempdir", return_value=temp):
                for asynchronous, native_type in [(False, Scheduler), (True, AsyncScheduler)]:
                    config.scheduler_config.async_scheduling = asynchronous
                    result = ExportingScheduler(
                        vllm_config=config, kv_cache_config=kv_config,
                        structured_output_manager=None, block_size=block_size, hash_block_size=hash_block_size,
                    )
                    self.assertIs(type(result), native_type)
                    exported = json.loads(prefix_path(config).read_text())
                    self.assertEqual(exported["block_size"], 16)
                    self.assertEqual(bytes(exported["initial_parent"]), sha256_cbor("42"))
                    self.assertEqual(config.cache_config.block_size, 32)

    def test_discovery_is_a_startup_snapshot_protected_by_native_authentication(self):
        from starlette.applications import Starlette
        from starlette.testclient import TestClient
        from vllm.entrypoints.serve.middleware.authenticate import AuthenticationMiddleware
        from types import SimpleNamespace

        application = Starlette()
        application.state.vllm_config = SimpleNamespace(instance_id="test-instance")
        application.add_middleware(ModelConfigMiddleware)
        application.add_middleware(AuthenticationMiddleware, tokens=["test-token"])
        resolved = {"encoder": {"models": ["fixture"]}, "tokenizer_json": "exact loaded assets"}
        with tempfile.TemporaryDirectory() as temp, patch("model_config.tempfile.gettempdir", return_value=temp):
            path = prefix_path(application.state.vllm_config)
            path.write_text('{"block_size":16}')
            with patch("model_config.model_config", return_value=resolved) as resolve:
                with TestClient(application) as client:
                    self.assertFalse(path.exists())
                    url = "/v1/infergate/model-config?model=fixture"
                    self.assertEqual(client.get(url).status_code, 401)
                    headers = {"Authorization": "Bearer test-token"}
                    for _ in range(2):
                        response = client.get(url, headers=headers)
                        self.assertEqual(response.status_code, 200)
                        self.assertEqual(response.json(), resolved)
                    self.assertEqual(client.get("/v1/infergate/model-config?model=other", headers=headers).status_code, 404)
                    self.assertEqual(client.post(url, headers=headers).status_code, 405)
                resolve.assert_called_once_with(application.state, {"block_size":16})

    def test_configuration_failure_prevents_native_readiness(self):
        from starlette.applications import Starlette
        from types import SimpleNamespace
        application = Starlette()
        application.state.vllm_config = SimpleNamespace(instance_id="test-instance")
        application.add_middleware(ModelConfigMiddleware)
        messages = []
        async def receive():
            return {"type":"lifespan.startup"}
        async def send(message):
            messages.append(message["type"])
        with tempfile.TemporaryDirectory() as temp, patch("model_config.tempfile.gettempdir", return_value=temp):
            prefix_path(application.state.vllm_config).write_text('{}')
            with patch("model_config.model_config", side_effect=ValueError("unsupported encoder")):
                with self.assertRaisesRegex(ValueError, "unsupported encoder"):
                    asyncio.run(application({"type":"lifespan", "state":{}}, receive, send))
        self.assertEqual(messages, ["lifespan.startup.failed"])


if __name__ == "__main__":
    unittest.main()
