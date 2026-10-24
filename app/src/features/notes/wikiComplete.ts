import { autocompletion, type CompletionContext, type CompletionResult } from "@codemirror/autocomplete";
import type { Extension } from "@codemirror/state";

export interface WikiCandidate {
  path: string;
  title: string;
}
export interface Ranked extends WikiCandidate {
  /** The link text: the file stem, or the vault path without `.md` when two notes share the stem. */
  label: string;
}

const MAX_OPTIONS = 50;
const stemOf = (path: string) => (path.split("/").pop() ?? path).replace(/\.md$/i, "");
const folderOf = (path: string) => path.split("/").slice(0, -1).join("/");

/**
 * Orders `[[` candidates: a title or stem that starts with the query first, then notes whose
 * folder name starts with it or that sit in the current note's folder, then any other substring
 * match (title, stem or path). Ties keep alphabetical order of the label.
 */
export function rank(query: string, items: WikiCandidate[], from?: string | null): Ranked[] {
  const q = query.trim().toLowerCase();
  const here = from ? folderOf(from).toLowerCase() : null;
  const stems = new Map<string, number>();
  for (const item of items) {
    const stem = stemOf(item.path).toLowerCase();
    stems.set(stem, (stems.get(stem) ?? 0) + 1);
  }
  const scored: { tier: number; item: Ranked }[] = [];
  for (const item of items) {
    const stem = stemOf(item.path);
    const title = item.title.trim().toLowerCase();
    const folder = folderOf(item.path).toLowerCase();
    const lower = stem.toLowerCase();
    const label = (stems.get(lower) ?? 0) > 1 ? item.path.replace(/\.md$/i, "") : stem;
    const anywhere = lower.includes(q) || title.includes(q) || item.path.toLowerCase().includes(q);
    let tier: number;
    if (lower.startsWith(q) || (title && title.startsWith(q))) tier = 0;
    else if (folder.split("/").some((part) => part && part.startsWith(q))) tier = 1;
    else if (anywhere && here !== null && folder === here) tier = 1;
    else if (anywhere) tier = 2;
    else continue;
    scored.push({ tier, item: { ...item, label } });
  }
  scored.sort((a, b) => a.tier - b.tier || a.item.label.localeCompare(b.item.label));
  return scored.slice(0, MAX_OPTIONS).map((entry) => entry.item);
}

/** Completes `[[query` from the note list; choosing an option writes `label]]`. */
export function wikiSource(items: () => WikiCandidate[], from: () => string | null) {
  return async (context: CompletionContext): Promise<CompletionResult | null> => {
    const open = context.matchBefore(/\[\[[^\]|#\n[]*$/);
    if (!open) return null;
    const query = open.text.slice(2);
    const options = rank(query, items(), from()).map((item) => ({
      label: item.label,
      detail: item.title && item.title !== item.label ? item.title : folderOf(item.path) || undefined,
      apply: `${item.label}]]`,
      type: "text",
    }));
    return { from: open.from + 2, options, filter: false };
  };
}

export function wikiCompletion(items: () => WikiCandidate[], from: () => string | null): Extension {
  return autocompletion({ override: [wikiSource(items, from)], activateOnTyping: true });
}
