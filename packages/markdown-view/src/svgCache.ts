export interface PreviewSvgResult {
  svg: string;
  hasError: boolean;
}

/**
 * Keeps a previously valid diagram visible when a later edit is malformed.
 * The cache is per preview block and intentionally contains only Mermaid's
 * generated SVG, never source HTML from the document.
 */
export function applySvgResult(
  cache: Map<string, string>,
  key: string,
  result: { ok: true; svg: string } | { ok: false },
): PreviewSvgResult {
  if (result.ok) {
    cache.set(key, result.svg);
    return { svg: result.svg, hasError: false };
  }
  return { svg: cache.get(key) ?? "", hasError: true };
}
