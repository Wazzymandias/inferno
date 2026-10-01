# Infergate

## Requirements

- [Rust nightly](https://www.rust-lang.org/tools/install)
- [Justfile](https://github.com/casey/just)
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

**2. Start the API.**

```sh
cp .env.example .env
docker compose up -d
```

The example includes a small model. To use another, change `INFERENCE_MODEL`
in `.env` before starting. First startup downloads the model and builds the API;
the first request loads the model into memory.

**3. Send a request** (requires `jq`).

```sh
. ./.env
jq -n --arg model "$INFERENCE_MODEL" \
  '{model:$model,messages:[{role:"user",content:"Hello"}],max_tokens:32,stream:true}' |
  curl -N "http://localhost:${API_HOST_PORT}/v1/chat/completions" \
    -H 'Content-Type: application/json' --data-binary @-
```

The default API URL is `http://localhost:8080/v1`. Use `INFERENCE_MODEL` in
requests; this setup's `/v1/models` response may contain an ID that vLLM rejects.

**Check, debug, or stop:**

| Task | Command or endpoint |
| --- | --- |
| Check API process | `/healthz` |
| Check backend reachability | `/readyz` (does not confirm model loading) |
| API logs | `docker compose logs api` |
| Model logs | `docker model logs` |
| Stop API | `docker compose down` |

Docker Desktop manages the model separately and retains downloaded models.
The example exposes the API on all interfaces. Host model memory is outside
API container limits; see [the memory observations in SETUP.md](SETUP.md).

**Already have a backend?** Set `COMPOSE_PROFILES=` and
`INFERENCE_ENDPOINT=<your API base URL>` in `.env`, then run `docker compose up -d`.
This skips local model provisioning. For native Rust runs and other settings,
see [CLI and runtime configuration](#cli-and-runtime-configuration).

## CLI and runtime configuration

Argument parsing and configuration live in `src/cli/config.rs`, with `src/cli.rs`
as the module entry point, and use [bpaf](https://docs.rs/bpaf/).
Run `cargo run -- --help` or `cargo run -- --version` without configuring a backend.
Flags override environment variables; omitted values use the defaults below.
Invalid configuration fails before the listener opens.

| Flag | Environment variable | Default |
| --- | --- | --- |
| `--api-address` | `API_ADDRESS` | Required |
| `--api-port` | `API_PORT` | Required |
| `--inference-endpoint` | `INFERENCE_ENDPOINT` | Required |
| `--inference-timeout-seconds` | `INFERENCE_TIMEOUT_SECONDS` | `300` |
| `--max-request-bytes` | `MAX_REQUEST_BYTES` | `1048576` |
| `--shutdown-timeout-seconds` | `SHUTDOWN_TIMEOUT_SECONDS` | `5` |

Timeouts and body limits must be positive. The backend URL must use HTTP(S),
without a query or fragment. Port `0` selects an available port for native runs.
The binary reads the process environment; it does not load `.env` automatically.
Compose reads `.env` and passes the settings to the container. For a native run:

```sh
API_ADDRESS=127.0.0.1 API_PORT=8080 INFERENCE_ENDPOINT=http://localhost:8000/v1 \
  cargo run --locked -- --inference-timeout-seconds 120
```
