"""Exercise the locked native publisher's binding, replay, and shutdown lifecycle."""

from contextlib import ExitStack
import json
import time
import unittest
from unittest.mock import patch

import msgspec
import zmq
from vllm.config.kv_events import KVEventsConfig
from vllm.distributed.kv_events import AllBlocksCleared, EventPublisherFactory, KVEventBatch

from serve_mlx import cache_arguments


class KVEventTests(unittest.TestCase):
    def test_launcher_cache_arguments_parse_with_the_locked_native_cli(self):
        from vllm.engine.arg_utils import AsyncEngineArgs
        from vllm.utils.argparse_utils import FlexibleArgumentParser

        parser = AsyncEngineArgs.add_cli_args(FlexibleArgumentParser())
        args = parser.parse_args(cache_arguments({}))
        self.assertTrue(args.enable_prefix_caching)
        self.assertEqual(args.prefix_caching_hash_algo, "sha256_cbor")
        self.assertIsInstance(args.kv_events_config, KVEventsConfig)
        self.assertTrue(args.kv_events_config.enable_kv_cache_events)
        self.assertEqual(args.kv_events_config.publisher, "zmq")
        self.assertEqual(args.kv_events_config.endpoint, "tcp://*:0")
        self.assertEqual(args.kv_events_config.replay_endpoint, "tcp://*:0")

    def test_independent_native_publishers_replay_and_release_assigned_ports(self):
        arguments = cache_arguments({"INFERENCE_KV_EVENTS_TOPIC": 'cache "events" 🦀'})
        config = KVEventsConfig(**json.loads(arguments[arguments.index("--kv-events-config") + 1]))
        addresses = set()
        with ExitStack() as stack, patch("vllm.distributed.kv_events.get_ip", return_value="127.0.0.1"):
            # Concurrent instances, including a nonzero DP rank, must each bind
            # independent ports. No launcher probe/reservation participates.
            for rank in (0, 3):
                publisher = EventPublisherFactory.create(config, data_parallel_rank=rank)
                stack.callback(publisher.shutdown)
                resolved = publisher.get_publisher_config()
                for endpoint in (resolved.endpoint, resolved.replay_endpoint):
                    self.assertTrue(endpoint.startswith("tcp://127.0.0.1:"))
                    self.assertGreater(int(endpoint.rsplit(":", 1)[1]), 0)
                    self.assertNotIn(endpoint, addresses)
                    addresses.add(endpoint)
                with zmq.Context() as context, context.socket(zmq.DEALER) as replay:
                    replay.setsockopt(zmq.LINGER, 0)
                    replay.setsockopt(zmq.RCVTIMEO, 2000)
                    replay.connect(resolved.replay_endpoint)
                    publisher.publish(KVEventBatch(ts=time.time(), events=[AllBlocksCleared()]))
                    deadline = time.monotonic() + 5
                    received = None
                    while received is None and time.monotonic() < deadline:
                        replay.send_multipart([b"", (0).to_bytes(8, "big")])
                        while True:
                            empty, topic, sequence, payload = replay.recv_multipart()
                            self.assertEqual(empty, b"")
                            if int.from_bytes(sequence, "big", signed=True) == -1:
                                break
                            self.assertEqual(topic.decode(), resolved.topic)
                            self.assertEqual(int.from_bytes(sequence, "big"), 0)
                            received = msgspec.msgpack.decode(payload, type=KVEventBatch)
                    self.assertIsNotNone(received)
                    self.assertEqual(received.data_parallel_rank, rank)
                    self.assertIsInstance(received.events[0], AllBlocksCleared)

        # Shutdown owns closing both sockets; those exact addresses can be bound
        # again, without a stale publisher or replay service surviving.
        with zmq.Context() as context:
            for endpoint in addresses:
                with context.socket(zmq.PUB) as socket:
                    socket.bind(endpoint)


if __name__ == "__main__":
    unittest.main()
