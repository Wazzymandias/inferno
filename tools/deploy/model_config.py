"""Publish the loaded model's preparation configuration for gateway startup.

The scheduler owns cache policy; the renderer owns tokenizer and template assets.
A per-instance temporary file carries scheduler state across native processes.
The completed configuration is served from memory, without rendering requests.
"""

import copy
from dataclasses import asdict
import hashlib
import json
from importlib.metadata import version
import tempfile
from pathlib import Path

from vllm.v1.core.sched.async_scheduler import AsyncScheduler

FORMAT = "infergate-vllm-0.31.0-model-config-v1"


def prefix_path(config) -> Path:
    # Native children inherit the deployment's instance_id. This is process
    # coordination only: no directory setting or shared gateway mount is needed.
    identity = hashlib.sha256(config.instance_id.encode()).hexdigest()
    return Path(tempfile.gettempdir()) / f"infergate-prefix-{identity}.json"


def write_json(path: Path, value) -> None:
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, ensure_ascii=False), encoding="utf-8")
    temporary.replace(path)


class ExportingScheduler(AsyncScheduler):
    """Construction-only hook, preserving native sync/async scheduler selection.

    Inheriting AsyncScheduler lets vLLM retain its async scheduling capability
    checks. __new__ returns the actual native instance; no scheduling method is
    overridden and no hook object survives initialization.
    """

    def __new__(cls, vllm_config, *args, **kwargs):
        if vllm_config.diffusion_config is not None:
            raise ValueError("local preparation does not support diffusion schedulers")
        if not vllm_config.cache_config.enable_prefix_caching:
            raise ValueError("this deployment requires prefix caching; the selected native configuration disabled it")
        native_config = copy.copy(vllm_config.scheduler_config)
        native_config.scheduler_cls = None
        scheduler = native_config.get_scheduler_cls()(vllm_config, *args, **kwargs)
        write_json(prefix_path(vllm_config), prefix_config(scheduler))
        return scheduler


def prefix_config(scheduler) -> dict:
    from vllm.utils.hashing import get_hash_fn_by_name
    from vllm.v1.core.kv_cache_utils import resolve_none_hash_seed

    config = scheduler.vllm_config
    algorithm = config.cache_config.prefix_caching_hash_algo
    if algorithm != "sha256_cbor":
        raise ValueError("local preparation requires --prefix-caching-hash-algo sha256_cbor")
    hash_fn = get_hash_fn_by_name(algorithm)
    # Read the resolved scheduler granularity, including hybrid cache layouts.
    # sha256_cbor has a deterministic seed unless PYTHONHASHSEED overrides it.
    return {
        "algorithm": algorithm,
        "block_size": scheduler.hash_block_size,
        "initial_parent": list(hash_fn(resolve_none_hash_seed(hash_fn))),
    }


def model_config(state, prefix: dict) -> dict:
    from vllm.renderers.hf import HfRenderer, resolve_chat_template, resolve_chat_template_content_format
    from vllm.entrypoints.openai.responses.protocol import ResponsesRequest

    if version("vllm").split("+")[0] != "0.31.0":
        raise ValueError("model configuration requires the locked vLLM 0.31.0 release")
    config = state.vllm_config
    renderer = state.engine_client.renderer
    online = state.online_renderer
    if not isinstance(renderer, HfRenderer) or online.use_harmony:
        raise ValueError("local preparation currently requires the native Hugging Face renderer")
    if config.model_config.is_encoder_decoder:
        raise ValueError("local preparation does not support encoder-decoder models")
    if config.lora_config is not None or config.model_config.enable_prompt_embeds:
        raise ValueError("local preparation does not yet support adapters or prompt embeddings")
    if config.parallel_config.data_parallel_size != 1 or config.parallel_config._api_process_count != 1:
        raise ValueError("model configuration discovery requires a single native data/API process")
    tokenizer = renderer.get_tokenizer()
    if not tokenizer.is_fast or not hasattr(tokenizer, "backend_tokenizer"):
        raise ValueError("local preparation requires a Rust-backed tokenizer")
    # Save the loaded tokenizer, including native-added tokens and its effective
    # postprocessor. Loading a guessed Hub file could produce different IDs.
    tokenizer_bytes = tokenizer.backend_tokenizer.to_str().encode()

    def template(tools):
        source = resolve_chat_template(tokenizer, online.chat_template, tools, model_config=config.model_config)
        if not source:
            raise ValueError("the native renderer did not resolve a chat template")
        if "strftime_now" in source:
            raise ValueError("time-dependent chat templates cannot be reproduced independently")
        return {
            "source": source,
            "content_format": resolve_chat_template_content_format(
                chat_template=online.chat_template, tools=tools,
                given_format=online.chat_template_content_format,
                tokenizer=tokenizer, model_config=config.model_config,
            ),
        }

    # Native template selection branches on tools being present. Content and
    # schemas are supplied per request; no synthetic tool enters preparation.
    sample_tools = [{"type": "function", "function": {"name": "export_template_selection"}}]
    native_request = ResponsesRequest(input="")
    tok_params = native_request.build_tok_params(config.model_config)
    if tok_params.do_lower_case or tok_params.pad_prompt_tokens is not None:
        raise ValueError("local preparation does not support native lowercase or padding overrides")
    defaults = online.default_chat_template_kwargs or {}
    if set(defaults) & {"tokenize", "tokenizer_kwargs", "tools", "documents", "return_tensors"}:
        raise ValueError("unsupported native chat template parameter override")
    return {
        "format": FORMAT,
        "encoder": {
            "models": state.args.served_model_name or [state.args.model],
            "max_model_len": config.model_config.max_model_len,
            "add_special_tokens": tok_params.add_special_tokens,
            "without_tools": template(None),
            "with_tools": template(sample_tools),
            "special_tokens": tokenizer.special_tokens_map,
            "template_kwargs": defaults,
            "exclude_tools_when_tool_choice_none": online.exclude_tools_when_tool_choice_none,
            "tokenizer_sha256": list(hashlib.sha256(tokenizer_bytes).digest()),
        },
        "prefix": prefix,
        "tokenizer_json": tokenizer_bytes.decode(),
    }


class ModelConfigMiddleware:
    """Resolve once before readiness; publish through the app's normal middleware."""

    def __init__(self, app):
        self.app = app

    async def __call__(self, scope, receive, send):
        if scope["type"] != "lifespan":
            await self.app(scope, receive, send)
            return

        async def publish_before_ready(message):
            if message["type"] == "lifespan.startup.complete":
                from starlette.responses import JSONResponse, Response
                application = scope["app"]
                path = prefix_path(application.state.vllm_config)
                resolved = model_config(application.state, json.loads(path.read_text()))
                path.unlink()
                body = json.dumps(resolved, ensure_ascii=False).encode()
                models = frozenset(resolved["encoder"]["models"])
                # The engine handshake carries the publisher's bound addresses.
                # Input configuration still contains :0 and cannot be used by
                # subscribers. Keep process-specific discovery out of the model
                # asset snapshot that the gateway also uses offline.
                sources = application.state.engine_client.get_kv_event_sources()
                if not sources or any(
                    not source.enable_kv_cache_events or source.publisher != "zmq" or not source.replay_endpoint
                    for source in sources.values()
                ):
                    raise ValueError("this deployment requires a native ZMQ KV event publisher with replay")
                events_body = json.dumps({
                    "instance_id": application.state.vllm_config.instance_id,
                    "sources": [
                        {"data_parallel_rank": rank, **asdict(source)}
                        for rank, source in sorted(sources.items())
                    ],
                }).encode()

                async def configuration(request):
                    if request.query_params.get("model") not in models:
                        return JSONResponse({"error": "model is not served by this deployment"}, status_code=404)
                    return Response(body, media_type="application/json")

                async def events(request):
                    if request.query_params.get("model") not in models:
                        return JSONResponse({"error": "model is not served by this deployment"}, status_code=404)
                    return Response(events_body, media_type="application/json", headers={"Cache-Control": "no-store"})

                # Register inside the application so native authentication and
                # other middleware apply to discovery exactly as to inference.
                application.add_route("/v1/infergate/model-config", configuration, methods=["GET"])
                application.add_route("/v1/infergate/kv-events", events, methods=["GET"])
            await send(message)

        await self.app(scope, receive, publish_before_ready)
