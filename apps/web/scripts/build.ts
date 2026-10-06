// Runtime configuration and secrets are deliberately absent from the browser build.
await Bun.$`rm -rf dist`;
const result = await Bun.build({
  entrypoints: ["./src/index.html"],
  outdir: "./dist",
  target: "browser",
  minify: true,
  define: { "process.env.NODE_ENV": JSON.stringify("production") },
});
if (!result.success) {
  for (const diagnostic of result.logs) console.log(diagnostic);
  process.exitCode = 1;
} else {
  console.log(`Built ${result.outputs.length} web assets in dist/.`);
}
