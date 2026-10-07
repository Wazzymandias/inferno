import { describe, expect, test } from "bun:test";
import { ChatClient } from "../src/chat/ChatClient";
import {
  ChatProtocol,
  type ChatEvent,
  type ChatRequest,
} from "../src/chat/protocol";

const request: ChatRequest = {
  model: "test-model",
  input: [{ role: "user", content: "Hello" }],
};

describe("browser stream transport", () => {
  test("decodes split UTF-8 and event boundaries, including a final line without newline", async () => {
    const expected: ChatEvent[] = [
      { type: "delta", text: "🌿" },
      { type: "completed", text: "Hello 🌿" },
    ];
    const bytes = new TextEncoder().encode(
      expected.map((event) => JSON.stringify(event)).join("\n"),
    );
    const client = new ChatClient(async (_path, options) => {
      expect(JSON.parse(String(options?.body))).toEqual(request);
      return new Response(
        new ReadableStream<Uint8Array>({
          start(controller) {
            for (const byte of bytes)
              controller.enqueue(new Uint8Array([byte]));
            controller.close();
          },
        }),
        { headers: { "Content-Type": ChatProtocol.streamContentType } },
      );
    });
    const received: ChatEvent[] = [];
    for await (const event of client.respond(
      request,
      new AbortController().signal,
    ))
      received.push(event);
    expect(received).toEqual(expected);
  });

  test("rejects malformed events without exposing response contents", async () => {
    const client = new ChatClient(
      async () =>
        new Response('{"type":"unexpected","secret":"secret-value"}\n', {
          headers: { "Content-Type": ChatProtocol.streamContentType },
        }),
    );
    const stream = client.respond(request, new AbortController().signal);
    const result = stream.next();
    await expect(result).rejects.toThrow();
    await expect(result).rejects.not.toThrow("secret-value");
  });

  test("closing the consumer cancels its HTTP response reader", async () => {
    let cancelled = false;
    const client = new ChatClient(
      async () =>
        new Response(
          new ReadableStream<Uint8Array>({
            start(controller) {
              controller.enqueue(
                new TextEncoder().encode('{"type":"delta","text":"partial"}\n'),
              );
            },
            cancel() {
              cancelled = true;
            },
          }),
          { headers: { "Content-Type": ChatProtocol.streamContentType } },
        ),
    );
    for await (const event of client.respond(
      request,
      new AbortController().signal,
    )) {
      expect(event.type).toBe("delta");
      break;
    }
    expect(cancelled).toBe(true);
  });
});
