import { invoke } from "@tauri-apps/api/core";
const nonce = (window as unknown as Record<string, unknown>).__DEVBOX_TERMINAL_EXPORT__;
const keys = ["wsl-desktop:cwd-pinned", "wsl-desktop:cwd-value", "wsl-desktop:recent-paths", "wsl-desktop:copy-on-select", "wsl-desktop:font-size", "wsl-desktop:settings", "wsl-desktop:last-layout"];
async function message<T>(action: Record<string, unknown>): Promise<T> {
  if (typeof nonce !== "string" || !/^[a-f0-9]{32}$/.test(nonce)) throw new Error("export ticket unavailable");
  return invoke<T>("plugin:workspace|terminal_export_message", { request: { nonce, action } });
}
async function run() {
  const deadline = Date.now() + 30_000;
  while (!(await message<{ ready: boolean }>({ kind: "open" })).ready) {
    if (Date.now() > deadline) throw new Error("profile verification expired");
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  const values: Record<string, string | null> = {};
  for (const key of keys) {
    const value = localStorage.getItem(key);
    if (value !== null && new TextEncoder().encode(value).length > 1024 * 1024) throw new Error("export value too large");
    values[key] = value;
  }
  await message({ kind: "complete", data: { schemaVersion: 1, values } });
}
void run().catch(() => { void message({ kind: "failed" }).catch(() => undefined); });
