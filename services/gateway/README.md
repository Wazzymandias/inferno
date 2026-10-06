# Gateway

The `infergate` Rust service forwards inference requests and streams backend
responses. It exposes `/healthz`, `/readyz`, and the `/v1` API.

The package, source, tests, Justfile, and Dockerfile live in this directory. The
repository root owns the Cargo workspace, lockfile, Rust toolchain, Compose
configuration, and a Justfile that exposes service command modules. Run these
commands from the repository root:

```sh
just gateway build
just gateway test
just gateway lint
just gateway docker
```

From `services/gateway`, use `just build`, `just test`, `just lint`, and
`just docker` directly. Both forms run the same service recipes.

See the [repository quickstart](../../README.md#quickstart) for Compose setup.
The image uses the repository root as its build context:

```sh
docker build --file services/gateway/Dockerfile --tag infergate:local .
```

## CLI and runtime configuration

Argument parsing and configuration live in `src/cli/config.rs`, with `src/cli.rs`
as the module entry point, and use [bpaf](https://docs.rs/bpaf/).
Run `cargo run --package infergate -- --help` or `cargo run --package infergate -- --version` without configuring a backend.
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
Compose reads the root `.env` and passes the settings to the container. For a native run:

```sh
API_ADDRESS=127.0.0.1 API_PORT=8080 INFERENCE_ENDPOINT=http://localhost:8000/v1 \
  cargo run --package infergate --locked -- --inference-timeout-seconds 120
```

## Responses API

Send requests to `POST /v1/responses` with `Content-Type: application/json`.
The gateway reads the body with the Responses API request types. It checks the
request before backend selection. Do not supply both `conversation` and
`previous_response_id`. Set `stream` to `true` when you supply `stream_options`.
The backend checks model support and model-specific limits.

The gateway serializes the typed request as JSON and sends it to the selected
backend's `responses` endpoint. Optional null fields can be omitted during
serialization. It keeps the configured URL prefix, query parameters, and application
headers. It returns the backend's status, application headers, and body.
For `stream: true`, it sends each chunk as it arrives. It does not collect the
complete event stream or change the event format.

The backend must support the
[OpenAI Responses API](https://developers.openai.com/api/reference/python/resources/responses/methods/create).
It owns stored responses, conversation state, background jobs, and tool execution.
The existing `/v1` forwarding route also sends retrieval, deletion, cancellation,
and input-item requests to the backend. The gateway currently selects the first
backend in pool order. It does not distribute requests between backends.

The gateway returns JSON error objects for invalid create requests and backend
connection failures. The configured body limit and timeout also apply to this
route. The gateway forwards backend errors without changes.

Replace `your-model-id` with a model ID supported by your backend:

```sh
curl --no-buffer http://localhost:8080/v1/responses \
  -H 'Content-Type: application/json' \
  -d '{"model":"your-model-id","input":"Say hello.","stream":true}'
```

Supply an `Authorization` header if your backend requires one. The gateway
forwards that header. No new gateway settings are required.
