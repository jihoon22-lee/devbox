import { MarkdownBody } from "@devbox/markdown-view";
import { useEffect, useRef } from "react";
import { openExternal } from "../api";
import type { RenderedDoc } from "../types";

// 주의: securityLevel을 "loose"/"antiscript"로 낮추지 말 것. mermaid 블록 원문은
// Rust의 ammonia 살균을 거치지 않고 그대로 이 컴포넌트로 전달되어 mermaid.render()에
// 들어간다(설계 문서 "살균 계층" 절 참고). 공유 renderer가 사용하는 "strict" 모드의
// 내부 DOMPurify가 이 경로의 유일한 방어선이다 — 여기는 Tauri 웹뷰라 주입된 스크립트가 invoke()로
// read_file/write_file/delete_file은 물론 set_root로 루트를 임의 경로로 재지정할 수
// 있어, 이 값을 낮추면 곧바로 임의 파일 조작으로 이어진다.

/** `href`를 `baseRel` 문서가 위치한 디렉터리 기준으로 해석한다. (POSIX 스타일 상대 경로) */
export function resolveNoteLink(baseRel: string, href: string): { path: string; fragment?: string } | null {
  // Split URL syntax before decoding once; encoded '#' remains part of a filename.
  if (/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith("//")) return null;
  const hash = href.indexOf("#");
  const urlPath = (hash < 0 ? href : href.slice(0, hash)).split("?")[0];
  try {
    const decoded = decodeURIComponent(urlPath);
    if (decoded.includes("\\") || decoded.includes("\0")) return null;
    const stack = decoded.startsWith("/") ? [] : baseRel.split("/").slice(0, -1);
    if (!decoded) return { path: baseRel, fragment: hash < 0 ? undefined : decodeURIComponent(href.slice(hash + 1)) };
    for (const part of decoded.split("/")) {
      if (part === "" || part === ".") continue;
      if (part === "..") {
        if (!stack.length) return null;
        stack.pop();
      } else stack.push(part);
    }
    return { path: stack.join("/"), fragment: hash < 0 ? undefined : decodeURIComponent(href.slice(hash + 1)) };
  } catch {
    return null;
  }
}

interface MarkdownPreviewProps {
  doc: RenderedDoc | null;
  /** 현재 문서의 루트 상대 경로 — 상대 링크/이미지 해석 기준 */
  baseRel: string;
  /** 상대 링크 클릭 시 호출된다 (기존 openFile 재사용) */
  onNavigate: (rel: string, fragment?: string) => void;
  anchorRequest?: { fragment: string; id: number } | null;
  /** backend가 유일하게 resolve한 root-relative wikilink 전용 안전 열기 경로. */
  onNavigateWikilink?: (rel: string) => void;
}

export default function MarkdownPreview({
  doc,
  baseRel,
  onNavigate,
  onNavigateWikilink,
  anchorRequest,
}: MarkdownPreviewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!doc || !anchorRequest) return;
    const target = Array.from(containerRef.current?.querySelectorAll<HTMLElement>("[id]") ?? []).find(
      (element) => element.id === anchorRequest.fragment,
    );
    target?.scrollIntoView?.({ block: "start" });
  }, [doc, anchorRequest]);

  const handleClick = (e: React.MouseEvent<HTMLDivElement>) => {
    const anchor = (e.target as HTMLElement).closest("a[href]");
    if (!anchor) return;
    const href = anchor.getAttribute("href");
    if (!href) return;
    e.preventDefault();
    if (href.startsWith("http://") || href.startsWith("https://")) {
      void openExternal(href);
      return;
    }
    if (href.startsWith("/") && anchor.classList.contains("wikilink")) {
      (onNavigateWikilink ?? onNavigate)(href.slice(1));
    } else {
      const link = resolveNoteLink(baseRel, href);
      if (link) {
        if (link.fragment === undefined) onNavigate(link.path);
        else onNavigate(link.path, link.fragment);
      }
    }
  };

  if (!doc) {
    return <div className="preview empty">렌더링 중...</div>;
  }

  return (
    <div className="preview">
      {(doc.title || doc.tags.length > 0) && (
        <div className="preview-meta">
          {doc.title && <span className="preview-title">{doc.title}</span>}
          {doc.tags.map((t) => (
            <span key={t} className="tag">
              {t}
            </span>
          ))}
        </div>
      )}
      <MarkdownBody
        html={doc.html}
        mermaid={doc.mermaid}
        docKey={baseRel}
        idPrefix="mermaid-preview"
        assignHeadingIds
        onClick={handleClick}
        bodyRef={containerRef}
      />
    </div>
  );
}
