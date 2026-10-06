import type { ChatTransport } from "./ChatClient";
import type { ChatEvent, ChatRequest } from "./protocol";

export type Reply =
  | {
      readonly status: "streaming" | "completed" | "stopped";
      readonly text: string;
    }
  | {
      readonly status: "failed" | "incomplete";
      readonly text: string;
      readonly message: string;
    };

export interface ChatTurn {
  readonly id: string;
  readonly model: string;
  readonly prompt: string;
  readonly reply: Reply;
}

export type ConversationState =
  | { readonly status: "idle"; readonly turns: readonly ChatTurn[] }
  | {
      readonly status: "generating";
      readonly turns: readonly ChatTurn[];
      readonly turnId: string;
    };

/** Owns turn history, cancellation, retries, and which replies are valid context. */
export class Conversation {
  #state: ConversationState = { status: "idle", turns: [] };
  #active: {
    readonly id: string;
    readonly controller: AbortController;
  } | null = null;
  readonly #listeners = new Set<() => void>();

  constructor(private readonly transport: ChatTransport) {}

  readonly getSnapshot = (): ConversationState => this.#state;

  readonly subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => {
      this.#listeners.delete(listener);
    };
  };

  async send(prompt: string, model: string): Promise<void> {
    const text = prompt.trim();
    const modelId = model.trim();
    if (!text || !modelId || this.#active) return;
    const turn: ChatTurn = {
      id: crypto.randomUUID(),
      model: modelId,
      prompt: text,
      reply: { status: "streaming", text: "" },
    };
    await this.#generate(turn, [...this.#state.turns, turn]);
  }

  async retry(model: string): Promise<void> {
    const turn = this.#state.turns.at(-1);
    const modelId = model.trim();
    if (!turn || !modelId || this.#active || turn.reply.status === "completed")
      return;
    const replacement: ChatTurn = {
      ...turn,
      model: modelId,
      reply: { status: "streaming", text: "" },
    };
    await this.#generate(replacement, [
      ...this.#state.turns.slice(0, -1),
      replacement,
    ]);
  }

  stop(): void {
    const active = this.#active;
    if (!active) return;
    this.#active = null;
    active.controller.abort();
    this.#replaceReply(active.id, (reply) => ({
      status: "stopped",
      text: reply.text,
    }));
  }

  clear(): void {
    this.stop();
    this.#publish({ status: "idle", turns: [] });
  }

  async #generate(turn: ChatTurn, turns: readonly ChatTurn[]): Promise<void> {
    const active = { id: turn.id, controller: new AbortController() };
    this.#active = active;
    this.#publish({ status: "generating", turns, turnId: turn.id });
    const input: ChatRequest["input"] = [];
    for (const previous of turns) {
      // Failed/stopped exchanges are visible but do not become invented model context.
      if (previous.id !== turn.id && previous.reply.status !== "completed")
        continue;
      input.push({ role: "user", content: previous.prompt });
      if (previous.reply.status === "completed") {
        input.push({ role: "assistant", content: previous.reply.text });
      }
    }
    try {
      for await (const event of this.transport.respond(
        { model: turn.model, input },
        active.controller.signal,
      )) {
        // An aborted request may still resolve after a new conversation has started.
        if (this.#active !== active) return;
        this.#apply(turn.id, event);
        if (event.type !== "delta") return;
      }
      if (this.#active === active) {
        this.#replaceReply(turn.id, (reply) => ({
          status: "failed",
          text: reply.text,
          message:
            "The connection closed before the reply was complete. You can retry.",
        }));
      }
    } catch (error) {
      if (this.#active !== active) return;
      this.#replaceReply(turn.id, (reply) => ({
        status: "failed",
        text: reply.text,
        message:
          error instanceof Error
            ? error.message
            : "The reply could not be loaded. You can retry.",
      }));
    } finally {
      if (this.#active === active) {
        this.#active = null;
        this.#publish({ status: "idle", turns: this.#state.turns });
      }
    }
  }

  #apply(id: string, event: ChatEvent): void {
    this.#replaceReply(id, (reply) => {
      switch (event.type) {
        case "delta":
          return { status: "streaming", text: reply.text + event.text };
        case "completed":
          return { status: "completed", text: event.text };
        case "incomplete":
          return {
            status: "incomplete",
            text: event.text || reply.text,
            message: event.message,
          };
        case "failed":
          return { status: "failed", text: reply.text, message: event.message };
      }
    });
  }

  #replaceReply(id: string, transition: (reply: Reply) => Reply): void {
    const turns = this.#state.turns.map((turn) =>
      turn.id === id ? { ...turn, reply: transition(turn.reply) } : turn,
    );
    this.#publish(
      this.#active
        ? { status: "generating", turns, turnId: this.#active.id }
        : { status: "idle", turns },
    );
  }

  #publish(state: ConversationState): void {
    this.#state = state;
    for (const listener of this.#listeners) listener();
  }
}
