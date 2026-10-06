import { z } from "zod";
import { ChatRequest } from "../src/chat/protocol";

const ProviderRequest = z.object({
  ...ChatRequest.shape,
  stream: z.literal(true),
  store: z.literal(false),
});

/** A real HTTP Responses backend for testing framing, lifecycle, and cancellation. */
export class InferenceFixture {
  readonly requests: z.infer<typeof ProviderRequest>[] = [];
  readonly authorization: (string | null)[] = [];
  readonly server: Bun.Server<undefined>;
  behavior:
    | "completed"
    | "refusal"
    | "failed"
    | "error"
    | "incomplete"
    | "disconnected"
    | "slow" = "completed";
  discovery: "available" | "unavailable" = "available";
  cancellations = 0;
  readonly reply = "Hello 🌿\n\n**A streamed reply.**";

  constructor() {
    this.server = Bun.serve({
      hostname: "127.0.0.1",
      port: 0,
      idleTimeout: 0,
      fetch: (request) => this.#handle(request),
    });
  }

  get endpoint(): string {
    return new URL("/tenant/inference", this.server.url).href;
  }

  stop(): Promise<void> {
    return this.server.stop(true);
  }

  async #handle(request: Request): Promise<Response> {
    const path = new URL(request.url).pathname;
    this.authorization.push(request.headers.get("Authorization"));
    if (path === "/tenant/inference/models") {
      return this.discovery === "available"
        ? Response.json({
            object: "list",
            data: [
              {
                id: "test-model",
                object: "model",
                created: 0,
                owned_by: "test",
              },
            ],
          })
        : Response.json(
            { error: { message: "discovery unavailable" } },
            { status: 503 },
          );
    }
    if (path !== "/tenant/inference/responses" || request.method !== "POST") {
      return new Response("Not found", { status: 404 });
    }
    const payload = ProviderRequest.parse(await request.json());
    this.requests.push(payload);
    if (payload.model === "broken-model") {
      return Response.json(
        {
          error: {
            message: "secret-provider-error",
            type: "invalid_request_error",
          },
        },
        { status: 400 },
      );
    }
    const behavior = this.behavior;
    const encoder = new TextEncoder();
    const events = this.#events(behavior);
    return new Response(
      new ReadableStream<Uint8Array>({
        async pull(controller) {
          const next = await events.next();
          if (next.done) controller.close();
          else
            controller.enqueue(
              encoder.encode(
                `event: ${next.value.type}\r\ndata: ${JSON.stringify(next.value)}\r\n\r\n`,
              ),
            );
        },
        cancel: async () => {
          this.cancellations++;
          await events.return(undefined);
        },
      }),
      { headers: { "Content-Type": "text/event-stream" } },
    );
  }

  async *#events(behavior: InferenceFixture["behavior"]) {
    const kind =
      behavior === "refusal"
        ? "response.refusal.delta"
        : "response.output_text.delta";
    yield { type: kind, delta: "Hello " };
    await Bun.sleep(behavior === "slow" ? 500 : 10);
    if (behavior === "disconnected") return;
    if (behavior === "error") {
      yield {
        type: "error",
        error: { code: "upstream_error", message: "secret-provider-error" },
      };
      return;
    }
    if (behavior === "failed") {
      yield {
        type: "response.failed",
        response: { error: { message: "secret-provider-error" } },
      };
      return;
    }
    yield { type: kind, delta: "🌿\n\n**A streamed reply.**" };
    yield {
      type:
        behavior === "incomplete"
          ? "response.incomplete"
          : "response.completed",
      response: {
        output: [
          {
            type: "message",
            role: "assistant",
            id: "message-test",
            status: "completed",
            content: [
              behavior === "refusal"
                ? { type: "refusal", refusal: this.reply }
                : { type: "output_text", text: this.reply, annotations: [] },
            ],
          },
        ],
        incomplete_details:
          behavior === "incomplete" ? { reason: "max_output_tokens" } : null,
      },
    };
  }
}
