# Infergate

Infergate is a multi-model LLM gateway that runs inference on user requests for intelligent routing. The Rust gateway lives in
[`services/gateway`](services/gateway/README.md).

## Requirements

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
see [gateway configuration](services/gateway/README.md#cli-and-runtime-configuration).
