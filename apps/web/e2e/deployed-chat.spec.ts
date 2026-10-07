import { expect, test } from "@playwright/test";

test("the deployed model completes replies and remembers the conversation", async ({
  page,
}) => {
  // Includes real generations and any model reasoning before visible text.
  test.setTimeout(3 * 60_000);
  const browserErrors: string[] = [];
  page.on("pageerror", (error) => browserErrors.push(error.message));
  await page.goto("/");
  await expect(page.getByLabel("Model", { exact: true })).not.toHaveValue("");
  await expect(
    page.getByText("Model discovery is unavailable.", { exact: false }),
  ).toHaveCount(0);

  for (const [index, prompt, answer] of [
    [0, "What is 17 multiplied by 19? Reply with only the integer.", "323"],
    [1, "Add one to your previous answer. Reply with only the integer.", "324"],
    [
      2,
      "List the integers from 1 through 20, separated by commas and single spaces. Output only the list.",
      Array.from({ length: 20 }, (_, index) => String(index + 1)).join(", "),
    ],
  ] as const) {
    await test.step(`complete conversation turn ${index + 1}`, async () => {
      await page.getByLabel("Message", { exact: true }).fill(prompt);
      await page.getByRole("button", { name: "Send message" }).click();
      const exchange = page
        .getByRole("region", { name: "Chat exchange" })
        .nth(index);
      // Wait on user-visible output, not Chrome's buffering of a consumed stream.
      await expect(exchange.getByRole("paragraph")).toHaveText(answer, {
        timeout: 60_000,
      });
      await expect(
        exchange.getByRole("button", { name: "Copy reply" }),
      ).toBeVisible();
      await expect(exchange.getByRole("alert")).toHaveCount(0);
      await expect(
        page.getByRole("button", { name: "Stop reply" }),
      ).toHaveCount(0);
      await expect(
        page.getByRole("status").filter({ hasText: "Reply finished." }),
      ).toBeVisible();
    });
  }
  expect(browserErrors).toEqual([]);
});
