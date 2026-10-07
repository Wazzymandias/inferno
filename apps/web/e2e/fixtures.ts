import { test as base } from "@playwright/test";
import { RuntimeConfig } from "../src/server/RuntimeConfig";
import { WebApplication } from "../src/server/WebApplication";
import { InferenceFixture } from "../tests/InferenceFixture";

/** Each browser test owns its HTTP backend and web server, including teardown. */
export const test = base.extend<{
  inference: InferenceFixture;
  web: Awaited<ReturnType<WebApplication["start"]>>;
}>({
  inference: async ({}, use) => {
    const inference = new InferenceFixture();
    try {
      await use(inference);
    } finally {
      await inference.stop();
    }
  },
  web: async ({ inference }, use) => {
    const config = RuntimeConfig.parse([], {
      NODE_ENV: "production",
      INFERENCE_ENDPOINT: inference.endpoint,
      INFERENCE_MODEL: "test-model",
      INFERENCE_API_KEY: "server-test-secret",
      WEB_HOST: "127.0.0.1",
      WEB_PORT: "0",
    });
    if (config === "help") throw new Error("Expected runtime configuration");
    const web = await new WebApplication(config).start();
    try {
      await use(web);
    } finally {
      await web.stop(true);
    }
  },
});
