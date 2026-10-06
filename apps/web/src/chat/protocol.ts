import { z } from "zod";

/** The browser/server contract. Provider-specific Responses events stay on the server. */
export const ChatProtocol = {
  configuration: "/api/chat/configuration",
  responses: "/api/chat/responses",
  streamContentType: "application/x-ndjson",
} as const;

export const ChatRequest = z.strictObject({
  model: z.string().trim().min(1),
  input: z
    .array(
      z.strictObject({
        role: z.enum(["user", "assistant"]),
        content: z.string().min(1),
      }),
    )
    .min(1)
    .refine((input) => input.at(-1)?.role === "user", {
      message: "A chat request must end with a user message.",
    }),
});
export type ChatRequest = z.infer<typeof ChatRequest>;

export const ChatEvent = z.discriminatedUnion("type", [
  z.strictObject({ type: z.literal("delta"), text: z.string() }),
  z.strictObject({ type: z.literal("completed"), text: z.string().min(1) }),
  z.strictObject({
    type: z.literal("incomplete"),
    text: z.string(),
    message: z.string(),
  }),
  z.strictObject({ type: z.literal("failed"), message: z.string() }),
]);
export type ChatEvent = z.infer<typeof ChatEvent>;

export const InferenceCatalog = z.discriminatedUnion("status", [
  z.strictObject({ status: z.literal("ready"), models: z.array(z.string()) }),
  z.strictObject({ status: z.literal("unavailable"), message: z.string() }),
]);
export type InferenceCatalog = z.infer<typeof InferenceCatalog>;

export const ChatConfiguration = z.strictObject({
  defaultModel: z.string(),
  catalog: InferenceCatalog,
});
export type ChatConfiguration = z.infer<typeof ChatConfiguration>;
