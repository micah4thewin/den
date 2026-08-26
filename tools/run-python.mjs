import { spawnSync } from "node:child_process";

const [script, ...rest] = process.argv.slice(2);
if (!script) {
  console.error("usage: run-python.mjs <script.py> [args...]");
  process.exit(2);
}

const candidates = process.platform === "win32"
  ? [["py", ["-3"]], ["python", []], ["python3", []]]
  : [["python3", []], ["python", []]];

for (const [program, prefix] of candidates) {
  const probe = spawnSync(program, [...prefix, "--version"], { stdio: "ignore" });
  if (probe.error || probe.status !== 0) continue;
  const run = spawnSync(program, [...prefix, script, ...rest], { stdio: "inherit" });
  process.exit(run.status ?? 1);
}

console.error(
  "No Python found. Play's runtime bundling needs Python 3; install it, or\n" +
  "skip bundling — a build without a staged runtime still works and falls\n" +
  "back to whatever RetroArch is installed.",
);
process.exit(0);
