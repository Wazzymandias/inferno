# Inferno

Inferno gateway routes multi-model Response API requests using "pre-inference" for enhanced routing and
provides a React chat interface. The gateway lives in
[`services/gateway`](services/gateway/README.md).

> ⚠️NOTE: Inferno local models, builds and development require MacOS with Apple Silicon

## Requirements

- [Just](https://github.com/casey/just) — runs the project commands.
- [Rust nightly](https://www.rust-lang.org/tools/install) with Clippy and rustfmt —
  for native gateway development. [rust-toolchain.toml](rust-toolchain.toml)
  selects `nightly` without pinning a date.
- [Bun 1.4.2](https://bun.sh/get) — for native web development; matches the
  project's container image.
- [uv](https://docs.astral.sh/uv/getting-started/installation/) — installs the
  locked native inference environment and provisions Python.
- [Docker Desktop with Docker Compose](https://docs.docker.com/desktop/setup/install/mac-install/) —
  runs the web app, gateway, and optional telemetry services.
- [cargo-nextest](https://nexte.st/docs/installation/) — runs the gateway tests.
- **Apple Silicon Mac with macOS 15 or newer** — required for native vLLM Metal.
- **Apple Command Line Tools** — provides the C++ compiler for the Metal
  extension. Install with `xcode-select --install`.
- **Git** — fetches the pinned Metal source; included in Apple's Command Line Tools.
- **Python 3.12** — provisioned automatically by uv; no separate install needed.
- **Ninja 1.13.2** — installed automatically by uv with the native dependencies.
- **Chromium** — installed automatically by `just web test` for browser tests.

## Quickstart

```sh
just deploy up
```

Open **[localhost:3000](http://localhost:3000)** to chat. Chat requests pass through
the gateway, available at [localhost:8080/v1](http://localhost:8080/v1).

**On first startup, the launcher:**

1. Creates an untracked `.env` from [.env.example](.env.example).
2. Installs the locked inference dependencies and builds the Metal extension.
3. Starts native vLLM and waits for the model to be ready.
4. Builds and starts the Compose services.

Model loading happens before containers start and may take several minutes.
vLLM runs on the host; Docker runs the web app and gateway.

**Jump to:** [Deployment](#deployment) · [Native inference](#native-inference-on-apple-silicon) ·
[Existing server](#use-an-existing-vllm-server) · [Web app](#chat-in-the-browser) ·
[Telemetry](#telemetry) · [Checks and logs](#check-debug-or-stop)

## Deployment

### Commands

| Task | Command |
| --- | --- |
| Start and return when ready | `just deploy up` or `just deploy` |
| Explicitly select detached mode | `just deploy -d` or `just deploy --detached` |
| Keep streaming until shutdown | `just deploy --detached=false` |
| Stop the project and remove its containers | `just deploy down` |
| Show startup options | `just deploy up --help` |

- `--dev` selects the same development stack.
- Native vLLM options go after `--`.
- The launcher handles both `up` and `down` and rejects unknown commands or
  options before starting services.
- `down` does not install or build native inference dependencies.
- Deployment code lives in [tools/deploy](tools/deploy/serve_mlx.py).

### Readiness and shutdown

| Action | What happens |
| --- | --- |
| Start in detached mode (default) | Streams startup output and returns successfully only after the model and Compose services are ready. Services keep running in the background. |
| Start with `--detached=false` | Streams output for the deployment's lifetime. |
| Startup fails | Returns a nonzero exit status. |
| Press Ctrl-C during startup | Cancels startup and streams cleanup output. |
| Press Ctrl-C in a foreground session | Stops Compose services and native vLLM workers. Keeps containers and persistent volumes. |
| Run `just deploy down` | Stops this project's deployment and native vLLM servers, even during startup. Removes containers and the network; keeps persistent volumes. |

### Logs

- Commands stream launcher progress and live vLLM and Docker output.
- The same output is saved to `tools/deploy/deploy.log`, beside the launcher.
- Each invocation prints the resolved log path and streams only newly appended output.
- Detached services keep writing to the log after startup returns.
- Shutdown output is also streamed and saved. The log is ignored by Git.

## Native inference on Apple Silicon

### Choose a model and port

Set these in the root `.env`:

| Setting | Purpose |
| --- | --- |
| `INFERENCE_MODEL` | Hugging Face repository ID or local snapshot directory. |
| `INFERENCE_PORT` | Native vLLM listener port. |
| `VLLM_ARGS` | Additional native vLLM options. |

- `just deploy` derives Compose's backend URL from `INFERENCE_PORT`.
- It supplies the same served model ID to vLLM and the web app.
- The example uses a small Llama checkpoint. Weights download on first startup
  if they are not already cached.

Override the configured model and port for one session:

```sh
just deploy --model your-hf-model --port 8002
```

Pass additional native options after `--`:

```sh
just deploy -- --max-num-seqs 2
```

The example `VLLM_ARGS` limits the development KV cache to **1,024 native blocks**
and lets vLLM fit the context length to that cache layout. Adjust the capacity
for your model and memory budget.

### Qwen text chat

Configure the model and native options explicitly:

```dotenv
INFERENCE_MODEL=lmstudio-community/Qwen3.8-27B-MLX-4bit
VLLM_ARGS='--num-gpu-blocks-override 1024 --max-model-len auto --language-model-only --reasoning-parser qwen3'
```

- `--language-model-only` serves text without loading the checkpoint's image processor.
- `--reasoning-parser qwen3` separates reasoning from the answer.

### Request preparation and cache policy

- **vLLM** owns the chat template, message content format, and tokenization policy.
- **Gateway startup** discovers and caches the loaded tokenizer, selected
  templates, and resolved prefix-hash policy from the native server.
- **Each create request** gets its tokens and hashes prepared locally in Rust
  before forwarding. There is no per-request render RPC.
- **`/v1/responses/render`** is exposed for parity verification only.
- **Automatic tool calls** use the Hermes parser.

The launcher owns the model, listener, served name, render API, model
configuration discovery, and cache policy:

- It enables prefix caching with `sha256_cbor` for reproducible native/Rust hashes.
- It enables the native ZMQ KV event publisher with replay and sets
  `VLLM_KV_EVENTS_USE_INT_BLOCK_HASHES=0` to retain all 32 hash bytes.
- Extra arguments cannot override these settings, the scheduler hook, or the
  discovery middleware.
- Native `--config` files are not accepted. Use `.env` and `VLLM_ARGS`.
- Startup fails if the model or Metal configuration disables prefix caching.
- Cache discovery supports full attention, sliding windows, and recurrent state
  checkpoints. Unsupported lookup policies, including speculative decoding,
  fail at startup rather than advertise incorrect reuse requirements.

### KV events

Set these in `.env` or export them in the shell:

| Setting | Unset or empty value | Purpose |
| --- | --- | --- |
| `INFERENCE_KV_EVENTS_ENDPOINT` | `tcp://*:0` | Complete native ZMQ event endpoint. |
| `INFERENCE_KV_EVENTS_REPLAY_ENDPOINT` | `tcp://*:0` | Complete native ZMQ replay endpoint. |
| `INFERENCE_KV_EVENTS_TOPIC` | `kv-events` | Event topic. |
| `VLLM_HOST_IP` | Auto-detected | Native node address for OS-assigned KV endpoints. |

**Port ownership and routing:**

- vLLM binds independent OS-assigned ports for `tcp://*:0`. The launcher does
  not reserve ports or derive them from the HTTP port.
- Compose resolves configuration for the launcher. The publisher runs in native
  vLLM, outside the containers.
- Endpoints and replay buffers last only as long as the native server process.
- The gateway subscribes to discovered event sources and ranks replicas by
  reusable prefix tokens minus active-request load, including streaming.
- Set `INFERENCE_ENDPOINT` to comma-separated replica API URLs and tune
  `ROUTING_LOAD_PENALTY` (default `256`, positive) in `.env`. The launcher starts
  one local replica; configure external replicas when running Compose directly.
- Sequence gaps, disconnects, evictions, and clears invalidate affected cache
  credit. Replay reconstructs disposable state without a shared database.
- Replicas must share model preparation and hash policy. Each group's block size
  and required history come from its native cache manager. Every group must be
  reusable at the same prefix boundary: full attention needs the entire prefix,
  sliding windows need their contiguous tail, and recurrent states need the
  checkpoint at that boundary.

**Discover the active publisher** on the native server, using its normal API authentication:

```http
GET /v1/inferno/kv-events?model=<served-model>
```

- The response contains resolved endpoints, the topic, the native `instance_id`,
  `cache_groups`, and `sources`. Each cache group specifies `block_size` and
  `required_blocks` (null for the entire prefix, otherwise the required trailing
  blocks). Each source includes its `data_parallel_rank` and native publisher
  configuration.
- Rediscover after every native restart; endpoints are process-scoped.
- For `tcp://*:0`, vLLM advertises `VLLM_HOST_IP` when set, or detects the node
  address otherwise. Subscribers must be able to reach that address.
- For an explicit wildcard bind endpoint, subscribers must substitute a
  reachable server address.

### Locked dependencies and Metal builds

| Dependency | Locked version |
| --- | --- |
| vLLM | [0.31.0](https://github.com/vllm-project/vllm/releases/tag/v0.31.0) |
| vLLM Metal | [Source commit `27cfcd8b6b6f4a3daf89ca7e30597517efa57a2d`](https://github.com/vllm-project/vllm-metal/commit/27cfcd8b6b6f4a3daf89ca7e30597517efa57a2d) |

- **Repeatable installs:** upstream deletes previous Metal development releases,
  so their wheel URLs are unreliable. uv locks the matching source revision and
  dependencies and provisions the required Python version.
- **Extension build:** the launcher builds the native extension before starting
  services. It reuses the build while sources and dependencies still match.
- **Shader compilation:** `VLLM_METAL_BUILD_FROM_SOURCE=1` enables upstream's
  source mode. MLX compiles shaders during native worker warm-up.
- **Tooling:** source mode requires neither full Xcode nor the standalone Metal
  shader compiler.

## Use an existing vLLM server

1. Enable the [model configuration integration](services/gateway/README.md#automatic-model-configuration)
   on the native server.
2. Set `INFERENCE_ENDPOINT` and `INFERENCE_MODEL` in `.env`.
3. Set `INFERENCE_API_KEY` if the backend requires authentication.
4. Start Compose:

   ```sh
   docker compose up -d --wait
   ```

Compose manages the application and optional telemetry services. The existing
native server runs separately.

## Chat in the browser

The React app in [apps/web](apps/web/README.md) streams Responses API replies.
Configure it through the root `.env`:

| Setting | Purpose |
| --- | --- |
| `INFERENCE_TEMPERATURE` | Sampling temperature. |
| `INFERENCE_MAX_OUTPUT_TOKENS` | Reply length budget. |
| `WEB_HOST_ADDR` | Host address for the published web port. |
| `WEB_HOST_PORT` | Web port published on the host. |
| `WEB_PORT` | Web service listener port. |
| `WEB_INFERENCE_ENDPOINT` | Select another inference service. |
| `WEB_INFERENCE_API_KEY` | Authenticate with that inference service. |

The web app owns generation defaults and validation; Compose passes overrides.

For standalone frontend development:

```sh
cd apps/web
bun install --frozen-lockfile
bun run dev --inference-endpoint http://localhost:8080/v1 --model your-model-id
```

## Telemetry

### Enable SigNoz

1. Set `COMPOSE_PROFILES=metrics` in your untracked `.env`.
2. Allocate **at least 4 GB of Docker memory** for SigNoz, in addition to the
   app and model's requirements.
3. Run `just deploy`.
4. Open **[localhost:8081](http://localhost:8081)** and create your SigNoz account.
   SigNoz then activates the collector's ingestion pipelines through OpAMP.

See the [telemetry guide](docs/TELEMETRY.md) for configuration, storage,
export endpoints, and shutdown instructions.

## Check, debug, or stop

| Task | Command or endpoint |
| --- | --- |
| Check API process | `/healthz` |
| Check gateway readiness | `/readyz` |
| Check web process | `docker compose ps web` |
| Web logs | `docker compose logs web` |
| Gateway logs | `docker compose logs gateway` |
| Deployment and model logs | Streamed by deployment commands; retained at the log path printed by the launcher. |
| Run with live terminal output | `just deploy --detached=false` |
| Verify the web app through the configured model | `just web test` |
| Stop a foreground session | Ctrl-C in its terminal. |
| Stop native models and remove application containers | `just deploy down` |

For native Rust runs, rendering inspection, and gateway settings, see
[gateway configuration](services/gateway/README.md).
