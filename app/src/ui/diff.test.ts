import { parseUnifiedDiff } from "./diff";

test("counts hunk content that looks like file headers and separates multiple hunks", () => {
  const file = parseUnifiedDiff(
    "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n--- original\n+++ replacement\n same\n@@ -9 +9 @@\n-last\n+next\n\\ No newline at end of file\n",
  )[0];
  expect(file!.newPath).toBe("a.txt");
  expect(file!.hunks[0]!.lines).toEqual([
    { kind: "del", old: 1, text: "-- original" },
    { kind: "add", new: 1, text: "++ replacement" },
    { kind: "ctx", old: 2, new: 2, text: "same" },
  ]);
  expect(file!.hunks[1]!.lines).toEqual([
    { kind: "del", old: 9, text: "last" },
    { kind: "add", new: 9, text: "next" },
    { kind: "meta", text: "\\ No newline at end of file" },
  ]);
});

test("decodes quoted UTF-8 octal paths, spaces, renames, addition, deletion and binary", () => {
  const files = parseUnifiedDiff(
    'diff --git "a/\\352\\260\\200 x.txt" "b/\\352\\260\\200 y.txt"\nsimilarity index 100%\nrename from "\\352\\260\\200 x.txt"\nrename to "\\352\\260\\200 y.txt"\ndiff --git a/new file b/new file\n--- /dev/null\n+++ b/new file\n@@ -0,0 +1 @@\n+x\ndiff --git a/deleted b/deleted\n--- a/deleted\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\ndiff --git a/i.png b/i.png\nGIT binary patch\n',
  );
  expect(files[0]).toMatchObject({ oldPath: "가 x.txt", newPath: "가 y.txt", hunks: [] });
  expect(files[1]).toMatchObject({ oldPath: null, newPath: "new file" });
  expect(files[1]!.hunks[0]!.lines[0]).toEqual({ kind: "add", new: 1, text: "x" });
  expect(files[2]).toMatchObject({ oldPath: "deleted", newPath: null });
  expect(files[3]!.binary).toBe(true);
});
