import { ChatProtocol, ChatRequest } from "../chat/protocol";
import { InferenceService } from "./InferenceService";
import type { RuntimeConfig } from "./RuntimeConfig";

/** Stateless HTTP application: configuration, validation, streaming, and asset serving. */
export class WebApplication {
  readonly #inference: InferenceService;

  constructor(readonly config: RuntimeConfig) {
    this.#inference = new InferenceService(config);
  }

  async start() {
    const routes: Bun.Serve.Routes<never, string> = {
      [ChatProtocol.configuration]: {
        GET: async (request) =>
          Response.json(
            {
              defaultModel: this.config.model,
              catalog: await this.#inference.catalog(request.signal),
            },
            { headers: { "Cache-Control": "no-store" } },
          ),
      },
      [ChatProtocol.responses]: { POST: (request) => this.#respond(request) },
      "/healthz": new Response("ok"),
    };

    if (this.config.mode === "development") {
      routes["/"] = (await import("../index.html")).default;
    } else {
      const directory = new URL("../../dist/", import.meta.url);
      const index = Bun.file(new URL("index.html", directory));
      if (!(await index.exists())) {
        throw new Error(
          "Built web assets are missing. Run bun run build before bun run start.",
        );
      }
      for await (const path of new Bun.Glob("**/*").scan({
        cwd: directory.pathname,
        onlyFiles: true,
      })) {
        const asset = Bun.file(new URL(path, directory));
        routes[`/${path}`] = () =>
          new Response(asset, {
            headers: {
              "Cache-Control":
                path === "index.html"
                  ? "no-cache"
                  : "public, max-age=31536000, immutable",
            },
          });
      }
      routes["/"] = () =>
        new Response(index, { headers: { "Cache-Control": "no-cache" } });
    }

    return Bun.serve({
      hostname: this.config.host,
      port: this.config.port,
      // Streaming generations may pause while a backend loads a model.
      idleTimeout: 0,
      routes,
      development:
        this.config.mode === "development"
          ? { hmr: true, console: false }
          : false,
      fetch: () => new Response("Not found", { status: 404 }),
      error: () => {
        console.log(JSON.stringify({ event: "web.request.failed" }));
        return Response.json(
          { error: "The chat server could not handle this request." },
          { status: 500 },
        );
      },
    });
  }

  async #respond(request: Request): Promise<Response> {
    const origin = request.headers.get("Origin");
    if (origin && origin !== new URL(request.url).origin) {
      return Response.json(
        { error: "Send chat requests from this site's origin." },
        { status: 403 },
      );
    }
    if (
      request.headers.get("Content-Type")?.split(";")[0]?.trim() !==
      "application/json"
    ) {
      return Response.json(
        { error: "Chat requests must use application/json." },
        { status: 415 },
      );
    }
    let body: unknown;
    try {
      body = await request.json();
    } catch {
      return Response.json(
        { error: "The chat request is not valid JSON." },
        { status: 400 },
      );
    }
    const result = ChatRequest.safeParse(body);
    if (!result.success) {
      return Response.json(
        {
          error:
            "Supply a model ID and a conversation ending with a user message.",
        },
        { status: 400 },
      );
    }

    const abort = new AbortController();
    const signal = AbortSignal.any([request.signal, abort.signal]);
    const events = this.#inference.respond(result.data, signal);
    const encoder = new TextEncoder();
    const stream = new ReadableStream<Uint8Array>({
      async pull(controller) {
        try {
          const next = await events.next();
          if (next.done) controller.close();
          else
            controller.enqueue(
              encoder.encode(`${JSON.stringify(next.value)}\n`),
            );
        } catch {
          controller.error(new Error("The chat stream was interrupted."));
        }
      },
      async cancel() {
        abort.abort();
        await events.return(undefined);
      },
    });
    return new Response(stream, {
      headers: {
        "Content-Type": ChatProtocol.streamContentType,
        "Cache-Control": "no-store",
        "X-Accel-Buffering": "no",
      },
    });
  }
}
