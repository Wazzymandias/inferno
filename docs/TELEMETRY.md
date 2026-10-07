# Telemetry

For startup instructions, see [Enable SigNoz](../README.md#enable-signoz).
Run the commands in this guide from the repository root.

## Services and profiles

**What starts:** SigNoz, its OTel collector, ClickHouse, ClickHouse Keeper,
PostgreSQL, and both setup jobs join the app's network.

- The root Compose file statically includes [telemetry/compose.yaml](../telemetry/compose.yaml).
- Telemetry is disabled by default: `.env.example` sets `COMPOSE_PROFILES=`.
- An empty or unset profile list enables only the web and API in Compose.
- Application instrumentation and cloud collection agents need separate configuration.

## Credentials and storage

- **Database password:** a one-off initialization container generates a random
  password and stores it in a Docker volume. PostgreSQL and SigNoz read it
  through read-only mounts.
- **Schema setup:** a one-off migration container initializes or upgrades the
  schema before SigNoz and the collector start.
- **Persistence:** named volumes retain credentials, telemetry, and account data
  across container restarts.

| Setting | Purpose |
| --- | --- |
| `SIGNOZ_POSTGRES_PASSWORD` | Optional URL-safe password for first startup. Keep the existing value when upgrading an initialized deployment. |
| `SIGNOZ_POSTGRES_DSN` | Override SigNoz's database URI. |
| `SIGNOZ_CLICKHOUSE_DSN` | ClickHouse connection used by SigNoz, the collector, and the migration job. |

## Ports and export endpoints

| Setting | Purpose |
| --- | --- |
| `SIGNOZ_HOST` | Host binding; defaults to `127.0.0.1`. |
| `SIGNOZ_UI_PORT` | Published UI port. |
| `SIGNOZ_OTLP_GRPC_PORT` | Published OTLP/gRPC ingestion port. |
| `SIGNOZ_OTLP_HTTP_PORT` | Published OTLP/HTTP ingestion port. |

ClickHouse, Keeper, and PostgreSQL have no published host ports.

| Exporter location | OTLP/gRPC | OTLP/HTTP |
| --- | --- | --- |
| Compose network | `http://signoz-otel-collector:4317` | `http://signoz-otel-collector:4318` |
| Host process | `localhost` + `SIGNOZ_OTLP_GRPC_PORT` | `localhost` + `SIGNOZ_OTLP_HTTP_PORT` |

Configure the SDK's OTLP protocol to match the endpoint.

## Pinned images and build dependencies

The following [.env.example](../.env.example) settings lock upstream images by digest:

- `SIGNOZ_IMAGE`
- `SIGNOZ_COLLECTOR_IMAGE`
- `SIGNOZ_CLICKHOUSE_IMAGE`
- `SIGNOZ_KEEPER_IMAGE`
- `SIGNOZ_POSTGRES_IMAGE`

The configurations are adapted from
[Foundry v0.3.0](https://github.com/SigNoz/foundry/tree/v0.3.0/docs/examples/docker/compose)
and run directly with Compose. Foundry is not a runtime dependency.

The ClickHouse build installs SigNoz's histogram function **v0.0.1**, with
architecture-specific SHA-256 verification. Container startup does not download
executables.

## Inspect, stop, or disable telemetry

With the `metrics` profile enabled:

```sh
docker compose ps -a
docker compose logs signoz signoz-otel-collector signoz-migrate
docker compose down
```

- `down` retains named volumes.
- **Adding `--volumes` deletes credentials, stored telemetry, and SigNoz accounts.**
- To disable a running telemetry stack, stop it while `metrics` is still enabled,
  then remove `metrics` from `COMPOSE_PROFILES`.
- Changing the profile list alone does not stop existing containers.

For a telemetry-only deployment:

```sh
docker compose -f telemetry/compose.yaml --profile metrics up -d --wait
```
