import { RuntimeConfig } from "./RuntimeConfig";
import { WebApplication } from "./WebApplication";

try {
  const config = RuntimeConfig.parse(Bun.argv.slice(2), process.env);
  if (config === "help") {
    console.log(RuntimeConfig.help());
  } else {
    const server = await new WebApplication(config).start();
    console.log(
      JSON.stringify({
        event: "web.started",
        host: server.hostname,
        port: server.port,
      }),
    );
    const shutdown = async () => {
      console.log(JSON.stringify({ event: "web.stopping" }));
      await server.stop(true);
      process.exit(0);
    };
    process.once("SIGTERM", shutdown);
    process.once("SIGINT", shutdown);
    import.meta.hot?.dispose(() => {
      process.off("SIGTERM", shutdown);
      process.off("SIGINT", shutdown);
    });
  }
} catch (error) {
  console.log(
    error instanceof Error ? error.message : "The web server could not start.",
  );
  process.exitCode = 1;
}
