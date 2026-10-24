import { CompletionContext } from "@codemirror/autocomplete";
import { EditorState } from "@codemirror/state";
import { expect, test } from "vitest";
import { rank, wikiSource } from "./wikiComplete";

test("title or stem prefix first, then a folder match, then a substring", () => {
  const items = rank("da", [
    { path: "daily/2026-10-21.md", title: "" },
    { path: "data.md", title: "Data" },
    { path: "x/adam.md", title: "Adam" },
  ]);
  expect(items.map((i) => i.label)).toEqual(["data", "2026-10-21", "adam"]);
});

test("a folder-name match outranks a substring match even when the substring sorts first", () => {
  const items = rank("da", [
    { path: "x/adam.md", title: "" },
    { path: "daily/zz.md", title: "" },
  ]);
  expect(items.map((i) => i.label)).toEqual(["zz", "adam"]);
});

test("a note in the same folder as the current note ranks above other substring matches", () => {
  const items = rank(
    "an",
    [
      { path: "z/plan.md", title: "" },
      { path: "work/scan.md", title: "" },
    ],
    "work/today.md",
  );
  expect(items.map((i) => i.label)).toEqual(["scan", "plan"]);
});

test("a stem shared by two notes is completed with its vault path; folders and non-matches are left out", () => {
  const items = rank("x", [
    { path: "a/x.md", title: "" },
    { path: "b/x.md", title: "" },
    { path: "other.md", title: "" },
  ]);
  expect(items.map((i) => i.label)).toEqual(["a/x", "b/x"]);
});

test("the completion source answers only inside an open [[ and inserts the closing brackets", async () => {
  const notes = [{ path: "data.md", title: "Data" }];
  const source = wikiSource(
    () => notes,
    () => null,
  );
  const at = (doc: string) => source(new CompletionContext(EditorState.create({ doc }), doc.length, true));
  expect(await at("see da")).toBeNull();
  expect(await at("see [[x]] da")).toBeNull();
  const result = await at("see [[da");
  expect(result?.from).toBe(6);
  expect(result?.options.map((o) => [o.label, o.apply])).toEqual([["data", "data]]"]]);
});
