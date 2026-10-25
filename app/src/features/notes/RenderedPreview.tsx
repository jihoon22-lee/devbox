import { type KeyboardEvent, type MouseEvent, useLayoutEffect, useRef } from "react";
import type { RenderedNote } from "../../rpc/gen/rpc";
import { openExternal } from "../../shell/external";
import { NETWORK_BLOCKED, networkFreeDiagram } from "./mermaidPolicy";
import s from "./notes.module.css";
import { sanitizePreview } from "./safeHtml";
import { sanitizeDiagram } from "./safeSvg";

let renders = 0;

/** Draws each mermaid block from its source list. Strict mode: no HTML labels, no click handlers. */
async function drawMermaid(host: HTMLElement, sources: string[], current: () => boolean) {
  const blocks = [...host.querySelectorAll<HTMLElement>(".mermaid-block")].filter(
    (el) => el.dataset.reason !== "budget" && (sources[Number(el.dataset.idx)] ?? "").trim() !== "",
  );
  if (!blocks.length) return;
  const { default: mermaid } = await import("mermaid");
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: "strict",
    htmlLabels: false,
    flowchart: { htmlLabels: false },
    layout: "dagre",
    theme: "default",
    look: "classic",
  });
  for (const el of blocks) {
    if (!current()) return;
    const source = sources[Number(el.dataset.idx)] as string;
    if (!networkFreeDiagram(source)) {
      if (current()) el.textContent = NETWORK_BLOCKED;
      continue;
    }
    try {
      renders += 1;
      const { svg } = await mermaid.render(`devbox-mermaid-${renders}`, source);
      if (current()) el.replaceChildren(sanitizeDiagram(svg));
    } catch {
      if (current()) el.textContent = "다이어그램을 그리지 못했습니다. 문법을 확인해 주세요.";
    }
  }
}

/** Shared inert HTML/Mermaid renderer; callers own data loading and relative links. */
export function RenderedPreview({
  result,
  error,
  onLink,
}: {
  result?: RenderedNote;
  error?: string;
  onLink(href: string): void;
}) {
  const host = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const el = host.current;
    if (!el) return;
    if (!result) {
      el.replaceChildren();
      return;
    }
    el.replaceChildren(sanitizePreview(result.html));
    let live = true;
    void drawMermaid(el, result.mermaid, () => live);
    return () => {
      live = false;
    };
  }, [result]);
  const follow = (link: Element) => {
    const href = link.getAttribute("data-href") ?? "";
    if (/^https?:\/\//i.test(href)) {
      void openExternal(href);
      return;
    }
    onLink(href);
  };
  const onClick = (event: MouseEvent<HTMLDivElement>) => {
    const link = (event.target as Element).closest?.("a");
    if (!link) return;
    event.preventDefault();
    follow(link);
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const link = (event.target as Element).closest?.("a[data-href]");
    if (!link || event.key !== "Enter") return;
    event.preventDefault();
    follow(link);
  };
  return (
    <section className={s.preview} aria-label="미리보기">
      {error && (
        <p role="alert" className={s.previewError}>
          {error}
        </p>
      )}
      <div ref={host} data-preview="" className={s.previewBody} onClick={onClick} onKeyDown={onKeyDown} />
    </section>
  );
}
