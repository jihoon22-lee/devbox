import type { DocKey } from "../../rpc/gen/rpc";
import { sanitizeDiagram } from "../notes/safeSvg";
import { validCodeKey } from "./editor/location";

export const canPreview = (path: string) => /\.(?:md|markdown|svg)$/i.test(path);
export function previewLink(from: DocKey, href: string): DocKey | undefined {
  let target: string;
  try {
    target = decodeURIComponent(href.split("#")[0] ?? "");
  } catch {
    return;
  }
  if (!target || /^[a-z][a-z0-9+.-]*:|^[/\\]/i.test(target) || target.includes("\\") || target.includes("?")) return;
  const parts = from.path.split("/").slice(0, -1);
  for (const part of target.split("/")) {
    if (part === "." || !part) continue;
    if (part === "..") {
      if (!parts.length) return;
      parts.pop();
    } else parts.push(part);
  }
  const key = { root: from.root, path: parts.join("/") };
  return validCodeKey(key) ? key : undefined;
}
export function svgBlob(text: string): Blob {
  const fragment = sanitizeDiagram(text, true);
  const root = fragment.querySelector("svg");
  if (!root) throw new Error("SVG를 표시할 수 없습니다.");
  return new Blob([new XMLSerializer().serializeToString(root)], { type: "image/svg+xml" });
}
