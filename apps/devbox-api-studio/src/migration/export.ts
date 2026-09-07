import { invoke } from "@tauri-apps/api/core";
import { normalizeLegacyApiStorage, readLegacyApiStorage } from "@devbox/api-studio-features/migration";
const nonce = (window as unknown as Record<string, unknown>).__DEVBOX_API_EXPORT__;
async function message<T>(action: Record<string, unknown>): Promise<T> {
  if (typeof nonce !== "string" || !/^[a-f0-9]{32}$/.test(nonce)) throw new Error("legacy_export_ticket_invalid");
  return invoke<T>("plugin:api-studio|legacy_export_message", { request: { nonce, action } });
}
async function run() {
  const deadline = Date.now() + 30_000;
  for (;;) {
    const { ready } = await message<{ ready: boolean }>({ kind: "open" });
    if (ready) break;
    if (Date.now() > deadline) throw new Error("legacy_export_profile_unverified");
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  // The native worker must verify the actual copied profile before this read.
  const raw = readLegacyApiStorage(localStorage);
  const data = await normalizeLegacyApiStorage(raw, {
    prepareEnvironment: (value) => message({ kind: "environment", raw: value }),
    sanitize: (serialized) => message({ kind: "sanitize", serialized }),
  });
  await message({ kind: "complete", data });
}
void run().catch(() => { void message({ kind: "failed" }).catch(() => undefined); });
