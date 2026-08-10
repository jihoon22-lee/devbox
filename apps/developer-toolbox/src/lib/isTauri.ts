/**
 * Tauri 런타임 안에서 실행 중인지 여부.
 * 브라우저(mock 모드)와 Tauri 실행 간의 분기 기준.
 */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
