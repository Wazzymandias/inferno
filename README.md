# Infergate

Infergate is a multi-model LLM gateway that runs inference on user requests for intelligent routing. The Rust gateway lives in
[`services/gateway`](services/gateway/README.md).

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
`WEB_HOST_ADDR` and `WEB_HOST_PORT` set the published bind address and port;
`WEB_PORT` sets the container's listener port. `BUN_IMAGE` selects the declared
Bun build and runtime image.

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



**Already have a backend?** Set `COMPOSE_PROFILES=` and
`INFERENCE_ENDPOINT=<your API base URL>` in `.env`, then run `docker compose up -d`.
This skips local model provisioning. For native Rust runs and other settings,
see [gateway configuration](services/gateway/README.md#cli-and-runtime-configuration).
