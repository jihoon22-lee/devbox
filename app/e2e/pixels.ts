import type { Locator, Page } from "@playwright/test";

/** A colour as red, green, blue (0–255) and alpha (0–1). */
export type Rgba = [number, number, number, number];

/** The screenshot of an element's box (widened to whole pixels), decoded in the page. */
export async function pixelsOf(page: Page, locator: Locator) {
  const box = await locator.boundingBox();
  if (!box) throw new Error("no box to capture");
  const [x, y] = [Math.floor(box.x), Math.floor(box.y)];
  const clip = { x, y, width: Math.ceil(box.x + box.width) - x, height: Math.ceil(box.y + box.height) - y };
  const png = await page.screenshot({ clip });
  const data = await page.evaluate(async (base64) => {
    const bytes = Uint8Array.from(atob(base64), (char) => char.charCodeAt(0));
    const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const context = new OffscreenCanvas(bitmap.width, bitmap.height).getContext("2d");
    if (!context) throw new Error("no 2d context");
    context.drawImage(bitmap, 0, 0);
    return Array.from(context.getImageData(0, 0, bitmap.width, bitmap.height).data);
  }, png.toString("base64"));
  const at = (px: number, py: number): Rgba => {
    const i = (py * clip.width + px) * 4;
    return [data[i]!, data[i + 1]!, data[i + 2]!, 1];
  };
  return { width: clip.width, height: clip.height, at };
}
export type Pixels = Awaited<ReturnType<typeof pixelsOf>>;
/** Every channel within `tolerance` of the other colour's. */
export const near = (a: Rgba, b: Rgba, tolerance = 48) => [0, 1, 2].every((i) => Math.abs(a[i]! - b[i]!) <= tolerance);
export const hex = (color: Rgba) =>
  `#${color
    .slice(0, 3)
    .map((c) => Math.round(c).toString(16).padStart(2, "0"))
    .join("")}`;
export function each(pixels: Pixels) {
  const all: Rgba[] = [];
  for (let y = 0; y < pixels.height; y++) for (let x = 0; x < pixels.width; x++) all.push(pixels.at(x, y));
  return all;
}
export function mostCommon(pixels: Pixels): Rgba {
  const counts = new Map<string, { color: Rgba; count: number }>();
  for (const color of each(pixels)) {
    const entry = counts.get(hex(color)) ?? { color, count: 0 };
    entry.count += 1;
    counts.set(hex(color), entry);
  }
  return [...counts.values()].sort((a, b) => b.count - a.count)[0]!.color;
}
