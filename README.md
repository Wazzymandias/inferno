# Infergate

Infergate is a multi-model LLM gateway that runs "pre-inference" on user requests for intelligent routing. 
The Rust gateway lives in [`services/gateway`](services/gateway/README.md).

## Requirements

### Frontend

- [Bun](https://bun.sh/get)

### Gateway
- [Rust nightly](https://www.rust-lang.org/tools/install)
- [cargo-nextest](https://nexte.st/docs/installation/)
- [Just 1.31+](https://github.com/casey/just) 
- [Docker Compose 2.38+](https://docs.docker.com/compose/install/)

> [!WARNING]
> Apple Silicon is supported. For other CPUs, check [Graviola's hardware requirements](https://docs.rs/graviola/0.4.1/graviola/#limitations) before building.

### Apple Silicon
For local inference with vLLM Metal:

- An Apple Silicon Mac.
- [Docker Desktop 4.62+](https://docs.docker.com/desktop/setup/install/mac-install/).
- [macOS 15+](https://support.apple.com/en-us/108382).

## Quickstart

On Apple Silicon, use Docker Model Runner with vLLM Metal:

**1. Set up Docker once.**

```sh
docker desktop enable model-runner
docker model install-runner --backend vllm
```

**2. Start the app.**

```sh
cp .env.example .env
docker compose up -d
```

Open the chat at `http://localhost:3000`. The `web` service and its container are
both named `web`; the browser sends inference requests through the gateway.

The example includes a small model. To use another, change `INFERENCE_MODEL`
in `.env` before starting. First startup downloads the model and builds the API;
the first request loads the model into memory.

**3. Send a request** (requires `jq`).

```sh
. ./.env
jq -n --arg model "$INFERENCE_MODEL" \
  '{model:$model,input:[{role:"user",content:"Hello"}],max_output_tokens:32,stream:true,store:false}' |
  curl -N "http://localhost:${API_HOST_PORT}/v1/responses" \
    -H 'Content-Type: application/json' --data-binary @-
```

The default API URL is `http://localhost:8080/v1`. Use `INFERENCE_MODEL` in
requests; this setup's `/v1/models` response may contain an ID that vLLM rejects.

### Chat in the browser

The React app in [`apps/web`](apps/web/README.md) uses the Responses API. Its
inference service is selected with `--inference-endpoint`, with no built-in
gateway address. The backend behind that endpoint must support `/responses`.

```sh
cd apps/web
bun install --frozen-lockfile
bun run dev --inference-endpoint http://localhost:8080/v1 --model your-model-id
```

Open `http://localhost:3000`. Replace `your-model-id` with the backend's accepted
ID (the `INFERENCE_MODEL` value in your root `.env` for the local setup).

`docker compose up -d --build` starts the web app and gateway together. The web
service derives its default endpoint from the API service's configured port and uses the same
`INFERENCE_MODEL`. `WEB_INFERENCE_ENDPOINT` selects another service;
`WEB_INFERENCE_API_KEY` supplies its optional server-side bearer token.
Chat explicitly uses greedy decoding (`INFERENCE_TEMPERATURE=0`) and a generated
token budget per reply (`INFERENCE_MAX_OUTPUT_TOKENS=512`, including reasoning).
Both settings are validated by the web app, which owns their defaults; Compose
passes overrides from `.env`. Increase the reply budget for longer answers.
Apply changed chat settings with `docker compose up -d --no-deps web`.
`WEB_HOST_ADDR` and `WEB_HOST_PORT` set the published bind address and port;
`WEB_PORT` sets the container's listener port. `BUN_IMAGE` selects the declared
Bun build and runtime image.

### SigNoz telemetry

Add `metrics` to `COMPOSE_PROFILES` in your untracked `.env`: use
`COMPOSE_PROFILES=local,metrics` with the local model, or
`COMPOSE_PROFILES=metrics` with an external backend. Then run:

```sh
docker compose up -d
```

The root Compose file statically includes
[`telemetry/compose.yaml`](telemetry/compose.yaml). The `metrics` profile enables
SigNoz, its OTel collector, ClickHouse, ClickHouse Keeper, PostgreSQL, and both
setup jobs on the app's network. `.env.example` defaults to
`COMPOSE_PROFILES=local`, so telemetry is disabled by default. Remove `metrics`
from the profile list to disable telemetry; keep `local` to retain local model
provisioning. An empty or unset `COMPOSE_PROFILES` enables only the web and API.

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
| Check backend reachability | `/readyz` (does not confirm model loading) |
| Check web process | `docker compose ps web` |
| Web logs | `docker compose logs web` |
| API logs | `docker compose logs api` |
| Model logs | `docker model logs` |
| Stop the app | `docker compose down` |



**Already have a backend?** Set `COMPOSE_PROFILES=` (or `metrics` to enable SigNoz) and
`INFERENCE_ENDPOINT=<your API base URL>` in `.env`, then run `docker compose up -d`.
This skips local model provisioning. For native Rust runs and other settings,
see [gateway configuration](services/gateway/README.md#cli-and-runtime-configuration).
