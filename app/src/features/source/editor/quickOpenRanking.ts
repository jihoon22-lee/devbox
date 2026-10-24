import type { DocKey } from "../../../rpc/gen/rpc";
import { keyId } from "../../documents/offlineJournal";

export function rankFiles(text: string, files: DocKey[], recent: DocKey[], root: string): DocKey[] {
  const query = text.trim().toLocaleLowerCase();
  const seen = new Map<string, DocKey>();
  for (const file of [...recent, ...files]) if (file.root === root) seen.set(keyId(file), file);
  const ranks = new Map(recent.map((key, index) => [keyId(key), index]));
  const score = (key: DocKey) => {
    const path = key.path.toLocaleLowerCase();
    const basename = path.split("/").at(-1) ?? path;
    const at = path.indexOf(query);
    if (at < 0) return -1;
    return (
      (basename.startsWith(query) ? 1000 : basename.includes(query) ? 500 : 0) +
      (ranks.has(keyId(key)) ? 200 - Math.min(ranks.get(keyId(key)) ?? 0, 100) : 0) -
      at
    );
  };
  if (!query)
    return [...seen.values()]
      .filter((key) => ranks.has(keyId(key)))
      .sort((a, b) => (ranks.get(keyId(a)) ?? 0) - (ranks.get(keyId(b)) ?? 0))
      .slice(0, 50);
  return [...seen.values()]
    .filter((key) => score(key) >= 0)
    .sort((a, b) => score(b) - score(a) || a.path.localeCompare(b.path))
    .slice(0, 100);
}
