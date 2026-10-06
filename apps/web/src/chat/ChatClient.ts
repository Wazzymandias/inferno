import { z } from "zod";
import {
  ChatConfiguration,
  ChatEvent,
  ChatProtocol,
  type ChatRequest,
} from "./protocol";

export interface ChatTransport {
  respond(request: ChatRequest, signal: AbortSignal): AsyncIterable<ChatEvent>;
}

/** Owns the same-origin HTTP contract and incremental UTF-8/NDJSON decoding. */
export class ChatClient implements ChatTransport {
  constructor(
    private readonly request: (
      path: string,
      options?: RequestInit,
    ) => Promise<Response> = fetch.bind(globalThis),
  ) {}

  async configuration(signal: AbortSignal): Promise<ChatConfiguration> {
    const response = await this.request(ChatProtocol.configuration, { signal });
    if (!response.ok)
      throw new Error("The chat server is unavailable. Reload to reconnect.");
    const result = ChatConfiguration.safeParse(await response.json());
    if (!result.success)
      throw new Error("The chat server returned invalid configuration.");
    return result.data;
  }

  async *respond(
    request: ChatRequest,
    signal: AbortSignal,
  ): AsyncGenerator<ChatEvent> {
    const response = await this.request(ChatProtocol.responses, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(request),
      signal,
    });
    if (!response.ok) {
      const error = z
        .object({ error: z.string() })
        .safeParse(await response.json().catch(() => null));
      throw new Error(
        error.success
          ? error.data.error
          : `The chat server rejected the request (${response.status}).`,
      );
    }
    if (
      !response.body ||
      !response.headers
        .get("Content-Type")
        ?.startsWith(ChatProtocol.streamContentType)
    ) {
      throw new Error("The chat server did not return a chat stream.");
    }

    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let pending = "";
    try {
      while (true) {
        const chunk = await reader.read();
        pending += chunk.done
          ? decoder.decode()
          : decoder.decode(chunk.value, { stream: true });
        let newline: number;
        while ((newline = pending.indexOf("\n")) !== -1) {
          const line = pending.slice(0, newline).trim();
          pending = pending.slice(newline + 1);
          if (line) yield this.#decodeEvent(line);
        }
        if (chunk.done) {
          if (pending.trim()) yield this.#decodeEvent(pending);
          return;
        }
      }
    } finally {
      await reader.cancel().catch(() => undefined);
      reader.releaseLock();
    }
  }

  #decodeEvent(line: string): ChatEvent {
    try {
      return ChatEvent.parse(JSON.parse(line));
    } catch {
      throw new Error("The chat server sent an invalid stream. You can retry.");
    }
  }
}
