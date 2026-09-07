import { API_PRODUCT_KEYS } from "@devbox/api-studio-features/migration-keys";
export const issueMessages = {
  "source-open": "API Playground를 닫은 후 다시 확인해 주세요.",
  cancelled: "데이터 확인을 취소했습니다.",
  busy: "다른 가져오기가 진행 중입니다.",
  "recovery-required": "이전 가져오기가 중단됐습니다. 계속하거나 되돌려 주세요.",
  "destination-changed": "가져오기 계획 이후 데이터가 변경됐습니다. 새 계획을 확인해 주세요.",
  "source-large-or-changed": "데이터 크기 제한을 초과했거나 읽는 동안 데이터가 변경됐습니다. 가져올 범위를 줄여 다시 확인해 주세요.",
  "schema-invalid": "손상됐거나 지원하지 않는 저장 형식이 있어 가져올 수 없습니다.",
  "secret-reconnect-required": "기존 비밀 값을 확인할 수 없습니다. 기존 앱에서 다시 연결한 후 데이터를 확인해 주세요.",
  "no-sources": "선택한 앱에서 가져올 데이터를 찾지 못했습니다.",
  "windows-required": "기존 앱 데이터 가져오기는 Windows 앱에서 지원합니다.",
  "restart-required": "열린 요청을 저장한 후 앱을 다시 시작해 가져오기를 열어 주세요.",
  unavailable: "가져오기 상태를 확인하지 못했습니다. 다시 시도해 주세요.",
} as const;
export class MigrationError extends Error {
  constructor(readonly code: keyof typeof issueMessages) { super(issueMessages[code]); }
}
export function migrationFailure(value: unknown): MigrationError {
  if (value && typeof value === "object" && Object.keys(value).join(",") === "issue") {
    const issue = (value as { issue: unknown }).issue;
    if (typeof issue === "string" && Object.prototype.hasOwnProperty.call(issueMessages, issue)) return new MigrationError(issue as keyof typeof issueMessages);
  }
  return new MigrationError("unavailable");
}
export type BrowserState = Record<string, string | null>;
export interface Summary { added: number; matched: number; alreadyImported: number; conflicts: number; capacityExcluded: number }
export interface BrowserPatch { id: string; rollback: boolean; before: BrowserState; after: BrowserState; summary: Summary }
export function captureBrowser(storage: Storage = localStorage): BrowserState {
  return Object.fromEntries(API_PRODUCT_KEYS.map((key) => [key, storage.getItem(key)]));
}
function validateState(state: BrowserState): void {
  if (!state || typeof state !== "object" || Array.isArray(state) || Object.keys(state).length !== API_PRODUCT_KEYS.length
    || Object.keys(state).some((key) => !API_PRODUCT_KEYS.includes(key as typeof API_PRODUCT_KEYS[number]))
    || Object.values(state).some((value) => value !== null && typeof value !== "string")
    || Object.values(state).reduce((total, value) => total + new TextEncoder().encode(value ?? "").length, 0) > 20 * 1024 * 1024) throw new MigrationError("unavailable");
}
/** Resume only exact before/after values. A third value may be a user's edit. */
export function applyBrowserPatch(patch: BrowserPatch, storage: Storage = localStorage): BrowserState {
  validateState(patch.before); validateState(patch.after);
  const before = captureBrowser(storage);
  for (const key of API_PRODUCT_KEYS) if (before[key] !== patch.before[key] && before[key] !== patch.after[key]) throw new MigrationError("destination-changed");
  for (const key of API_PRODUCT_KEYS) {
    const current = storage.getItem(key);
    if (current !== patch.before[key] && current !== patch.after[key]) throw new MigrationError("destination-changed");
    if (patch.after[key] === null) storage.removeItem(key); else storage.setItem(key, patch.after[key]!);
    if (storage.getItem(key) !== patch.after[key]) throw new MigrationError("unavailable");
  }
  return captureBrowser(storage);
}
