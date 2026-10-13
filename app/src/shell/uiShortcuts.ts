import { useToasts } from "../ui/Toast";
import type { Binding } from "./keymap";
import { useLayout } from "./layout";

function textFocus() {
  return !!document.activeElement?.closest(
    'input, textarea, select, [contenteditable]:not([contenteditable="false"]), [data-keyscope="terminal"], [data-keyscope="editor"]',
  );
}
function zoomFocus() {
  return !!document.activeElement?.closest('[data-keyscope="terminal"], [data-keyscope="editor"]');
}
function readScale() {
  try {
    const saved = localStorage.getItem("devbox.uiScale");
    const value = saved === null ? 1 : Number(saved);
    return Number.isFinite(value) ? Math.min(1.5, Math.max(0.9, value)) : 1;
  } catch {
    return 1;
  }
}
export function setUiScale(scale: number) {
  const value = Math.round(Math.min(1.5, Math.max(0.9, scale)) * 100) / 100;
  document.documentElement.style.zoom = String(value);
  try {
    localStorage.setItem("devbox.uiScale", String(value));
  } catch {
    /* Zoom works without persistence. */
  }
}
export function restoreUiScale() {
  setUiScale(readScale());
}
function zoom(delta: number) {
  setUiScale((Number(document.documentElement.style.zoom) || 1) + delta);
}
export function uiBindings(): Binding[] {
  return [
    {
      id: "layout.list",
      code: "KeyB",
      ctrl: true,
      shift: true,
      scope: "app",
      label: "목록 접기",
      run: () => useLayout.getState().toggleList(),
    },
    {
      id: "layout.bottom",
      code: "Backquote",
      ctrl: true,
      scope: "app",
      label: "하단 패널",
      run: () => useLayout.getState().toggleBottom(),
    },
    {
      id: "ui.zoomIn",
      code: "Equal",
      ctrl: true,
      scope: "app",
      label: "화면 확대",
      enabled: () => !zoomFocus(),
      run: () => zoom(0.1),
    },
    {
      id: "ui.zoomOut",
      code: "Minus",
      ctrl: true,
      scope: "app",
      label: "화면 축소",
      enabled: () => !zoomFocus(),
      run: () => zoom(-0.1),
    },
    {
      id: "ui.zoomReset",
      code: "Digit0",
      ctrl: true,
      scope: "app",
      label: "화면 배율 초기화",
      enabled: () => !zoomFocus(),
      run: () => setUiScale(1),
    },
    {
      id: "toast.undo",
      code: "KeyZ",
      ctrl: true,
      scope: "app",
      label: "최근 알림 되돌리기",
      enabled: () => !textFocus() && useToasts.getState().items.some((t) => t.undo && Date.now() < t.expiresAt),
      run: () => {
        useToasts.getState().undoLatest();
      },
    },
  ];
}
