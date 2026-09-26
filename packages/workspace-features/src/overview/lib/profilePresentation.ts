import { type KeyboardEvent as ReactKeyboardEvent } from "react";
import { type PreflightItem, type ResourceProvenance, type RuntimeSuggestions } from "../api";

export const RUNTIME_STATUS_LABEL: Record<RuntimeSuggestions["status"], string> = {
  fresh: "최신 snapshot",
  stale: "오래된 snapshot — 반영 시 추가 확인 필요",
  expired: "만료된 snapshot — 반영 불가",
  missing: "WSL Desktop snapshot 없음",
  corrupt: "WSL Desktop snapshot을 안전하게 읽을 수 없음",
};

export const PREFLIGHT_ITEM_LABEL: Record<string, string> = {
  "required-apps": "필수 앱",
  "wsl-distro": "WSL 배포판",
  "working-directory": "작업 디렉터리",
  ports: "예상 포트",
  "service-dependencies": "서비스 dependency",
};

export const PREFLIGHT_STATUS_LABEL: Record<PreflightItem["status"], string> = {
  pass: "통과",
  warning: "경고",
  failure: "차단",
  unavailable: "확인 불가",
};

export const RESOURCE_STATE_LABEL: Record<ResourceProvenance["state"], string> = {
  available: "사용 가능",
  existing: "이미 실행 중",
  workbenchStarted: "Workbench가 시작",
  notRunning: "실행 전",
  missing: "없음",
  conflict: "충돌",
  unsafe: "안전하지 않음",
  unavailable: "확인 불가",
};

export const DIALOG_FOCUSABLE_SELECTOR = [
  "button:not([disabled])",
  "[href]",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(", ");

export function trapModalFocus(event: ReactKeyboardEvent<HTMLElement>, close: () => void, busy: boolean) {
  if (event.key === "Escape" && !event.nativeEvent.isComposing && !busy) {
    event.preventDefault();
    close();
    return;
  }
  if (event.key !== "Tab") return;
  const focusable = Array.from(event.currentTarget.querySelectorAll<HTMLElement>(DIALOG_FOCUSABLE_SELECTOR));
  if (focusable.length === 0) {
    event.preventDefault();
    return;
  }
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}
