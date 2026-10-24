import { type KeyboardEvent, type MouseEvent, useEffect, useLayoutEffect, useRef, useState } from "react";
import { RpcError } from "../../rpc/client";
import type { RenderedNote } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { useRpcSession } from "../../rpc/RpcProvider";
import { openExternal } from "../../shell/external";
import { NETWORK_BLOCKED, networkFreeDiagram } from "./mermaidPolicy";
import s from "./notes.module.css";
import { sanitizePreview } from "./safeHtml";
import { sanitizeDiagram } from "./safeSvg";

const DEBOUNCE_MS = 250;
const decode = (value: string) => {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
};
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

/**
 * Rendered Markdown of the current buffer (`notes.render` with the note path, so relative
 * images resolve from its folder). Latest request wins and results are not cached: a render
 * can be several MiB. Links never navigate the app window.
 */
export function Preview({ path, text, onOpenNote }: { path: string; text: string; onOpenNote(path: string): void }) {
  const { client, state } = useRpcSession();
  const host = useRef<HTMLDivElement>(null);
  const [result, setResult] = useState<RenderedNote>();
  const [error, setError] = useState<string>();
  useEffect(() => {
    if (state !== "ready") return;
    let live = true;
    const timer = setTimeout(() => {
      client.call("notes.render", { path, text }).then(
        (rendered) => {
          if (!live) return;
          setResult(rendered);
          setError(undefined);
        },
        (failure) => {
          if (!live) return;
          const e = failure instanceof RpcError ? failure : new RpcError("internal");
          setError(messageFor(e.code, e.detail).title);
        },
      );
    }, DEBOUNCE_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [client, state, path, text]);
  useLayoutEffect(() => {
    const el = host.current;
    if (!el || !result) return;
    el.replaceChildren(sanitizePreview(result.html));
    let live = true;
    void drawMermaid(el, result.mermaid, () => live);
    return () => {
      live = false;
    };
  }, [result]);
  // Links carry `data-href` only (see `sanitizePreview`): web links open outside, a plain relative
  // `.md` link opens that note, and anything else (`/\host`, `//host`, other schemes) does nothing.
  const follow = (link: Element) => {
    const href = link.getAttribute("data-href") ?? "";
    if (/^https?:\/\//i.test(href)) {
      void openExternal(href);
      return;
    }
    const target = decode(href.split("#")[0] ?? "");
    if (!target || /^[a-z][a-z0-9+.-]*:|^[/\\]/i.test(target) || !/\.md$/i.test(target)) return;
    void client.call("notes.resolve", { target, kind: "md", from: path }).then(
      ({ paths, ambiguous }) => {
        if (!ambiguous && paths[0]) onOpenNote(paths[0]);
      },
      () => {},
    );
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
