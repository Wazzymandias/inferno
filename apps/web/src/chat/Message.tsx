import { useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { ChatTurn } from "./Conversation";

/** Rendering and clipboard feedback belong to the visible exchange. */
export function Message({ turn }: { readonly turn: ChatTurn }) {
  const [copyStatus, setCopyStatus] = useState<"idle" | "copied" | "failed">(
    "idle",
  );
  const reply = turn.reply;

  return (
    <section className="exchange" aria-label="Chat exchange">
      <div className="user-message">
        <span className="message-label">You</span>
        <div className="user-content">{turn.prompt}</div>
      </div>
      <div className={`assistant-message reply-${reply.status}`}>
        <div className="assistant-heading">
          <span className="assistant-mark" aria-hidden="true">
            ✳
          </span>
          <span className="message-label">Assistant</span>
          <span className="reply-model">{turn.model}</span>
        </div>
        {reply.text ? (
          <div className="markdown">
            <Markdown
              remarkPlugins={[remarkGfm]}
              components={{
                a: (props) => (
                  <a {...props} target="_blank" rel="noreferrer noopener" />
                ),
              }}
            >
              {reply.text}
            </Markdown>
          </div>
        ) : reply.status === "streaming" ? (
          <div className="thinking" role="status">
            Thinking<span aria-hidden="true">…</span>
          </div>
        ) : null}
        {reply.status === "streaming" && reply.text && (
          <span className="stream-cursor" aria-label="Generating reply" />
        )}
        {reply.status === "stopped" && (
          <p className="reply-notice">Reply stopped.</p>
        )}
        {(reply.status === "failed" || reply.status === "incomplete") && (
          <p className="reply-notice" role="alert">
            {reply.message}
          </p>
        )}
        {reply.text && reply.status !== "streaming" && (
          <button
            className="copy-button"
            type="button"
            onClick={async () => {
              try {
                await navigator.clipboard.writeText(reply.text);
                setCopyStatus("copied");
              } catch {
                setCopyStatus("failed");
              }
            }}
          >
            {copyStatus === "copied"
              ? "Copied"
              : copyStatus === "failed"
                ? "Could not copy"
                : "Copy reply"}
          </button>
        )}
      </div>
    </section>
  );
}
