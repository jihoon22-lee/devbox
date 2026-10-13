import { describe, expect, test } from "vitest";
import {
  closePane,
  emptyLayout,
  openTab,
  prune,
  sanitizeProjects,
  sessionsIn,
  setRatio,
  splitActive,
  titleOf,
} from "./store";

describe("terminal layouts", () => {
  test("open, nest splits, close and prune while repairing active tabs", () => {
    let layout = openTab(emptyLayout(), "s1");
    layout = splitActive(layout, "s1", "s2", "row");
    layout = splitActive(layout, "s2", "s3", "column");
    expect(sessionsIn(layout.tabs[0]!.root)).toEqual(["s1", "s2", "s3"]);
    layout = closePane(layout, "s2");
    expect(sessionsIn(layout.tabs[0]!.root)).toEqual(["s1", "s3"]);
    layout = openTab(layout, "s4");
    layout = prune(layout, new Set(["s1", "s4"]));
    expect(layout.tabs.map((tab) => sessionsIn(tab.root))).toEqual([["s1"], ["s4"]]);
    layout = closePane(layout, "s4");
    expect(layout.active).toBe(layout.tabs[0]!.id);
  });
  test("duplicates focus their tab and a disappeared split target keeps the new session", () => {
    let layout = openTab(openTab(emptyLayout(), "s1"), "s2");
    layout = splitActive(layout, "s1", "s2", "row");
    expect(layout.tabs).toHaveLength(2);
    expect(layout.active).toBe(layout.tabs[1]!.id);
    layout = splitActive(layout, "missing", "s3", "column");
    expect(sessionsIn(layout.tabs[2]!.root)).toEqual(["s3"]);
  });
  test("nested ratios clamp and titles count panes", () => {
    let layout = splitActive(openTab(emptyLayout(), "s1"), "s1", "s2", "row");
    layout = splitActive(layout, "s2", "s3", "column");
    layout = setRatio(layout, layout.tabs[0]!.id, ["b"], 20);
    expect(layout.tabs[0]!.root).toMatchObject({ ratio: 0.5, b: { ratio: 0.85 } });
    expect(titleOf(layout.tabs[0]!, [{ id: "s1", title: "app" }])).toBe("app +2");
  });
  test("untrusted persisted layouts discard malformed, duplicate and excessively deep trees", () => {
    const leaf = { kind: "leaf", session: "s1" };
    let deep: unknown = leaf;
    for (let index = 0; index < 40; index++) deep = { kind: "split", dir: "row", ratio: 0.5, a: deep, b: leaf };
    const projects = sanitizeProjects({
      p: {
        active: "missing",
        tabs: [
          { id: "one", root: { kind: "split", dir: "row", ratio: 100, a: leaf, b: { kind: "leaf", session: "s2" } } },
          { id: "two", root: leaf },
          { id: "bad", root: deep },
        ],
      },
      broken: null,
    });
    expect(projects.p!.tabs).toHaveLength(1);
    expect(projects.p!.active).toBe("one");
    expect(projects.p!.tabs[0]!.root).toMatchObject({ ratio: 0.85 });
    expect(projects.broken).toBeUndefined();
  });
});
