import { type MouseEvent, type RefObject, useEffect, useMemo, useRef } from "react";
import { useMermaidBlocks } from "./useMermaidBlocks";

export interface MarkdownBodyProps {
  /** HTML already sanitized by the native markdown crate. */
  html: string;
  mermaid: readonly string[];
  docKey: string;
  idPrefix: string;
  className?: string;
  assignHeadingIds?: boolean;
  onClick?: (event: MouseEvent<HTMLDivElement>) => void;
  bodyRef?: RefObject<HTMLDivElement | null>;
}

function assignIds(root: HTMLElement): void {
  const used = new Set(Array.from(root.querySelectorAll("[id]")).map((node) => node.id));
  for (const heading of root.querySelectorAll("h1,h2,h3,h4,h5,h6")) {
    if (heading.id) continue;
    const base =
      (heading.textContent ?? "")
        .trim()
        .toLowerCase()
        .replace(/[^\p{L}\p{N}_\s-]/gu, "")
        .replace(/\s/g, "-") || "section";
    let id = base;
    let index = 1;
    while (used.has(id)) id = `${base}-${index++}`;
    heading.id = id;
    used.add(id);
  }
}

export function MarkdownBody({
  html,
  mermaid,
  docKey,
  idPrefix,
  className = "preview-body md-body",
  assignHeadingIds = false,
  onClick,
  bodyRef,
}: MarkdownBodyProps) {
  const ownRef = useRef<HTMLDivElement>(null);
  const ref = bodyRef ?? ownRef;
  const document = useMemo(() => ({ html, docKey }), [html, docKey]);
  useMermaidBlocks(ref, mermaid, docKey, document, idPrefix);
  useEffect(() => {
    if (assignHeadingIds && document.html && ref.current) assignIds(ref.current);
  }, [assignHeadingIds, document, ref]);
  // Native ammonia sanitized this HTML; Mermaid inserts only strict-mode SVG.
  return (
    <div
      key={document.docKey}
      ref={ref}
      className={className}
      onClick={onClick}
      dangerouslySetInnerHTML={{ __html: document.html }}
    />
  );
}
