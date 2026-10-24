import { invoke } from "@tauri-apps/api/core";
import type { AppendNote } from "../../rpc/gen/rpc";

export const DRAFT_KEY = "devbox.capture.draft";
export function captureKey(event: {
  key: string;
  shiftKey: boolean;
  isComposing: boolean;
  keyCode: number;
}): "none" | "submit" | "hide" {
  if (event.isComposing || event.keyCode === 229) return "none";
  if (event.key === "Escape") return "hide";
  return event.key === "Enter" && !event.shiftKey ? "submit" : "none";
}
export function readCaptureDraft(): string {
  try {
    return sessionStorage.getItem(DRAFT_KEY) ?? "";
  } catch {
    return "";
  }
}
export function saveCaptureDraft(text: string): boolean {
  try {
    if (text) sessionStorage.setItem(DRAFT_KEY, text);
    else sessionStorage.removeItem(DRAFT_KEY);
    return true;
  } catch {
    return false;
  }
}
export async function hideCapture() {
  if ("__TAURI_INTERNALS__" in window) await invoke("capture_hide");
  else window.close();
}
export async function submitCapture(
  text: string,
  append: (params: AppendNote) => Promise<unknown>,
  hide: () => Promise<void>,
  clear: (text: string) => void,
) {
  if (!text.trim()) return;
  await append({ target: "inbox", text, format: "capture" });
  clear("");
  await hide();
}
