export interface TreeEntry {
  path: string;
  is_dir: boolean;
}

export interface SearchResult {
  path: string;
  title: string;
}

/** `render_markdown` 커맨드의 응답. Rust `RenderedDoc`과 1:1. */
export interface RenderedDoc {
  title: string | null;
  tags: string[];
  /** 살균 완료된 HTML. mermaid 블록은 placeholder로 치환됨. */
  html: string;
  /** placeholder `data-idx` 순서의 mermaid 원문. */
  mermaid: string[];
}
