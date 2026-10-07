"""Generate the Rust adapter's wire fixtures using the locked vLLM event types."""

import json
from pathlib import Path

import msgspec
from vllm.distributed.kv_events import AllBlocksCleared, BlockRemoved, BlockStored, KVEventBatch


def main():
    testdata = Path(__file__).resolve().parents[2] / "services/gateway/src/testdata"
    cases = json.loads((testdata / "input-string/parity.json").read_text())
    case = next(case for case in cases if case["name"] == "unicode-whitespace")
    hashes = [bytes.fromhex(value) for value in case["expected"]["prefix_hashes"][:3]]
    base = dict(parent_block_hash=None, token_ids=case["expected"]["token_ids"][:24],
                block_size=8, lora_id=None, lora_name=None, medium="GPU", group_idx=0)
    events = {
        "stored": [BlockStored(block_hashes=hashes, **base)],
        "removed": [BlockRemoved(block_hashes=[hashes[1]], medium="GPU", group_idx=0)],
        "cleared": [AllBlocksCleared()],
        "integer-hash": [BlockStored(block_hashes=[123], **base)],
        "short-hash": [BlockStored(block_hashes=[b"bad"], **base)],
        "remote": [BlockStored(block_hashes=hashes, locality="REMOTE", **base)],
        "cpu": [BlockRemoved(block_hashes=hashes, medium="CPU", group_idx=None)],
    }
    destination = testdata / "kv-events"
    destination.mkdir(exist_ok=True)
    for name, batch in events.items():
        (destination / f"{name}.msgpack").write_bytes(msgspec.msgpack.encode(
            KVEventBatch(ts=1.0, events=batch, data_parallel_rank=0),
        ))


if __name__ == "__main__":
    main()
