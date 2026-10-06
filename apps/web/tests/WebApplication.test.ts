import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { ChatEvent, ChatProtocol } from "../src/chat/protocol";
import { RuntimeConfig } from "../src/server/RuntimeConfig";
import { WebApplication } from "../src/server/WebApplication";
import { InferenceFixture } from "./InferenceFixture";

describe("web server and Responses API", () => {
  let inference: InferenceFixture;
  let web: Bun.Server<never>;

  beforeEach(async () => {
    inference = new InferenceFixture();
    const config = RuntimeConfig.parse([], {
      INFERENCE_ENDPOINT: inference.endpoint,
      INFERENCE_MODEL: "test-model",
      INFERENCE_API_KEY: "server-test-secret",
      WEB_HOST: "127.0.0.1",
      WEB_PORT: "0",
    });
    if (config === "help") throw new Error("Expected runtime configuration");
    web = await new WebApplication(config).start();
  });

  afterEach(async () => {
    await web?.stop(true);
    await inference?.stop();
  });

  const send = async (model = "test-model") => {
    const response = await fetch(new URL(ChatProtocol.responses, web.url), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        model,
        input: [{ role: "user", content: "hello" }],
      }),
    });
    return (await response.text())
      .trim()
      .split("\n")
      .map((line) => ChatEvent.parse(JSON.parse(line)));
  };

  test("preserves endpoint prefixes, streams replies, and keeps credentials on the server", async () => {
    const configuration = await fetch(
      new URL(ChatProtocol.configuration, web.url),
    ).then((response) => response.json());
    expect(configuration).toEqual({
      defaultModel: "test-model",
      catalog: { status: "ready", models: ["test-model"] },
    });
    const events = await send();
    expect(events[0]).toEqual({ type: "delta", text: "Hello " });
    expect(events.at(-1)).toEqual({ type: "completed", text: inference.reply });
    expect(inference.requests[0]).toMatchObject({
      model: "test-model",
      stream: true,
      store: false,
    });
    expect(inference.authorization).toEqual([
      "Bearer server-test-secret",
      "Bearer server-test-secret",
    ]);
    expect(JSON.stringify(configuration)).not.toContain("server-test-secret");
  });

  test.each([
    "refusal",
    "incomplete",
    "failed",
    "error",
    "disconnected",
  ] as const)("handles provider lifecycle: %s", async (behavior) => {
    inference.behavior = behavior;
    const events = await send();
    expect(events.at(-1)?.type).toBe(
      behavior === "refusal"
        ? "completed"
        : behavior === "incomplete"
          ? "incomplete"
          : "failed",
    );
    expect(JSON.stringify(events)).not.toContain("secret-provider-error");
  });

  test("discovery failure leaves configured and manual model IDs usable", async () => {
    inference.discovery = "unavailable";
    const response = await fetch(new URL(ChatProtocol.configuration, web.url));
    const configuration = await response.json();
    expect(configuration.defaultModel).toBe("test-model");
    expect(configuration.catalog.status).toBe("unavailable");
    expect((await send()).at(-1)?.type).toBe("completed");
  });

  test("status errors do not expose provider error bodies", async () => {
    const events = await send("broken-model");
    expect(events).toEqual([
      {
        type: "failed",
        message:
          "The inference provider rejected this request. Check the model ID and conversation length.",
      },
    ]);
  });

  test("rejects invalid and cross-origin requests before inference", async () => {
    const invalid = await fetch(new URL(ChatProtocol.responses, web.url), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ model: "", input: [] }),
    });
    expect(invalid.status).toBe(400);
    const crossOrigin = await fetch(new URL(ChatProtocol.responses, web.url), {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Origin: "https://unrelated.test",
      },
      body: JSON.stringify({
        model: "test-model",
        input: [{ role: "user", content: "hello" }],
      }),
    });
    expect(crossOrigin.status).toBe(403);
    expect(inference.requests).toHaveLength(0);
  });

  test("unauthenticated inference sends no placeholder bearer header", async () => {
    await web.stop(true);
    const config = RuntimeConfig.parse([], {
      INFERENCE_ENDPOINT: inference.endpoint,
      WEB_PORT: "0",
      WEB_HOST: "127.0.0.1",
    });
    if (config === "help") throw new Error("Expected runtime configuration");
    web = await new WebApplication(config).start();
    expect((await send()).at(-1)?.type).toBe("completed");
    expect(inference.authorization).toEqual([null]);
  });
});
