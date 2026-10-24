import type { DocKey } from "../../../rpc/gen/rpc";
import { movedKey } from "../../documents/effects";
import { keyId } from "../../documents/offlineJournal";

export type Group = 0 | 1;
export interface CodeTab {
  id: string;
  key: DocKey;
  pinned: boolean;
  group: Group;
}
export interface Tabs {
  tabs: CodeTab[];
  active: [string | undefined, string | undefined];
  group: Group;
}
export type TabEvent =
  | { type: "open"; key: DocKey; pinned?: boolean; group?: Group }
  | { type: "pin"; key: DocKey }
  | { type: "activate"; id: string }
  | { type: "move"; key: DocKey; group: Group; index: number }
  | { type: "closed"; key: DocKey }
  | { type: "rename"; from: DocKey; to: DocKey };
export const emptyTabs = (): Tabs => ({ tabs: [], active: [undefined, undefined], group: 0 });
const same = (a: DocKey, b: DocKey) => keyId(a) === keyId(b);
function normalize(s: Tabs): Tabs {
  return {
    ...s,
    active: s.active.map((id, group) =>
      s.tabs.some((t) => t.id === id && t.group === group) ? id : s.tabs.find((t) => t.group === group)?.id,
    ) as Tabs["active"],
  };
}

/** One tab owns one document in a window. Split moves it instead of copying its undo history. */
export function reduceTabs(s: Tabs, e: TabEvent): Tabs {
  if (e.type === "rename")
    return { ...s, tabs: s.tabs.map((t) => ({ ...t, key: movedKey(t.key, e.from, e.to) ?? t.key })) };
  if (e.type === "activate") {
    const tab = s.tabs.find((t) => t.id === e.id);
    if (!tab) return s;
    const active = [...s.active] as Tabs["active"];
    active[tab.group] = tab.id;
    return { ...s, active, group: tab.group };
  }
  const tab = s.tabs.find((t) => same(t.key, e.key));
  if (e.type === "pin")
    return tab && !tab.pinned ? { ...s, tabs: s.tabs.map((t) => (t === tab ? { ...t, pinned: true } : t)) } : s;
  if (e.type === "closed") return normalize({ ...s, tabs: s.tabs.filter((t) => t !== tab) });
  if (e.type === "move") {
    if (!tab) return s;
    const others = s.tabs.filter((t) => t !== tab);
    const groupTabs = others.filter((t) => t.group === e.group);
    const before = groupTabs[Math.max(0, e.index)];
    const index = before ? others.indexOf(before) : others.length;
    others.splice(index, 0, { ...tab, group: e.group, pinned: true });
    const active = [...s.active] as Tabs["active"];
    active[e.group] = tab.id;
    return normalize({ tabs: others, active, group: e.group });
  }
  if (tab)
    return reduceTabs(e.pinned ? reduceTabs(s, { type: "pin", key: e.key }) : s, { type: "activate", id: tab.id });
  const group = e.group ?? s.group;
  let id = keyId(e.key);
  for (let suffix = 1; s.tabs.some((t) => t.id === id); suffix++) id = `${keyId(e.key)}:${suffix}`;
  const next = { id, key: e.key, pinned: !!e.pinned, group };
  const tabs = s.tabs.filter((t) => t.group !== group || t.pinned || e.pinned);
  tabs.push(next);
  const active = [...s.active] as Tabs["active"];
  active[group] = next.id;
  return { tabs, active, group };
}
