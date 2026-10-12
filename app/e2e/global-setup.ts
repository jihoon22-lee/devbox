import { execFileSync } from "node:child_process";
import { join } from "node:path";

// Both public entrypoints already hold the shared verification lock.
export default function globalSetup() {
  execFileSync("cargo", ["build", "--locked", "-q", "-p", "devbox-cli"], { stdio: "inherit" });
  const meta = JSON.parse(execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"]).toString());
  process.env.DEVBOX_E2E_BIN = join(meta.target_directory, "debug", "devbox");
}
