import { describe, expect, test } from "bun:test";
import { RuntimeConfig } from "../src/server/RuntimeConfig";

describe("runtime configuration", () => {
  test("requires an endpoint, even when an unrelated OpenAI URL exists", () => {
    expect(() =>
      RuntimeConfig.parse([], { OPENAI_BASE_URL: "https://example.com/v1" }),
    ).toThrow("--inference-endpoint");
    expect(RuntimeConfig.parse(["--help"], {})).toBe("help");
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
      ],
      {
        INFERENCE_ENDPOINT: "http://unused.test",
        WEB_PORT: "4000",
        INFERENCE_MODEL: "other-model",
      },
    );
    expect(result).toMatchObject({
      inferenceEndpoint: "https://example.com/team/api",
      port: 0,
      model: "selected-model",
    });
  });

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
      ).toThrow("--inference-endpoint");
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
      ).toThrow("--port");
    },
  );
});
