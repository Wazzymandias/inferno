"""Check startup discovery against native scheduler objects, without model weights."""

import asyncio
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "tools/parity"))

from model_config import ExportingScheduler, ModelConfigMiddleware, prefix_path
from fixtures import fixture_model


class ModelConfigTests(unittest.TestCase):
    def test_native_downgrade_of_prefix_caching_fails_before_scheduler_creation(self):
        from types import SimpleNamespace

        config = SimpleNamespace(diffusion_config=None, cache_config=SimpleNamespace(enable_prefix_caching=False))
        with self.assertRaisesRegex(ValueError, "requires prefix caching"):
            ExportingScheduler(config)

    def test_integer_event_hashes_fail_before_scheduler_creation(self):
        from types import SimpleNamespace

        config = SimpleNamespace(diffusion_config=None, cache_config=SimpleNamespace(enable_prefix_caching=True))
        with patch.dict(os.environ, VLLM_KV_EVENTS_USE_INT_BLOCK_HASHES="1"):
            with self.assertRaisesRegex(ValueError, "VLLM_KV_EVENTS_USE_INT_BLOCK_HASHES=0"):
                ExportingScheduler(config)

    def test_native_scheduler_selection_and_resolved_hash_granularity(self):
        from vllm.config import ModelConfig, VllmConfig, DeviceConfig, CacheConfig, SchedulerConfig
        from vllm.v1.kv_cache_interface import KVCacheConfig, KVCacheGroupSpec, FullAttentionSpec, SlidingWindowSpec
        import torch
        from vllm.v1.core.sched.scheduler import Scheduler
        from vllm.v1.core.sched.async_scheduler import AsyncScheduler
        from vllm.utils.hashing import sha256_cbor

        with tempfile.TemporaryDirectory(prefix="inferno-export-test-") as temp:
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
            with patch.dict(os.environ, PYTHONHASHSEED="42", VLLM_KV_EVENTS_USE_INT_BLOCK_HASHES="0"), patch("model_config.tempfile.gettempdir", return_value=temp):
                for asynchronous, native_type in [(False, Scheduler), (True, AsyncScheduler)]:
                    config.scheduler_config.async_scheduling = asynchronous
                    result = ExportingScheduler(
                        vllm_config=config, kv_cache_config=kv_config,
                        structured_output_manager=None, block_size=block_size, hash_block_size=hash_block_size,
                    )
                    self.assertIs(type(result), native_type)
                    exported = json.loads(prefix_path(config).read_text())
                    self.assertEqual(exported["prefix"]["block_size"], 16)
                    self.assertEqual(bytes(exported["prefix"]["initial_parent"]), sha256_cbor("42"))
                    self.assertEqual(config.cache_config.block_size, 32)
                    self.assertEqual(exported["cache_groups"], [16, 32])

    def test_discovery_is_a_startup_snapshot_protected_by_native_authentication(self):
        from starlette.applications import Starlette
        from starlette.testclient import TestClient
        from vllm.entrypoints.serve.middleware.authenticate import AuthenticationMiddleware
        from types import SimpleNamespace

        application = Starlette()
        application.state.vllm_config = SimpleNamespace(instance_id="test-instance")
        from vllm.config.kv_events import KVEventsConfig
        source = KVEventsConfig(
            enable_kv_cache_events=True, publisher="zmq", endpoint="ipc://resolved-events",
            replay_endpoint="ipc://resolved-replay", topic="discovered-topic",
        )
        application.state.engine_client = Mock(get_kv_event_sources=Mock(return_value={0: source}))
        application.add_middleware(ModelConfigMiddleware)
        application.add_middleware(AuthenticationMiddleware, tokens=["test-token"])
        resolved = {"encoder": {"models": ["fixture"]}, "tokenizer_json": "exact loaded assets"}
        with tempfile.TemporaryDirectory() as temp, patch("model_config.tempfile.gettempdir", return_value=temp):
            path = prefix_path(application.state.vllm_config)
            path.write_text('{"prefix":{"block_size":16},"cache_groups":[16]}')
            with patch("model_config.model_config", return_value=resolved) as resolve:
                with TestClient(application) as client:
                    self.assertFalse(path.exists())
                    url = "/v1/inferno/model-config?model=fixture"
                    self.assertEqual(client.get(url).status_code, 401)
                    headers = {"Authorization": "Bearer test-token"}
                    for _ in range(2):
                        response = client.get(url, headers=headers)
                        self.assertEqual(response.status_code, 200)
                        self.assertEqual(response.json(), resolved)
                    self.assertEqual(client.get("/v1/inferno/model-config?model=other", headers=headers).status_code, 404)
                    self.assertEqual(client.post(url, headers=headers).status_code, 405)
                    events_url = "/v1/inferno/kv-events?model=fixture"
                    self.assertEqual(client.get(events_url).status_code, 401)
                    response = client.get(events_url, headers=headers)
                    self.assertEqual(response.status_code, 200)
                    self.assertEqual(response.headers["cache-control"], "no-store")
                    events = response.json()
                    self.assertEqual(events["instance_id"], "test-instance")
                    self.assertEqual(events["cache_groups"], [16])
                    self.assertEqual(events["sources"][0]["endpoint"], source.endpoint)
                    self.assertEqual(events["sources"][0]["replay_endpoint"], source.replay_endpoint)
                    self.assertEqual(events["sources"][0]["topic"], source.topic)
                    self.assertEqual(events["sources"][0]["data_parallel_rank"], 0)
                    self.assertEqual(client.get("/v1/inferno/kv-events?model=other", headers=headers).status_code, 404)
                    self.assertEqual(client.post(events_url, headers=headers).status_code, 405)
                resolve.assert_called_once_with(application.state, {"block_size":16})
                application.state.engine_client.get_kv_event_sources.assert_called_once_with()

    def test_missing_native_event_publisher_prevents_readiness(self):
        from starlette.applications import Starlette
        from starlette.testclient import TestClient
        from types import SimpleNamespace
        from vllm.config.kv_events import KVEventsConfig

        for sources in ({}, {0: KVEventsConfig()}, {0: KVEventsConfig(enable_kv_cache_events=True)}):
            with self.subTest(sources=sources), tempfile.TemporaryDirectory() as temp:
                application = Starlette()
                application.state.vllm_config = SimpleNamespace(instance_id="test-instance")
                application.state.engine_client = Mock(get_kv_event_sources=Mock(return_value=sources))
                application.add_middleware(ModelConfigMiddleware)
                with patch("model_config.tempfile.gettempdir", return_value=temp), patch(
                    "model_config.model_config", return_value={"encoder": {"models": ["fixture"]}},
                ):
                    prefix_path(application.state.vllm_config).write_text('{"prefix":{},"cache_groups":[16]}')
                    with self.assertRaisesRegex(ValueError, "requires a native ZMQ KV event publisher with replay"):
                        with TestClient(application):
                            self.fail("native readiness must fail")

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
            prefix_path(application.state.vllm_config).write_text('{"prefix":{},"cache_groups":[16]}')
            with patch("model_config.model_config", side_effect=ValueError("unsupported encoder")):
                with self.assertRaisesRegex(ValueError, "unsupported encoder"):
                    asyncio.run(application({"type":"lifespan", "state":{}}, receive, send))
        self.assertEqual(messages, ["lifespan.startup.failed"])


if __name__ == "__main__":
    unittest.main()
