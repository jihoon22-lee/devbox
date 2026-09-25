import { getMermaidRenderer } from "@devbox/mermaid-renderer";
import { type RefObject, useEffect, useRef } from "react";
import { applySvgResult } from "./svgCache";

const BADGE = '<span class="mermaid-error-badge" title="mermaid 구문 오류">⚠ 구문 오류</span>';

/** Sources bypass native HTML sanitization: the shared Mermaid renderer must
 * retain strict mode. Only its sanitized SVG enters these placeholders. */
export function useMermaidBlocks(
  container: RefObject<HTMLElement | null>,
  sources: readonly string[],
  docKey: string,
  version: unknown,
  idPrefix: string,
): void {
  const cache = useRef({ docKey, version, lastGood: new Map<string, string>() });
  const sequence = useRef(0);
  useEffect(() => {
    if (cache.current.docKey !== docKey) {
      cache.current = { docKey, version, lastGood: new Map() };
    }
    const current = cache.current;
    current.version = version;
    const root = container.current;
    if (!root) return;
    let cancelled = false;
    const blocks = Array.from(root.querySelectorAll<HTMLElement>(".mermaid-block[data-idx]"));
    if (blocks.length === 0) return;
    const canApply = (element: HTMLElement) =>
      !cancelled &&
      cache.current === current &&
      current.version === version &&
      container.current === root &&
      element.isConnected;
    const fail = (element: HTMLElement, key: string) => {
      if (canApply(element)) element.innerHTML = `${applySvgResult(current.lastGood, key, { ok: false }).svg}${BADGE}`;
    };
    void (async () => {
      let renderer: Awaited<ReturnType<typeof getMermaidRenderer>>;
      try {
        renderer = await getMermaidRenderer();
      } catch {
        for (const element of blocks) {
          const key = element.dataset.idx ?? "";
          if (sources[Number(key)] !== undefined) fail(element, key);
        }
        return;
      }
      await Promise.all(
        blocks.map(async (element) => {
          const key = element.dataset.idx ?? "";
          const source = sources[Number(key)];
          if (source === undefined) return;
          try {
            const { svg } = await renderer.render(`${idPrefix}-${key}-${sequence.current++}`, source);
            if (canApply(element)) element.innerHTML = applySvgResult(current.lastGood, key, { ok: true, svg }).svg;
          } catch {
            fail(element, key);
          }
        }),
      );
    })();
    return () => {
      cancelled = true;
    };
  }, [container, sources, docKey, version, idPrefix]);
}
