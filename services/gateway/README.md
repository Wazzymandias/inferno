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

Serve and render commands live in `src/cli/serve.rs` and `src/cli/render.rs`,
with `src/cli.rs` as the command parser. They use [bpaf](https://docs.rs/bpaf/).
Run `cargo run --package infergate -- --help` or `cargo run --package infergate -- --version` without configuring a backend.
Use `cargo run --package infergate -- serve --help` for server options. A subcommand
is required: `serve` starts the gateway, and `render` prints locally prepared token IDs and prefix hashes.
Flags override environment variables; omitted values use the defaults below.
Invalid configuration fails before the listener opens.

| Flag | Environment variable | Default |
| --- | --- | --- |
| `--api-address` | `API_ADDRESS` | Required |
| `--api-port` | `API_PORT` | Required |
| `--model` | `INFERENCE_MODEL` | Required; both commands |
| Environment only | `INFERENCE_API_KEY` | Empty; optional backend bearer token |
| Environment only | `XDG_CACHE_HOME` | `$HOME/.cache`; Compose supplies `/var/cache` |
| `--inference-endpoint` | `INFERENCE_ENDPOINT` | Required; both commands |
| `--inference-timeout-seconds` | `INFERENCE_TIMEOUT_SECONDS` | `300` |
| `--max-request-bytes` | `MAX_REQUEST_BYTES` | `1048576` |
| `--shutdown-timeout-seconds` | `SHUTDOWN_TIMEOUT_SECONDS` | `5` |

Timeouts and body limits must be positive. The backend URL must use HTTP(S),
without a query or fragment. Port `0` selects an available port for native runs.
The binary reads the process environment; it does not load `.env` automatically.
Compose reads the root `.env` and passes the settings to the container. For a native run:

```sh
INFERENCE_MODEL=your-model-id \
API_ADDRESS=127.0.0.1 API_PORT=8080 INFERENCE_ENDPOINT=http://localhost:8000/v1 \
  cargo run --package infergate --locked -- serve --inference-timeout-seconds 120
```

## Responses API

Send requests to `POST /v1/responses` with `Content-Type: application/json`.
The gateway reads the body with the Responses API request types. It checks the
request before backend selection. Do not supply both `conversation` and
`previous_response_id`. Set `stream` to `true` when you supply `stream_options`.
Preparation checks the served model name and available context before forwarding.

The gateway serializes the typed request as JSON and sends it to the selected
backend's `responses` endpoint. Optional null fields can be omitted during
serialization. It keeps the configured URL prefix, query parameters, and application
headers. It returns the backend's status, application headers, and body.
For `stream: true`, it sends each chunk as it arrives. It does not collect the
complete event stream or change the event format.

The configured native vLLM backend must support the
[Responses API](https://developers.openai.com/api/reference/python/resources/responses/methods/create).
It owns response storage, background jobs, and tool execution. Create requests
must include their history inline so local preparation can reproduce the input.
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

For an authenticated backend, set `INFERENCE_API_KEY` for startup discovery.
It also supplies the default backend credential when forwarding. An incoming
`Authorization` header takes precedence. Credentials are never written to the
model cache or runtime logs.

## Local input preparation

Select the served model and backend endpoint. Startup discovers their resolved
configuration and loads `InputProcessor` once. Each `prepare()` call
validates a borrowed request, renders and tokenizes locally, computes prefix
hashes, and returns a completed `ModelInput`:

```rust
let input = processor.prepare(&request)?;
let tokens = input.tokens();
let hashes = input.prefix_hashes();
```

`InputProcessor` owns the reusable encoder and hash policy. `ModelInput` owns only
its tokens and hashes; accessors perform no work and borrow neither processor nor
request. The handler retains the original typed request for forwarding. CPU work
runs on Tokio's blocking pool. `RenderInput` is private to `HuggingFaceEncoder`.
Generic proxy and health routes do not manufacture model input.

`InputError` covers request validation, preparation, and backend dispatch. Causes
remain typed and available through `Error::source`; the HTTP boundary maps each
variant to a response without exposing source diagnostics in runtime logs.

The gateway performs no render RPC. Inspect the same preparation with:

```sh
cargo run --locked --package infergate -- render \
  --model your-model-id --inference-endpoint http://localhost:8002/v1 < request.json
```

Output is `{"token_ids":[...],"prefix_hashes":["<64 hex digits>",...]}`. The server
never logs that data. Preparation errors identify fields without including input
content. Prefix hashes cover complete blocks only; a partial tail has no hash.
Each request starts a fresh chain. The native `cache_salt` request extension salts
the first block and therefore all descendants. `prompt_cache_key` is a different
API field and is not substituted for native cache salt.

### Automatic model configuration

`just deploy` installs the native integration and passes the same `INFERENCE_MODEL`
to the gateway. Before opening its listener, the gateway fetches
`GET /v1/infergate/model-config?model=...` from `INFERENCE_ENDPOINT`. That startup
request transfers the loaded tokenizer, resolved templates, and deployment policy.
It carries no Responses request and performs no rendering. Per-request preparation
uses only the processor's in-memory assets.

The gateway caches the configuration automatically under
`$XDG_CACHE_HOME/infergate/models` (default `$HOME/.cache/infergate/models`), keyed
by backend endpoint and model. Compose supplies a writable, disposable named
volume. There are no asset-directory settings or shared filesystem mounts.
`serve` always refreshes from the active backend and fails if discovery fails;
it never starts with stale policy. `render` uses the last discovered copy offline,
fetching it automatically on first use. Restart the gateway when the native
model/configuration changes; its successful startup also refreshes offline inspection.

The integration in `tools/deploy/model_config.py` reads each value from its owner:

- The loaded native tokenizer supplies its vocabulary, added tokens, and
  postprocessor; the renderer supplies templates, detected content formats,
  special tokens, and template defaults.
- The native configuration supplies served names, effective context length, and
  Responses tokenization policy.
- The scheduler supplies its resolved hash block size, including hybrid cache
  layouts. Native vLLM derives the initial parent, including `PYTHONHASHSEED`.

A temporary file scoped to the native process instance transfers the scheduler's
policy to the API process and is consumed before readiness. The completed snapshot
is served from memory through normal native authentication. The integration
currently supports one native data process and one API process.

These deployment values cannot be inferred from a static model ID alone. There
are no independent gateway knobs for block size, hash seed, template selection,
or content format. The launcher selects `sha256_cbor` for matching Python/Rust
canonical encoding.

For a separately launched vLLM 0.31.0 server, put this repository on `PYTHONPATH`
and include the same integration:

```sh
--prefix-caching-hash-algo sha256_cbor \
--scheduler-cls tools.deploy.model_config.ExportingScheduler \
--middleware tools.deploy.model_config.ModelConfigMiddleware
```

Then configure only its endpoint, served model, and credentials if required. The
integration follows the locked native version; backend upgrades require checking
parity against that version. Plain servers without this integration cannot supply
the required configuration.

### Supported inputs and verification

The local implementation supports inline text conversations, system/developer
instructions, function definitions/calls/results, and reasoning text/effort.
It follows the native string or structured text content format and developer-role
normalization. It rejects unresolved stored history/prompts/item references,
media, prompt embeddings, adapters, automatic truncation, partial assistant
continuations, forced tool selection, unsupported tool execution, and time-dependent templates. These
cases return errors; there is no approximate-token or RPC fallback. Adapter, encoder-decoder, and
prompt-embedding configurations fail during native startup.

Selection still uses the first backend. Preparing hashes does not yet implement
cache-aware routing or cache-state discovery.

The Rust suite checks committed token/hash fixtures generated by the locked native
renderer, including both content formats, special tokens, Unicode, tools,
reasoning, salts, and repeated/concurrent use. Regenerate them with:

```sh
uv run --locked --project tools/deploy python tools/parity/fixtures.py
```

The generator captures the gateway's actual forwarded JSON before native rendering
because schema property order can change template tokens. To compare against a
running native server's render endpoint (verification only):

```sh
just gateway parity --inference-endpoint http://localhost:8002/v1 \
  --model your-model-id
```

To check a cached checkpoint without loading inference weights or starting vLLM:

```sh
uv run --locked --project tools/deploy python tools/parity/verify_local.py \
  --model-path /path/to/snapshot --model your-model-id
```

This checks native rendering and hashing against the local implementation using
a small fixture block size. The startup-discovery tests separately verify the
resolved scheduler block size, including hybrid layouts.
