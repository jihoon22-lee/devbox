import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface Attention {
  id: string;
  title: string;
  body?: string;
  visible: boolean;
}
export interface AgentNavigation {
  to: "/agents";
  search: { id: string; view: "list" };
}
interface NavigationIntent extends AgentNavigation {
  sequence: number;
}
function mainHost() {
  return "__TAURI_INTERNALS__" in window && getCurrentWindow().label === "main";
}
function report(error: unknown) {
  console.error("네이티브 알림을 처리하지 못했습니다", error);
}
export async function nativeAttention(a: Attention) {
  if (mainHost()) await invoke("notify_attention", { a: { ...a, body: a.body ?? "" } }).catch(report);
}
export function nativeBadge(attention: number, open: number) {
  if ("__TAURI_INTERNALS__" in window) {
    if (mainHost()) void invoke("set_badge", { attention, open }).catch(report);
  } else {
    const title = attention ? `(${attention}) devbox` : "devbox";
    if (document.title !== title) document.title = title;
  }
}
function intent(value: unknown): value is NavigationIntent {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<NavigationIntent>;
  return (
    Number.isSafeInteger(candidate.sequence) &&
    (candidate.sequence ?? 0) > 0 &&
    candidate.to === "/agents" &&
    candidate.search?.view === "list" &&
    typeof candidate.search.id === "string" &&
    /^[A-Za-z0-9]*$/.test(candidate.search.id)
  );
}
/** Register before reading pending state; ack only a completed navigation while still mounted. */
export function subscribeNativeNavigation(navigate: (target: AgentNavigation) => Promise<void> | void) {
  if (!mainHost()) return () => {};
  let cancelled = false;
  let release: (() => void) | undefined;
  let newest = 0;
  let pending = Promise.resolve();
  const receive = (value: unknown) => {
    if (cancelled || !intent(value) || value.sequence <= newest) return;
    newest = value.sequence;
    pending = pending
      .then(async () => {
        if (cancelled || value.sequence < newest) return;
        await navigate({ to: value.to, search: value.search });
        if (!cancelled) await invoke("acknowledge_navigation", { sequence: value.sequence });
      })
      .catch(report);
  };
  void listen<NavigationIntent>("devbox://navigate", (event) => receive(event.payload))
    .then(async (unlisten) => {
      if (cancelled) {
        unlisten();
        return;
      }
      release = unlisten;
      receive(await invoke<NavigationIntent | null>("pending_navigation"));
    })
    .catch(report);
  return () => {
    cancelled = true;
    release?.();
  };
}
