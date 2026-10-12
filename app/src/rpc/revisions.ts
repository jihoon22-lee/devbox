import { type QueryClient, type QueryKey, replaceEqualDeep } from "@tanstack/react-query";

export function compareRev(a: string, b: string): number {
  const x = BigInt(a),
    y = BigInt(b);
  return x < y ? -1 : x > y ? 1 : 0;
}
function revOf(data: unknown): string | undefined {
  return data !== null && typeof data === "object" && "rev" in data && typeof data.rev === "string"
    ? data.rev
    : undefined;
}
/** Runs at Query's commit boundary, including responses racing with cache writes. */
export function shareByRevision(old: unknown, incoming: unknown): unknown {
  const before = revOf(old),
    after = revOf(incoming);
  if (before !== undefined && after !== undefined && compareRev(after, before) <= 0) return old;
  return replaceEqualDeep(old, incoming);
}
export function setIfNewer(qc: QueryClient, key: QueryKey, data: unknown): void {
  qc.setQueryData(key, (old: unknown) => shareByRevision(old, data));
}
export async function invalidateIfNewer(
  qc: QueryClient,
  keys: readonly QueryKey[],
  eventRev: string | null,
): Promise<void> {
  // Enumerate matches: keys may be prefixes for several parameterized queries.
  const matches = new Set(keys.flatMap((key) => qc.getQueryCache().findAll({ queryKey: key })));
  await Promise.all(
    [...matches].map(async (query) => {
      const cached = revOf(query.state.data);
      if (eventRev !== null && cached !== undefined && compareRev(cached, eventRev) >= 0) return;
      // invalidateQueries alone deduplicates initial pending requests, letting an older
      // response consume the invalidation. Explicit cancellation also fences late replies.
      await qc.cancelQueries({ queryKey: query.queryKey, exact: true });
      await qc.invalidateQueries({ queryKey: query.queryKey, exact: true });
    }),
  );
}
