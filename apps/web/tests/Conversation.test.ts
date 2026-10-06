import { describe, expect, test } from "bun:test";
import type { ChatTransport } from "../src/chat/ChatClient";
import { Conversation } from "../src/chat/Conversation";
import type { ChatEvent, ChatRequest } from "../src/chat/protocol";

class ScriptedChat implements ChatTransport {
  readonly requests: ChatRequest[] = [];
  script: ChatEvent[] = [
    { type: "delta", text: "partial" },
    { type: "completed", text: "authoritative reply" },
  ];

  async *respond(request: ChatRequest): AsyncGenerator<ChatEvent> {
    this.requests.push(request);
    for (const event of this.script) yield event;
  }
}

describe("conversation lifecycle", () => {
  test("follow-ups send complete user/assistant history and reconcile final text", async () => {
    const transport = new ScriptedChat();
    const chat = new Conversation(transport);
    await chat.send(" first ", " test-model ");
    await chat.send("follow-up", "test-model");
    expect(transport.requests[1]?.input).toEqual([
      { role: "user", content: "first" },
      { role: "assistant", content: "authoritative reply" },
      { role: "user", content: "follow-up" },
    ]);
    expect(chat.getSnapshot().status).toBe("idle");
    expect(chat.getSnapshot().turns[0]?.reply).toEqual({
      status: "completed",
      text: "authoritative reply",
    });
  });

  test("retry replaces a failed reply without duplicating the prompt", async () => {
    const transport = new ScriptedChat();
    const chat = new Conversation(transport);
    transport.script = [
      { type: "delta", text: "partial" },
      { type: "failed", message: "Unavailable" },
    ];
    await chat.send("try this", "test-model");
    expect(chat.getSnapshot().turns[0]?.reply).toEqual({
      status: "failed",
      text: "partial",
      message: "Unavailable",
    });
    transport.script = [{ type: "completed", text: "success" }];
    await chat.retry("replacement-model");
    expect(transport.requests[1]?.input).toEqual([
      { role: "user", content: "try this" },
    ]);
    expect(transport.requests[1]?.model).toBe("replacement-model");
    expect(chat.getSnapshot().turns).toHaveLength(1);
    expect(chat.getSnapshot().turns[0]?.reply).toEqual({
      status: "completed",
      text: "success",
    });
  });

  test("a truncated stream is a visible failure and cannot become completed context", async () => {
    const transport = new ScriptedChat();
    const chat = new Conversation(transport);
    transport.script = [{ type: "delta", text: "unfinished" }];
    await chat.send("first", "test-model");
    expect(chat.getSnapshot().turns[0]?.reply.status).toBe("failed");
    transport.script = [{ type: "completed", text: "next" }];
    await chat.send("new prompt", "test-model");
    expect(transport.requests[1]?.input).toEqual([
      { role: "user", content: "new prompt" },
    ]);
  });

  test("stop and clear isolate late events from the next conversation", async () => {
    let release: () => void = () => {
      throw new Error("Request has not started");
    };
    let aborted = false;
    const transport: ChatTransport = {
      async *respond(_request, signal) {
        yield { type: "delta", text: "partial" };
        await new Promise<void>((resolve) => {
          release = resolve;
        });
        aborted = signal.aborted;
        yield { type: "completed", text: "late reply" };
      },
    };
    const chat = new Conversation(transport);
    const pending = chat.send("first", "test-model");
    await Bun.sleep(0);
    await chat.send("duplicate", "test-model");
    expect(chat.getSnapshot().turns).toHaveLength(1);
    chat.stop();
    expect(chat.getSnapshot().turns[0]?.reply).toEqual({
      status: "stopped",
      text: "partial",
    });
    chat.clear();
    release();
    await pending;
    expect(aborted).toBe(true);
    expect(chat.getSnapshot()).toEqual({ status: "idle", turns: [] });
  });
});
