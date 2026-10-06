import OpenAI from "openai";
import type { Response as ModelResponse } from "openai/resources/responses/responses";
import type {
  ChatEvent,
  ChatRequest,
  InferenceCatalog,
} from "../chat/protocol";
import type { RuntimeConfig } from "./RuntimeConfig";

/** Owns the Responses API boundary; no provider URL or credentials reach the browser. */
export class InferenceService {
  readonly #client: OpenAI;

  constructor(private readonly config: RuntimeConfig) {
    this.#client = new OpenAI({
      baseURL: config.inferenceEndpoint,
      // The SDK requires a nonempty key. This sentinel is never sent: the
      // Authorization header is explicitly removed for unauthenticated endpoints.
      apiKey: config.apiKey || "unused",
      defaultHeaders: config.apiKey ? {} : { Authorization: null },
      organization: null,
      project: null,
      // The application emits its own redacted events; SDK debug logs can include bodies.
      logLevel: "off",
      // A retry of a generation can silently create a second response.
      maxRetries: 0,
    });
  }

  async catalog(signal: AbortSignal): Promise<InferenceCatalog> {
    try {
      const models: string[] = [];
      // Discovery is optional. Bound setup latency while leaving manual IDs usable.
      for await (const model of this.#client.models.list({
        signal,
        timeout: 10_000,
      })) {
        models.push(model.id);
      }
      return { status: "ready", models: [...new Set(models)].sort() };
    } catch (error) {
      if (signal.aborted) throw error;
      return {
        status: "unavailable",
        message: "Model discovery is unavailable. Enter a model ID to chat.",
      };
    }
  }

  async *respond(
    request: ChatRequest,
    signal: AbortSignal,
  ): AsyncGenerator<ChatEvent> {
    try {
      const stream = await this.#client.responses.create(
        {
          model: request.model,
          input: request.input,
          // Send the chat policy explicitly; backend sampling defaults can differ.
          temperature: this.config.temperature,
          max_output_tokens: this.config.maxOutputTokens,
          stream: true,
          store: false,
        },
        { signal },
      );

      for await (const event of stream) {
        switch (event.type) {
          case "response.output_text.delta":
          case "response.refusal.delta":
            yield { type: "delta", text: event.delta };
            break;
          case "response.completed": {
            const text = this.#responseText(event.response);
            yield text
              ? { type: "completed", text }
              : {
                  type: "failed",
                  message:
                    "The model completed without a text reply. Choose a model that supports text responses.",
                };
            return;
          }
          case "response.incomplete":
            yield {
              type: "incomplete",
              text: this.#responseText(event.response),
              message:
                event.response.incomplete_details?.reason ===
                "max_output_tokens"
                  ? "The model reached its output limit."
                  : "The model ended this reply before it was complete.",
            };
            return;
          case "response.failed":
          case "error":
            yield {
              type: "failed",
              message:
                "The inference provider failed to generate a reply. Retry or choose another model.",
            };
            return;
        }
      }
      yield {
        type: "failed",
        message:
          "The inference stream ended before the reply was complete. You can retry.",
      };
    } catch (error) {
      if (signal.aborted) return;
      yield { type: "failed", message: this.#failureMessage(error) };
    }
  }

  #responseText(response: ModelResponse): string {
    return response.output
      .flatMap((item) =>
        item.type === "message"
          ? item.content.map((part) =>
              part.type === "output_text" ? part.text : part.refusal,
            )
          : [],
      )
      .join("\n\n");
  }

  #failureMessage(error: unknown): string {
    // Provider error bodies can echo requests or credentials. Expose actionable status only.
    if (error instanceof OpenAI.APIConnectionTimeoutError) {
      return "The inference request timed out. You can retry.";
    }
    if (error instanceof OpenAI.APIError) {
      switch (error.status) {
        case 400:
        case 422:
          return "The inference provider rejected this request. Check the model ID and conversation length.";
        case 401:
        case 403:
          return "Inference authentication failed. Check INFERENCE_API_KEY on the web server.";
        case 404:
          return "The inference endpoint or model was not found. Check the API base URL and model ID.";
        case 429:
          return "The inference provider is at its request limit. Wait a moment and retry.";
        case 408:
        case 504:
          return "The inference request timed out. You can retry.";
      }
      return "The inference provider failed to generate a reply. Retry or choose another model.";
    }
    return "The inference provider could not be reached or returned an invalid response. Check the endpoint and retry.";
  }
}
