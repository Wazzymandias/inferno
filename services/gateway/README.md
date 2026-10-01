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
