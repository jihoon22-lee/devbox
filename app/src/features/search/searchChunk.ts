import type { Scope, SearchChunk, SearchHit } from "../../rpc/gen/rpc";

const scopes = new Set<Scope>(["notes", "files", "code"]);
export function readSearchChunk(line: string): SearchChunk {
  const p: unknown = JSON.parse(line);
  if (!p || typeof p !== "object") throw Error("invalid search chunk");
  const chunk = p as SearchChunk;
  if (
    !scopes.has(chunk.scope) ||
    typeof chunk.rev !== "string" ||
    !/^\d+$/.test(chunk.rev) ||
    typeof chunk.done !== "boolean" ||
    typeof chunk.truncated !== "boolean" ||
    !Array.isArray(chunk.items) ||
    chunk.items.length > 50
  )
    throw Error("invalid search chunk");
  for (const hit of chunk.items) {
    if (
      !hit ||
      typeof hit.root !== "string" ||
      typeof hit.path !== "string" ||
      (hit.title !== null && typeof hit.title !== "string") ||
      (hit.line !== null && (!Number.isSafeInteger(hit.line) || hit.line < 1)) ||
      typeof hit.snippet !== "string" ||
      !Array.isArray(hit.rangesUtf16)
    )
      throw Error("invalid search result");
    for (const range of hit.rangesUtf16)
      if (
        !Array.isArray(range) ||
        range.length !== 2 ||
        !range.every(Number.isSafeInteger) ||
        range[0] < 0 ||
        range[1] < range[0] ||
        range[1] > hit.snippet.length
      )
        throw Error("invalid search range");
  }
  return chunk;
}
export const hitKey = (hit: SearchHit) => JSON.stringify([hit.root, hit.path, hit.line]);
