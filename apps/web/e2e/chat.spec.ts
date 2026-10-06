import { expect, test } from "@playwright/test";
import { RuntimeConfig } from "../src/server/RuntimeConfig";
import { WebApplication } from "../src/server/WebApplication";
import { InferenceFixture } from "../tests/InferenceFixture";

let inference: InferenceFixture;
let web: Awaited<ReturnType<WebApplication["start"]>>;

test.beforeEach(async () => {
  inference = new InferenceFixture();
  const config = RuntimeConfig.parse([], {
    NODE_ENV: "production",
    INFERENCE_ENDPOINT: inference.endpoint,
    INFERENCE_MODEL: "test-model",
    INFERENCE_API_KEY: "server-test-secret",
    WEB_HOST: "127.0.0.1",
    WEB_PORT: "0",
  });
  if (config === "help") throw new Error("Expected runtime configuration");
  web = await new WebApplication(config).start();
});

test.afterEach(async () => {
  await web?.stop(true);
  await inference?.stop();
});

test("streams a chat, remembers context, renders markdown, copies, and starts fresh", async ({
  page,
  context,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await context.grantPermissions(["clipboard-read", "clipboard-write"], {
    origin: web.url.origin,
  });
  await page.goto(web.url.href);
  await expect(page.getByLabel("Model", { exact: true })).toHaveValue(
    "test-model",
  );
  await page.getByLabel("Message", { exact: true }).fill("Say hello");
  await page.getByLabel("Message", { exact: true }).press("Enter");
  await expect(page.locator(".markdown strong")).toHaveText(
    "A streamed reply.",
  );
  await page.getByRole("button", { name: "Copy reply" }).click();
  await expect(page.getByRole("button", { name: "Copied" })).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
    inference.reply,
  );
  await page
    .getByLabel("Message", { exact: true })
    .fill("What did I just ask?");
  await page.getByRole("button", { name: "Send message" }).click();
  await expect(page.locator(".reply-completed")).toHaveCount(2);
  expect(inference.requests[1]?.input).toEqual([
    { role: "user", content: "Say hello" },
    { role: "assistant", content: inference.reply },
    { role: "user", content: "What did I just ask?" },
  ]);
  await page.getByRole("button", { name: "New chat" }).click();
  await expect(
    page.getByRole("heading", { name: "What’s on your mind?" }),
  ).toBeVisible();
  expect(errors).toEqual([]);
});

test("stops an in-flight response and retries without adding another user turn", async ({
  page,
}) => {
  inference.behavior = "slow";
  await page.goto(web.url.href);
  await expect(page.getByLabel("Model", { exact: true })).toHaveValue(
    "test-model",
  );
  await page.getByLabel("Message", { exact: true }).fill("A slow reply");
  await page.getByRole("button", { name: "Send message" }).click();
  await expect(page.locator(".markdown")).toContainText("Hello");
  await page.getByRole("button", { name: "Stop reply" }).click();
  await expect(page.getByText("Reply stopped.", { exact: true })).toBeVisible();
  await expect.poll(() => inference.cancellations).toBeGreaterThan(0);
  inference.behavior = "completed";
  await page.getByRole("button", { name: "Retry reply" }).click();
  await expect(page.locator(".reply-completed")).toHaveCount(1);
  await expect(page.locator(".user-content")).toHaveCount(1);
  expect(inference.requests).toHaveLength(2);
});

test("shows inference errors and allows custom model IDs when discovery is unavailable", async ({
  page,
}) => {
  inference.discovery = "unavailable";
  await page.goto(web.url.href);
  await expect(
    page.getByText("Model discovery is unavailable.", { exact: false }),
  ).toBeVisible();
  await page.getByLabel("Model", { exact: true }).fill("broken-model");
  await page.getByLabel("Message", { exact: true }).fill("Hello");
  await page.getByRole("button", { name: "Send message" }).click();
  await expect(page.getByRole("alert")).toContainText("rejected this request");
  await page.getByLabel("Model", { exact: true }).fill("custom-model");
  await page
    .getByLabel("Message", { exact: true })
    .fill("Try the custom model");
  await page.getByRole("button", { name: "Send message" }).click();
  await expect(page.locator(".reply-completed")).toHaveCount(1);
  expect(inference.requests[1]?.model).toBe("custom-model");
});

test("mobile layout supports keyboard composition without horizontal overflow", async ({
  page,
}) => {
  await page.setViewportSize({ width: 375, height: 812 });
  await page.goto(web.url.href);
  await expect(page.getByLabel("Model", { exact: true })).toHaveValue(
    "test-model",
  );
  const composer = page.getByLabel("Message", { exact: true });
  await composer.fill("First line");
  await composer.press("Shift+Enter");
  await composer.pressSequentially("Second line");
  await expect(composer).toHaveValue("First line\nSecond line");
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await composer.press("Enter");
  await expect(page.locator(".reply-completed")).toHaveCount(1);
  expect(inference.requests[0]?.input[0]?.content).toBe(
    "First line\nSecond line",
  );
});
