import type { TreeItem } from "../../rpc/gen/rpc";
import { fileName, folderOf, stem } from "./notePaths";

export interface TreeRow {
  path: string;
  dir: boolean;
  depth: number;
  /** The note title, or the file stem when it has none; folders show their name. */
  label: string;
}

/** Flattens the vault listing into visible rows: folders first, then notes, each sorted by name. */
export function treeRows(items: TreeItem[], collapsed: ReadonlySet<string>): TreeRow[] {
  const children = new Map<string, TreeItem[]>();
  const seen = new Set<string>();
  const add = (item: TreeItem) => {
    if (seen.has(item.path)) return;
    seen.add(item.path);
    const parent = folderOf(item.path);
    if (parent && !seen.has(parent)) add({ path: parent, dir: true, title: "" });
    children.set(parent, [...(children.get(parent) ?? []), item]);
  };
  for (const item of items) add(item);
  const rows: TreeRow[] = [];
  const walk = (folder: string, depth: number) => {
    const list = [...(children.get(folder) ?? [])].sort(
      (a, b) => Number(b.dir) - Number(a.dir) || fileName(a.path).localeCompare(fileName(b.path)),
    );
    for (const item of list) {
      const label = item.dir ? fileName(item.path) : item.title.trim() || stem(item.path);
      rows.push({ path: item.path, dir: item.dir, depth, label });
      if (item.dir && !collapsed.has(item.path)) walk(item.path, depth + 1);
    }
  };
  walk("", 0);
  return rows;
}
