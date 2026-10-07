import { describe, expect, test } from "bun:test";
import { RuntimeConfig } from "../src/server/RuntimeConfig";

describe("runtime configuration", () => {
  test("requires an endpoint, even when an unrelated OpenAI URL exists", () => {
    expect(() =>
      RuntimeConfig.parse([], { OPENAI_BASE_URL: "https://example.com/v1" }),
    ).toThrow();
  });

  test("flags override environment and preserve the API prefix", () => {
    const result = RuntimeConfig.parse(
      [
        "--inference-endpoint",
        "https://example.com/team/api/",
        "--port",
        "0",
        "--model",
        "selected-model",
        "--temperature",
        "0",
        "--max-output-tokens",
        "64",
      ],
      {
        INFERENCE_ENDPOINT: "http://unused.test",
        WEB_PORT: "4000",
        INFERENCE_MODEL: "other-model",
        INFERENCE_TEMPERATURE: "0.8",
        INFERENCE_MAX_OUTPUT_TOKENS: "4096",
      },
    );
    expect(result).toMatchObject({
      inferenceEndpoint: "https://example.com/team/api",
      port: 0,
      model: "selected-model",
      temperature: 0,
      maxOutputTokens: 64,
    });
  });

  test("accepts generation settings from the environment", () => {
    expect(
      RuntimeConfig.parse([], {
        INFERENCE_ENDPOINT: "http://example.com/v1",
        INFERENCE_TEMPERATURE: "0.3",
        INFERENCE_MAX_OUTPUT_TOKENS: "1024",
      }),
    ).toMatchObject({ temperature: 0.3, maxOutputTokens: 1024 });
  });

  test.each(["", "-0.1", "2.1", "NaN", "Infinity", "invalid"])(
    "rejects invalid sampling temperatures: %s",
    (temperature) => {
      expect(() =>
        RuntimeConfig.parse([], {
          INFERENCE_ENDPOINT: "http://example.com/v1",
          INFERENCE_TEMPERATURE: temperature,
        }),
      ).toThrow();
    },
  );

  test.each(["", "0", "-1", "1.5", "NaN", "9007199254740992", "invalid"])(
    "rejects invalid reply budgets: %s",
    (maxOutputTokens) => {
      expect(() =>
        RuntimeConfig.parse([], {
          INFERENCE_ENDPOINT: "http://example.com/v1",
          INFERENCE_MAX_OUTPUT_TOKENS: maxOutputTokens,
        }),
      ).toThrow();
    },
  );

  test.each([
    "not-a-url",
    "ftp://example.com/v1",
    "https://name:secret@example.com/v1",
    "https://example.com/v1?key=secret",
    "https://example.com/v1#secret",
  ])(
    "rejects unsupported endpoints without exposing their contents: %s",
    (endpoint) => {
      expect(() =>
        RuntimeConfig.parse([], { INFERENCE_ENDPOINT: endpoint }),
      ).toThrow();
      try {
        RuntimeConfig.parse([], { INFERENCE_ENDPOINT: endpoint });
      } catch (error) {
        expect(String(error)).not.toContain("secret");
      }
    },
  );

  test.each(["-1", "65536", "3.5", "", "3000junk"])(
    "rejects invalid ports: %s",
    (port) => {
      expect(() =>
        RuntimeConfig.parse([], {
          INFERENCE_ENDPOINT: "https://example.com/v1",
          WEB_PORT: port,
        }),
      ).toThrow();
    },
  );
});
