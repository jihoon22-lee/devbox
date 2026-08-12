import mermaid from "mermaid";
import { useEffect, useRef } from "react";
import { openExternal } from "../api";
import type { RenderedDoc } from "../types";

// 주의: securityLevel을 "loose"/"antiscript"로 낮추지 말 것. mermaid 블록 원문은
// Rust의 ammonia 살균을 거치지 않고 그대로 이 컴포넌트로 전달되어 mermaid.render()에
// 들어간다(설계 문서 "살균 계층" 절 참고). "strict"(mermaid 기본값)의 내부 DOMPurify가
// 이 경로의 유일한 방어선이다 — 여기는 Tauri 웹뷰라 주입된 스크립트가 invoke()로
// read_file/write_file/delete_file은 물론 set_root로 루트를 임의 경로로 재지정할 수
// 있어, 이 값을 낮추면 곧바로 임의 파일 조작으로 이어진다.
mermaid.initialize({ startOnLoad: false, theme: "dark", securityLevel: "strict" });

/** `href`를 `baseRel` 문서가 위치한 디렉터리 기준으로 해석한다. (POSIX 스타일 상대 경로) */
function resolveRelativePath(baseRel: string, href: string): string {
  const stack = baseRel.split("/").slice(0, -1);
  for (const part of href.split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") stack.pop();
    else stack.push(part);
  }
  return stack.join("/");
}

interface MarkdownPreviewProps {
  doc: RenderedDoc | null;
  /** 현재 문서의 루트 상대 경로 — 상대 링크/이미지 해석 기준 */
  baseRel: string;
  /** 상대 링크 클릭 시 호출된다 (기존 openFile 재사용) */
  onNavigate: (rel: string) => void;
}

export default function MarkdownPreview({ doc, baseRel, onNavigate }: MarkdownPreviewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  // 인덱스별 마지막 성공 SVG. mermaid 문법 오류가 나도 지우지 않고 유지한다(설계 결정 6).
  const lastGoodSvg = useRef<Map<number, string>>(new Map());
  const renderSeq = useRef(0);

  // 문서를 전환하면 이전 문서의 인덱스 기준 SVG는 더 이상 의미가 없다.
  useEffect(() => {
    lastGoodSvg.current.clear();
  }, [baseRel]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container || !doc) return;
    const blocks = container.querySelectorAll<HTMLDivElement>(".mermaid-block[data-idx]");
    blocks.forEach((el) => {
      const idx = Number(el.dataset.idx);
      const src = doc.mermaid[idx];
      if (src === undefined) return;
      const svgId = `mermaid-preview-${idx}-${renderSeq.current++}`;
      mermaid
        .render(svgId, src)
        .then(({ svg }) => {
          lastGoodSvg.current.set(idx, svg);
          el.innerHTML = svg;
        })
        .catch(() => {
          const cached = lastGoodSvg.current.get(idx);
          el.innerHTML = `${cached ?? ""}<span class="mermaid-error-badge" title="mermaid 구문 오류">⚠ 구문 오류</span>`;
        });
    });
  }, [doc]);

  const handleClick = (e: React.MouseEvent<HTMLDivElement>) => {
    const anchor = (e.target as HTMLElement).closest("a[href]");
    if (!anchor) return;
    const href = anchor.getAttribute("href");
    if (!href || href.startsWith("#")) return;
    e.preventDefault();
    if (href.startsWith("http://") || href.startsWith("https://")) {
      void openExternal(href);
      return;
    }
    onNavigate(resolveRelativePath(baseRel, href));
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
      <div
        ref={containerRef}
        className="preview-body md-body"
        onClick={handleClick}
        // html은 Rust 쪽 ammonia로 이미 살균되어 도착한다.
        dangerouslySetInnerHTML={{ __html: doc.html }}
      />
    </div>
  );
}
