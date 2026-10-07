# Infergate

Infergate forwards native vLLM Responses requests through a Rust gateway and
provides a React chat interface. The gateway lives in
[`services/gateway`](services/gateway/README.md).

## Requirements

For `just deploy`, install [Just](https://github.com/casey/just),
[uv](https://docs.astral.sh/uv/getting-started/installation/), and
[Docker Desktop](https://docs.docker.com/desktop/setup/install/mac-install/).
Native vLLM Metal requires an Apple Silicon Mac with macOS 15 or newer.
Docker runs the web app and gateway; vLLM runs directly on the host.

Native service development additionally uses [Bun](https://bun.sh/get),
[Rust nightly](https://www.rust-lang.org/tools/install), and
[cargo-nextest](https://nexte.st/docs/installation/).

## Quickstart

```sh
just deploy
```

`--dev` is optional and defaults to true; `just deploy --dev` runs the same
development stack. Use `just deploy down` or `just deploy down --dev` to stop it.

On first use, the command creates an untracked `.env` from `.env.example`,
installs the locked native inference dependencies, starts vLLM, waits for model
readiness, and builds and starts Compose. Open `http://localhost:3000` to chat.
The browser connects through the gateway at `http://localhost:8080/v1`.

Detached mode is enabled by default; `just deploy`, `just deploy -d`, and
`just deploy --detached` all start in the background and return.
Follow startup progress and live vLLM and Docker output with
`tail -f tools/deploy/deploy.log`. The log announces readiness after the native
model and Compose services are ready; failures and cleanup appear there too.
The log is ignored by Git.

Use `just deploy --detached=false` to keep the deployment and its live output in
the terminal. Ctrl-C stops the Compose services and native server's workers,
retaining containers and persistent volumes. `just deploy down` stops the
project's deployment and native vLLM servers, including during startup, and
streams `docker compose down` progress as it removes containers and the network.
Persistent volumes are retained.
Deployment code lives in
[`tools/deploy`](tools/deploy/serve_mlx.py).

The current dependency lock uses [vLLM 0.31.0](https://github.com/vllm-project/vllm/releases/tag/v0.31.0)
and the matching [Metal prerelease](https://github.com/vllm-project/vllm-metal/releases/tag/v0.31.0.dev20261006214222).
The exact wheel URLs and dependency hashes are locked; startup does not select
an unpinned latest version. uv provisions the required Python version.

Choose a Hugging Face repository ID or local snapshot using `INFERENCE_MODEL`
in `.env`. `INFERENCE_PORT` owns the native listener port; `just deploy` derives
Compose's backend connection from it and supplies the same served model ID to
both vLLM and the web app. The example uses a small Llama checkpoint. First
startup downloads weights if they are not cached.

Arguments override the configured model and port for that session:

```sh
just deploy --model your-hf-model --port 8002
```

`VLLM_ARGS` in `.env` configures native vLLM options. Additional options can be
passed after `--`. The example limits the development KV cache to 1,024 native
blocks and lets vLLM fit the context length to its actual cache layout. Adjust
that capacity for your model and memory budget. vLLM owns the model's chat template, message content format,
and tokenization; the deployment command exposes `/v1/responses/render` on the
same inference listener. Automatic tool calls use the Hermes parser.

For Qwen text chat, configure its native options explicitly:

```dotenv
INFERENCE_MODEL=lmstudio-community/Qwen3.8-27B-MLX-4bit
VLLM_ARGS=--num-gpu-blocks-override 1024 --max-model-len auto --language-model-only --reasoning-parser qwen3
```

`--language-model-only` serves text without loading the checkpoint's image
processor. `--reasoning-parser qwen3` separates reasoning from the answer.
Model, listener, served name, and render API availability belong to the
launcher; its additional arguments cannot override those settings.

To use an already-running native vLLM server, set `INFERENCE_ENDPOINT` and
`INFERENCE_MODEL` in `.env`, then run `docker compose up -d --wait`.
Compose manages only the application and optional telemetry services.

### Chat in the browser

The React app in [`apps/web`](apps/web/README.md) streams Responses API replies.
`INFERENCE_TEMPERATURE` and `INFERENCE_MAX_OUTPUT_TOKENS` in `.env` configure
sampling and reply length. The web app owns their defaults and validation;
Compose passes overrides. `WEB_HOST_ADDR`, `WEB_HOST_PORT`, and `WEB_PORT`
configure the web listener and published address. `WEB_INFERENCE_ENDPOINT`
and `WEB_INFERENCE_API_KEY` select and authenticate another inference service.

For standalone frontend development:

```sh
cd apps/web
bun install --frozen-lockfile
bun run dev --inference-endpoint http://localhost:8080/v1 --model your-model-id
```

### Telemetry

Set `COMPOSE_PROFILES=metrics` in your untracked `.env`, then run:

```sh
just deploy
```

The root Compose file statically includes
[`telemetry/compose.yaml`](telemetry/compose.yaml). The `metrics` profile enables
SigNoz, its OTel collector, ClickHouse, ClickHouse Keeper, PostgreSQL, and both
setup jobs on the app's network. `.env.example` defaults to
`COMPOSE_PROFILES=`, so telemetry is disabled by default. Remove `metrics`
from the profile list to disable telemetry. An empty or unset profile list
enables only the web and API in Compose.

A one-off initialization container generates a random database password and
retains it in a Docker volume. PostgreSQL and SigNoz read it through read-only
mounts. `SIGNOZ_POSTGRES_PASSWORD` optionally supplies your own URL-safe password
on first startup; keep the existing value if upgrading an initialized deployment.
`SIGNOZ_POSTGRES_DSN` can override SigNoz's database URI;
`SIGNOZ_CLICKHOUSE_DSN` configures the ClickHouse connection used by SigNoz, the
collector, and the migration job. A one-off migration container initializes and
upgrades the telemetry schema before SigNoz and the collector start. Named volumes
retain credentials, telemetry, and account data across container restarts.
Allocate at least 4 GB of Docker memory for SigNoz, in addition to the app and
model's requirements.

Open `http://localhost:8081` and create your SigNoz account. SigNoz then activates
the collector's ingestion pipelines through OpAMP. `SIGNOZ_HOST` defaults
to `127.0.0.1`; `SIGNOZ_UI_PORT`, `SIGNOZ_OTLP_GRPC_PORT`, and
`SIGNOZ_OTLP_HTTP_PORT` configure the published UI and ingestion ports. ClickHouse,
Keeper, and PostgreSQL have no published host ports.

Instrumented services on the Compose network can export to
`http://signoz-otel-collector:4317` (OTLP/gRPC) or
`http://signoz-otel-collector:4318` (OTLP/HTTP). Host processes use `localhost`
and the corresponding published port. Configure the SDK's OTLP protocol to
match the endpoint. This starts the telemetry backend; application instrumentation
and cloud collection agents must be configured separately.

The `SIGNOZ_IMAGE`, `SIGNOZ_COLLECTOR_IMAGE`, `SIGNOZ_CLICKHOUSE_IMAGE`,
`SIGNOZ_KEEPER_IMAGE`, and `SIGNOZ_POSTGRES_IMAGE` settings in `.env.example`
lock upstream dependencies by digest. The configurations are adapted from
[Foundry v0.3.0](https://github.com/SigNoz/foundry/tree/v0.3.0/docs/examples/docker/compose),
and run directly with Compose; Foundry is not a runtime dependency. The ClickHouse
build installs SigNoz's histogram function v0.0.1 with architecture-specific
SHA-256 verification, so container startup does not download executables.

With the `metrics` profile enabled, inspect or stop the deployment with ordinary
Compose commands:

```sh
docker compose ps -a
docker compose logs signoz signoz-otel-collector signoz-migrate
docker compose down
```

`down` retains named volumes; adding `--volumes` deletes credentials, stored
telemetry, and SigNoz accounts. To disable a running telemetry stack, stop it while
the `metrics` profile is enabled before removing it from `COMPOSE_PROFILES`;
changing the profile list alone does not stop existing containers. For a
telemetry-only deployment, run
`docker compose -f telemetry/compose.yaml --profile metrics up -d --wait`.

### Check, debug, or stop

| Task | Command or endpoint |
| --- | --- |
| Check API process | `/healthz` |
| Check backend reachability | `/readyz` |
| Check web process | `docker compose ps web` |
| Web logs | `docker compose logs web` |
| API logs | `docker compose logs api` |
| Deployment and model logs | `tail -f tools/deploy/deploy.log` |
| Run with live terminal output | `just deploy --detached=false` |
| Stop a foreground session | Ctrl-C in its terminal |
| Stop native models and remove application containers | `just deploy down` |

For native Rust runs, rendering inspection, and gateway settings, see
[gateway configuration](services/gateway/README.md).
