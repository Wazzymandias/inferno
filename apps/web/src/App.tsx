import {
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type FormEvent,
  type KeyboardEvent,
} from "react";
import { ChatClient } from "./chat/ChatClient";
import { Conversation } from "./chat/Conversation";
import type { ChatConfiguration } from "./chat/protocol";
import { Message } from "./chat/Message";

type Setup =
  | { readonly status: "loading" }
  | { readonly status: "ready"; readonly configuration: ChatConfiguration }
  | { readonly status: "failed"; readonly message: string };

// Keep following a reply when the reader is within the bottom of its transcript.
const FOLLOW_REPLY_DISTANCE_PX = 80;

export function App() {
  const [client] = useState(() => new ChatClient());
  const [conversation] = useState(() => new Conversation(client));
  const state = useSyncExternalStore(
    conversation.subscribe,
    conversation.getSnapshot,
  );
  const [setup, setSetup] = useState<Setup>({ status: "loading" });
  const [model, setModel] = useState("");
  const [draft, setDraft] = useState("");
  const composer = useRef<HTMLTextAreaElement>(null);
  const transcript = useRef<HTMLDivElement>(null);
  const followReplies = useRef(true);
  const generating = state.status === "generating";
  const canSend =
    setup.status === "ready" && !!model.trim() && !!draft.trim() && !generating;

  useEffect(() => {
    const abort = new AbortController();
    void client
      .configuration(abort.signal)
      .then((configuration) => {
        setSetup({ status: "ready", configuration });
        setModel(configuration.defaultModel);
      })
      .catch((error: unknown) => {
        if (!abort.signal.aborted) {
          setSetup({
            status: "failed",
            message:
              error instanceof Error
                ? error.message
                : "The chat server is unavailable.",
          });
        }
      });
    return () => {
      abort.abort();
      conversation.stop();
    };
  }, [client, conversation]);

  useEffect(() => {
    if (followReplies.current && transcript.current) {
      transcript.current.scrollTop = transcript.current.scrollHeight;
    }
  }, [state]);

  useEffect(() => {
    const element = composer.current;
    if (!element) return;
    element.style.height = "auto";
    element.style.height = `${element.scrollHeight}px`;
  }, [draft]);

  const send = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!canSend) return;
    followReplies.current = true;
    void conversation.send(draft, model);
    setDraft("");
    composer.current?.focus();
  };

  const composeKey = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (
      event.key === "Enter" &&
      !event.shiftKey &&
      !event.nativeEvent.isComposing
    ) {
      event.preventDefault();
      event.currentTarget.form?.requestSubmit();
    }
  };

  const catalog = setup.status === "ready" ? setup.configuration.catalog : null;
  const latestTurn = state.turns.at(-1);
  const retryable =
    !generating && latestTurn && latestTurn.reply.status !== "completed";

  return (
    <div className="chat-app">
      <header className="app-header">
        <a className="wordmark" href="/" aria-label="Chat home">
          <span className="brand-mark" aria-hidden="true">
            ✳
          </span>{" "}
          Chat
        </a>
        <div className="header-controls">
          <label className="model-control">
            <span>Model</span>
            <input
              aria-label="Model"
              list="available-models"
              value={model}
              onChange={(event) => setModel(event.target.value)}
              placeholder={
                setup.status === "loading"
                  ? "Loading models…"
                  : "Choose or enter a model ID"
              }
              disabled={generating || setup.status !== "ready"}
              spellCheck={false}
            />
            <datalist id="available-models">
              {catalog?.status === "ready" &&
                catalog.models.map((id) => <option key={id} value={id} />)}
            </datalist>
          </label>
          <button
            className="new-chat"
            type="button"
            disabled={!state.turns.length}
            onClick={() => {
              conversation.clear();
              setDraft("");
              followReplies.current = true;
              composer.current?.focus();
            }}
          >
            <span aria-hidden="true">＋</span> New chat
          </button>
        </div>
      </header>

      <main className="chat-main">
        {setup.status === "failed" && (
          <div className="connection-notice" role="alert">
            {setup.message} <a href="/">Reconnect</a>
          </div>
        )}
        {catalog?.status === "unavailable" && (
          <div className="connection-notice" role="status">
            {catalog.message}
          </div>
        )}
        <div
          className="transcript"
          ref={transcript}
          role="log"
          aria-label="Conversation"
          aria-live="off"
          onScroll={(event) => {
            const element = event.currentTarget;
            followReplies.current =
              element.scrollHeight - element.scrollTop - element.clientHeight <
              FOLLOW_REPLY_DISTANCE_PX;
          }}
        >
          {state.turns.length === 0 ? (
            <div className="welcome">
              <div className="welcome-mark" aria-hidden="true">
                ✳
              </div>
              <p className="eyebrow">A little room to think</p>
              <h1>What’s on your mind?</h1>
              <p className="welcome-description">
                Ask a question, work through an idea,
                <br />
                or start with a rough draft.
              </p>
              <div className="suggestions" aria-label="Conversation starters">
                {[
                  "Explain something simply",
                  "Help me write a first draft",
                  "Think through a decision",
                ].map((prompt) => (
                  <button
                    key={prompt}
                    type="button"
                    onClick={() => {
                      setDraft(prompt);
                      composer.current?.focus();
                    }}
                  >
                    {prompt}
                    <span aria-hidden="true">↗</span>
                  </button>
                ))}
              </div>
            </div>
          ) : (
            <div className="messages">
              {state.turns.map((turn) => (
                <Message key={turn.id} turn={turn} />
              ))}
              {retryable && (
                <button
                  type="button"
                  className="retry-button"
                  disabled={!model.trim()}
                  onClick={() => {
                    followReplies.current = true;
                    void conversation.retry(model);
                  }}
                >
                  Retry reply
                </button>
              )}
            </div>
          )}
        </div>

        <div className="composer-region">
          <form className="composer" onSubmit={send}>
            <label className="sr-only" htmlFor="message">
              Message
            </label>
            <textarea
              id="message"
              ref={composer}
              value={draft}
              rows={1}
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={composeKey}
              placeholder="Write a message…"
              disabled={setup.status === "failed"}
            />
            <div className="composer-toolbar">
              <span className="composer-hint">
                {setup.status === "loading"
                  ? "Connecting…"
                  : !model.trim()
                    ? "Choose a model above to begin"
                    : "Enter to send · Shift + Enter for a new line"}
              </span>
              {generating ? (
                <button
                  className="send-button stop-button"
                  type="button"
                  onClick={() => conversation.stop()}
                  aria-label="Stop reply"
                >
                  <span aria-hidden="true">■</span>
                </button>
              ) : (
                <button
                  className="send-button"
                  type="submit"
                  disabled={!canSend}
                  aria-label="Send message"
                >
                  <span aria-hidden="true">↑</span>
                </button>
              )}
            </div>
          </form>
          <p className="session-note">
            History clears on reload. Check answers that matter.
          </p>
        </div>
      </main>
      <div className="sr-only" role="status" aria-live="polite">
        {generating
          ? "Generating reply."
          : latestTurn
            ? "Reply finished."
            : "Ready to chat."}
      </div>
    </div>
  );
}
