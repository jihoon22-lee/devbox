import type { Layout } from "../types";

export type FocusDirection = "left" | "right" | "up" | "down";

/** 활성 탭의 팬이 화면에서 몇 열로 놓이는지. PaneCanvas의 grid 계산과 같은 규칙이다. */
export function paneColumns(layout: Layout, count: number): number {
  if (count <= 0) return 1;
  if (layout === "cols") return count;
  if (layout === "rows") return 1;
  return Math.ceil(Math.sqrt(count));
}

/**
 * 방향키로 이동할 팬의 index. 목록을 순환하지 않고 실제로 그 방향에 팬이 있을 때만
 * 옮긴다 — 격자에서 오른쪽을 눌렀는데 아래 줄로 내려가지 않게 한다.
 */
export function nextPaneIndex(
  layout: Layout,
  count: number,
  current: number,
  direction: FocusDirection,
): number | null {
  if (count <= 0 || current < 0 || current >= count) return null;
  const columns = paneColumns(layout, count);
  const row = Math.floor(current / columns);
  const column = current % columns;

  if (direction === "left" || direction === "right") {
    const nextColumn = column + (direction === "right" ? 1 : -1);
    if (nextColumn < 0 || nextColumn >= columns) return null;
    const index = row * columns + nextColumn;
    return index < count ? index : null;
  }

  const nextRow = row + (direction === "down" ? 1 : -1);
  if (nextRow < 0) return null;
  const index = nextRow * columns + column;
  return index >= 0 && index < count ? index : null;
}
