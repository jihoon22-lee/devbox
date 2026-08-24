export interface Point {
  x: number;
  y: number;
}

export interface Size {
  width: number;
  height: number;
}

export interface ViewportSize extends Size {}

export interface Rect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export interface MenuPlacement extends Point {
  horizontal: "left" | "right";
  vertical: "up" | "down";
}

export const DEFAULT_VIEWPORT_MARGIN = 8;

function finite(value: number, fallback = 0): number {
  return Number.isFinite(value) ? value : fallback;
}

function nonNegative(value: number): number {
  return Math.max(0, finite(value));
}

function clamp(value: number, minimum: number, maximum: number): number {
  if (maximum < minimum) return minimum;
  return Math.min(Math.max(value, minimum), maximum);
}

function bounds(
  menu: Size,
  viewport: ViewportSize,
  margin: number,
): { maxLeft: number; maxTop: number; margin: number; menu: Size; viewport: ViewportSize } {
  const safeMargin = nonNegative(margin);
  const safeMenu = {
    width: nonNegative(menu.width),
    height: nonNegative(menu.height),
  };
  const safeViewport = {
    width: nonNegative(viewport.width),
    height: nonNegative(viewport.height),
  };
  return {
    maxLeft: Math.max(safeMargin, safeViewport.width - safeMargin - safeMenu.width),
    maxTop: Math.max(safeMargin, safeViewport.height - safeMargin - safeMenu.height),
    margin: safeMargin,
    menu: safeMenu,
    viewport: safeViewport,
  };
}

/** Pointer/keyboard anchor에서 root menu를 viewport 안에 놓는다. */
export function placeRootMenu(
  anchor: Point,
  menu: Size,
  viewport: ViewportSize,
  margin = DEFAULT_VIEWPORT_MARGIN,
): MenuPlacement {
  const safe = bounds(menu, viewport, margin);
  const x = finite(anchor.x);
  const y = finite(anchor.y);
  const horizontal: MenuPlacement["horizontal"] =
    x + safe.menu.width <= safe.viewport.width - safe.margin ? "right" : "left";
  const vertical: MenuPlacement["vertical"] =
    y + safe.menu.height <= safe.viewport.height - safe.margin ? "down" : "up";
  const preferredLeft = horizontal === "right" ? x : x - safe.menu.width;
  const preferredTop = vertical === "down" ? y : y - safe.menu.height;

  return {
    x: clamp(preferredLeft, safe.margin, safe.maxLeft),
    y: clamp(preferredTop, safe.margin, safe.maxTop),
    horizontal,
    vertical,
  };
}

/** Parent item 오른쪽을 우선하고 공간이 부족하면 왼쪽으로 submenu를 뒤집는다. */
export function placeSubmenu(
  parent: Rect,
  menu: Size,
  viewport: ViewportSize,
  margin = DEFAULT_VIEWPORT_MARGIN,
): MenuPlacement {
  const safe = bounds(menu, viewport, margin);
  const leftEdge = finite(parent.left);
  const rightEdge = finite(parent.right, leftEdge);
  const topEdge = finite(parent.top);
  const rightSpace = safe.viewport.width - safe.margin - rightEdge;
  const leftSpace = leftEdge - safe.margin;
  const fitsRight = rightSpace >= safe.menu.width;
  const fitsLeft = leftSpace >= safe.menu.width;
  const horizontal: MenuPlacement["horizontal"] = fitsRight
    ? "right"
    : fitsLeft || leftSpace > rightSpace
      ? "left"
      : "right";
  const preferredLeft = horizontal === "right" ? rightEdge : leftEdge - safe.menu.width;
  const preferredTop = topEdge;
  const top = clamp(preferredTop, safe.margin, safe.maxTop);

  return {
    x: clamp(preferredLeft, safe.margin, safe.maxLeft),
    y: top,
    horizontal,
    vertical: top < topEdge ? "up" : "down",
  };
}
