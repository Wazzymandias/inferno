import { parseArgs } from "node:util";

/** One definition owns each flag, its environment fallback, and its help text. */
const settings = {
  "inference-endpoint": {
    environment: "INFERENCE_ENDPOINT",
    description:
      "Required HTTP(S) API base URL, including its prefix (for example /v1).",
    fallback: "",
  },
  model: {
    environment: "INFERENCE_MODEL",
    description: "Initial model ID. Otherwise choose or enter a model in chat.",
    fallback: "",
  },
  temperature: {
    environment: "INFERENCE_TEMPERATURE",
    description:
      "Sampling temperature from 0 to 2. 0 uses greedy decoding: choose the most likely next token.",
    fallback: "0",
  },
  "max-output-tokens": {
    environment: "INFERENCE_MAX_OUTPUT_TOKENS",
    description:
      "Positive integer token budget for each reply, including any reasoning tokens. Stops long generations at this limit.",
    fallback: "512",
  },
  host: {
    environment: "WEB_HOST",
    description: "HTTP listener address.",
    fallback: "0.0.0.0",
  },
  port: {
    environment: "WEB_PORT",
    description: "HTTP listener port; 0 selects an available port.",
    fallback: "3000",
  },
} as const;

export class RuntimeConfig {
  private constructor(
    readonly inferenceEndpoint: string,
    readonly model: string,
    readonly temperature: number,
    readonly maxOutputTokens: number,
    readonly host: string,
    readonly port: number,
    readonly apiKey: string,
    readonly mode: "development" | "production",
  ) {}

  static parse(
    args: string[],
    environment: Readonly<Record<string, string | undefined>>,
  ): RuntimeConfig | "help" {
    const options: Record<
      string,
      { type: "string" | "boolean"; short?: string }
    > = Object.fromEntries(
      Object.keys(settings).map((name) => [name, { type: "string" as const }]),
    );
    options["help"] = { type: "boolean", short: "h" };
    const { values } = parseArgs({
      args,
      options,
      strict: true,
      allowPositionals: false,
    });
    if (values["help"]) return "help";

    const value = (name: keyof typeof settings): string => {
      const setting = settings[name];
      const flag = values[name];
      return typeof flag === "string"
        ? flag.trim()
        : (environment[setting.environment] ?? setting.fallback).trim();
    };

    const endpoint = value("inference-endpoint");
    if (!endpoint) {
      throw new Error(
        "Set --inference-endpoint or INFERENCE_ENDPOINT to your Responses API base URL.",
      );
    }
    let url: URL;
    try {
      url = new URL(endpoint);
    } catch {
      throw new Error(
        "--inference-endpoint must be an absolute HTTP(S) API base URL.",
      );
    }
    if (
      !["http:", "https:"].includes(url.protocol) ||
      url.username ||
      url.password ||
      url.search ||
      url.hash
    ) {
      throw new Error(
        "--inference-endpoint must use HTTP(S) without credentials, a query, or a fragment. Use INFERENCE_API_KEY for authentication.",
      );
    }

    const temperatureValue = value("temperature");
    const temperature = Number(temperatureValue);
    if (
      !temperatureValue ||
      !Number.isFinite(temperature) ||
      temperature < 0 ||
      temperature > 2
    ) {
      throw new Error(
        "--temperature / INFERENCE_TEMPERATURE must be a number between 0 and 2; 0 uses greedy decoding.",
      );
    }
    const maxOutputTokensValue = value("max-output-tokens");
    const maxOutputTokens = Number(maxOutputTokensValue);
    if (
      !/^\d+$/.test(maxOutputTokensValue) ||
      !Number.isSafeInteger(maxOutputTokens) ||
      maxOutputTokens < 1
    ) {
      throw new Error(
        "--max-output-tokens / INFERENCE_MAX_OUTPUT_TOKENS must be a positive integer token budget for each reply.",
      );
    }

    const port = value("port");
    if (!/^\d+$/.test(port) || Number(port) > 65535) {
      throw new Error(
        "--port / WEB_PORT must be an integer between 0 and 65535.",
      );
    }
    const host = value("host");
    if (!host) throw new Error("--host / WEB_HOST cannot be empty.");
    const mode =
      environment["NODE_ENV"] === "production" ? "production" : "development";
    return new RuntimeConfig(
      url.href.replace(/\/+$/, ""),
      value("model"),
      temperature,
      maxOutputTokens,
      host,
      Number(port),
      environment["INFERENCE_API_KEY"]?.trim() ?? "",
      mode,
    );
  }

  static help(): string {
    return [
      "Usage: bun run dev|start --inference-endpoint <API base URL> [options]",
      "",
      ...Object.entries(settings).map(
        ([name, setting]) =>
          `  --${name} <value> (${setting.environment})\n    ${setting.description}${setting.fallback ? ` Default: ${setting.fallback}.` : ""}`,
      ),
      "  --help, -h\n    Show this help without starting a server.",
      "",
      "INFERENCE_API_KEY supplies an optional bearer token on the server only.",
      "NODE_ENV=production serves built assets; run bun run build first.",
    ].join("\n");
  }
}
