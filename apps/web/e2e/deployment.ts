import { fileURLToPath } from "node:url";
import { z } from "zod";

export const repository = fileURLToPath(new URL("../../../", import.meta.url));

/** Compose owns the published address; tests never select a second model or port. */
export async function deployedWebURL(): Promise<string> {
  const environment = (await Bun.file(`${repository}/.env`).exists())
    ? ".env"
    : ".env.example";
  const result = Bun.spawnSync(
    [
      "docker",
      "compose",
      "--env-file",
      environment,
      "config",
      "--format",
      "json",
    ],
    { cwd: repository, stdout: "pipe", stderr: "pipe" },
  );
  if (result.exitCode !== 0) {
    // Compose configuration may contain credentials; never print its output.
    throw new Error(
      "Cannot resolve the deployment configuration. Check Docker and the root .env.",
    );
  }
  const configuration = z
    .object({
      services: z.object({
        web: z.object({
          ports: z
            .array(
              z.object({
                published: z.string(),
                host_ip: z.string().optional(),
              }),
            )
            .length(1),
        }),
      }),
    })
    .parse(JSON.parse(result.stdout.toString()));
  const port = configuration.services.web.ports[0]!;
  const host =
    !port.host_ip || port.host_ip === "0.0.0.0"
      ? "127.0.0.1"
      : port.host_ip === "::"
        ? "::1"
        : port.host_ip;
  return `http://${host.includes(":") ? `[${host}]` : host}:${port.published}`;
}
