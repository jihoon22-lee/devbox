/**
 * Mermaid strict mode is an XSS boundary, not a network boundary: image shapes fetch while
 * rendering. Keep notes diagrams text/vector-only. Reject resource-capable syntax before
 * Mermaid sees it, including diagram config (which could re-enable HTML labels), metadata
 * image shapes, HTML/encoded HTML, Markdown images and CSS/escaped CSS. This deliberately
 * also excludes harmless uses of those constructs. Only flowchart/graph and sequenceDiagram
 * have audited render-time resource paths in the pinned Mermaid version. Other diagram types
 * fail closed until audited. Sequence properties.icon uses SVG image (not new Image).
 */
export function networkFreeDiagram(source: string): boolean {
  const header = source
    .split("\n")
    .map((line) => line.trim())
    .find((line) => line && !line.startsWith("%%"));
  if (!header || !/^(?:flowchart|graph|sequenceDiagram)\b/.test(header)) return false;
  return !(
    /@\s*\{|%%\s*\{|^\s*---|<\s*[a-z!/]|!\s*\[|&(?:#\w+|\w+);|#(?:\d+|x[\da-f]+|[a-z]+);/im.test(source) ||
    /\b(?:style|classDef|linkStyle|properties|details|links|link|click)\b|url\s*\(|@import|\\/i.test(source)
  );
}

export const NETWORK_BLOCKED = "외부 리소스 차단을 위해 이 다이어그램 종류나 문법은 표시하지 않습니다.";
