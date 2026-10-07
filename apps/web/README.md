# Inference chat

A React chat app that streams replies through the OpenAI Responses API. The
server accepts any compatible API base URL through `--inference-endpoint` or
`INFERENCE_ENDPOINT`. It has no built-in gateway address or model ID.

```bash
bun install --frozen-lockfile
bun run dev --inference-endpoint http://localhost:8080/v1 --model your-model-id
```

Open `http://localhost:3000`. Replace the example URL and model with your service's
API base URL and accepted model ID. The endpoint must include its prefix, such
as `/v1`; the SDK appends `/models` and `/responses`. The model field offers
discovered IDs and accepts custom IDs. Discovery failure does not disable chat.
The root `just deploy` command starts native vLLM, the gateway, and this app.
It configures the same served model ID for inference and the chat default.

Type a message and press Enter to send, or Shift + Enter for another line.
Replies stream as they arrive and render Markdown, including code and tables.
Stop cancels the request and preserves visible partial text. Retry replaces the
latest failed, incomplete, or stopped reply using the selected model, without
duplicating its user message.
New chat cancels any active generation and clears the transcript.

## Runtime configuration

Flags override environment variables. `bun run dev --help` or
`bun run start --help` describes the flags without requiring an endpoint.

| Flag                   | Environment                   | Default                                      |
| ---------------------- | ----------------------------- | -------------------------------------------- |
| `--inference-endpoint` | `INFERENCE_ENDPOINT`          | Required                                     |
| `--model`              | `INFERENCE_MODEL`             | Choose or enter in the UI                    |
| `--temperature`        | `INFERENCE_TEMPERATURE`       | `0`: greedy decoding                         |
| `--max-output-tokens`  | `INFERENCE_MAX_OUTPUT_TOKENS` | `512` generated tokens per reply             |
| `--host`               | `WEB_HOST`                    | `0.0.0.0`                                    |
| `--port`               | `WEB_PORT`                    | `3000`                                       |
| —                      | `INFERENCE_API_KEY`           | No bearer token                              |
| —                      | `NODE_ENV`                    | Development; `bun run start` sets production |

Copy `.env.example` to `.env` inside `apps/web` and fill in `INFERENCE_ENDPOINT`
to use environment configuration. Bun reads this app's `.env` automatically.
The root `.env` configures Compose and the gateway; the two endpoint settings
describe different connections. Compose maps `WEB_INFERENCE_ENDPOINT` into the
web container's `INFERENCE_ENDPOINT` and `WEB_INFERENCE_API_KEY` into its
`INFERENCE_API_KEY`. Its default web endpoint derives the gateway's configured
listener port, and `INFERENCE_MODEL` remains the model's single configuration.

Every chat request explicitly sends its temperature and reply budget to the
provider. Temperature `0` uses greedy decoding: the model chooses the most likely
next token instead of sampling randomly. The temperature must be between `0` and
`2`; increasing it enables random sampling. The reply budget must be a positive
integer and counts generated tokens, including any reasoning tokens, rather than
characters. Increase it when longer replies are needed. The web app owns these
defaults; Compose passes overrides from the root `.env` without redefining them.
Apply changed Compose settings with `docker compose up -d --no-deps web`.
These controls stabilize and bound generation; they do not verify factual claims.

Endpoint URLs must use HTTP(S) without URL credentials, query parameters, or
fragments. `INFERENCE_API_KEY` is optional and stays on the server. Browser
requests use the site's own `/api/chat` routes, avoiding cross-origin inference
requests and browser access to service credentials.

## Build and run

```bash
bun run check
bun run format:check
bun run test
bun run build
bun run start --inference-endpoint http://localhost:8080/v1 --model your-model-id
```

The build produces browser assets in `dist/` without reading deployment
configuration. Production serves those assets; it does not rebuild them at
startup. Development and production use the same HTTP API and inference client.
`GET /healthz` reports that the web process is running. Startup, shutdown, and
server failures write events to stdout without request bodies, service URLs, or
credentials. Model discovery has a ten-second request timeout because it is
optional; generation uses the SDK's timeout and the inference service's limits.

From the repository root, run the complete web check:

```bash
just web test
```

This installs locked dependencies and Chromium, checks TypeScript, runs the unit
tests, builds production assets, and runs Playwright. Each UI test owns its HTTP
fixture and web server. A separate browser test uses the configured native model
through the deployed web app and gateway: it checks a correct completed answer,
conversation context, and a longer complete reply. Failure, truncation, and
browser errors fail the test; model responses are not mocked in this check.

Playwright's [web server lifecycle](https://playwright.dev/docs/test-webserver)
derives the web address from Compose and uses the root `.env` model
configuration. It reuses an already running local deployment. Otherwise it owns
`just deploy up --detached=false` and shuts down that session after the tests,
retaining containers and volumes. CI requires a fresh deployment. Native startup
has a fifteen-minute budget for builds and model loading; each expected answer
has a sixty-second wait within a three-minute conversation test.

The complete check needs the native deployment prerequisites listed in the root
README. `bun run test` runs the application tests without native inference.
`bun run test:browser` runs the browser suite. Failed browser tests retain traces,
screenshots, and an HTML report in `test-results/` and `playwright-report/`;
open the report with `bun --bun playwright show-report`.

From the repository root, `just deploy` starts native inference and builds and
runs the `web` and gateway Compose services. Compose owns container names.
Its health check verifies the configured web listener's `/healthz` endpoint.
The container uses the locked dependencies and
the declared `BUN_IMAGE`, runs as the Bun user, and accepts the same environment
configuration as native runs. Compose's `WEB_HOST_PORT` sets the published port;
`WEB_HOST_ADDR` controls its host binding; `WEB_PORT` sets the container's listener.

## Ownership

- `src/server/RuntimeConfig.ts` owns runtime settings and CLI help.
- `src/server/InferenceService.ts` owns SDK authentication, model discovery,
  Responses events, refusals, final text, and provider error handling.
- `src/server/WebApplication.ts` owns HTTP validation, same-origin enforcement,
  stream cancellation, and asset serving.
- `src/chat/protocol.ts` owns the validated browser/server contract.
- `src/chat/ChatClient.ts` owns incremental browser stream decoding.
- `src/chat/Conversation.ts` owns history and reply transitions. Every request
  includes previous completed exchanges with `store: false`; failed or stopped
  exchanges stay visible but are excluded from future context. No response IDs,
  sticky server sessions, provider storage, or gateway-specific state are needed.
- `src/App.tsx` and `src/chat/Message.tsx` own the chat UI and Markdown rendering.

History lives in the current tab's memory and clears on reload. The web process
keeps no conversation state and can scale independently. Inference requests
still send the conversation to your configured service, whose own data policy
applies. Streaming follows the [official Responses API documentation](https://developers.openai.com/api/docs/guides/streaming-responses).
