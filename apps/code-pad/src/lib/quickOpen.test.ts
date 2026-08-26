import { describe, expect, it } from "vitest";
import {
  filterQuickOpenFiles,
  flattenQuickOpenTree,
  groupQuickOpenMatches,
  scoreQuickOpen,
  splitQuickOpenPath,
} from "./quickOpen";
import type { WorkspaceFile } from "../types";

const files: WorkspaceFile[] = [
  { path: "/workspace/src/QuickOpen.tsx", relativePath: "src/QuickOpen.tsx", size: 10 },
  { path: "/workspace/src/other.ts", relativePath: "src/other.ts", size: 10 },
  { path: "/workspace/README.md", relativePath: "README.md", size: 10 },
];

describe("Quick Open matching", () => {
  it("ranks a substring above a loose subsequence", () => {
    expect(scoreQuickOpen("quick", "src/QuickOpen.tsx")).toBeGreaterThan(
      scoreQuickOpen("quick", "src/other.ts") ?? -Infinity,
    );
    expect(filterQuickOpenFiles(files, "QOp")[0]?.file.relativePath).toBe("src/QuickOpen.tsx");
  });

  it("matches case-insensitively and returns all files for an empty query", () => {
    expect(filterQuickOpenFiles(files, "readme")[0]?.file.relativePath).toBe("README.md");
    expect(filterQuickOpenFiles(files, "")).toHaveLength(files.length);
  });

  it("rejects candidates that cannot form a subsequence", () => {
    expect(scoreQuickOpen("xyz", "src/QuickOpen.tsx")).toBeNull();
  });

  it("groups nested matches by directory while keeping root files separate", () => {
    const tree = groupQuickOpenMatches(filterQuickOpenFiles([
      ...files,
      { path: "/workspace/src/components/Deep.tsx", relativePath: "src/components/Deep.tsx", size: 10 },
      { path: "/workspace/src/components/Other.tsx", relativePath: "src/components/Other.tsx", size: 10 },
    ], ""));

    expect(tree.files.map(({ file }) => file.relativePath)).toEqual(["README.md"]);
    expect(tree.directories.map(({ path }) => path)).toEqual(["src"]);
    expect(tree.directories[0]?.files.map(({ file }) => file.relativePath)).toEqual(
      expect.arrayContaining(["src/other.ts", "src/QuickOpen.tsx"]),
    );
    expect(tree.directories[0]?.directories[0]?.path).toBe("src/components");
    expect(tree.directories[0]?.directories[0]?.files.map(({ file }) => file.relativePath)).toEqual(
      expect.arrayContaining(["src/components/Deep.tsx", "src/components/Other.tsx"]),
    );
  });

  it("flattens the tree in its visual order for deterministic keyboard navigation", () => {
    const tree = groupQuickOpenMatches(
      filterQuickOpenFiles(
        [
          { path: "/workspace/a/one.ts", relativePath: "a/one.ts", size: 10 },
          { path: "/workspace/b.ts", relativePath: "b.ts", size: 10 },
          { path: "/workspace/c/two.ts", relativePath: "c/two.ts", size: 10 },
        ],
        "",
      ),
    );

    expect(flattenQuickOpenTree(tree).map(({ file }) => file.relativePath)).toEqual([
      "b.ts",
      "a/one.ts",
      "c/two.ts",
    ]);
  });

  it("splits long paths without discarding the filename or directory context", () => {
    expect(splitQuickOpenPath("packages\\editor\\src\\very-long-file-name.ts")).toEqual({
      directory: "packages/editor/src",
      name: "very-long-file-name.ts",
    });
    expect(splitQuickOpenPath("README.md")).toEqual({ directory: "", name: "README.md" });
  });
});
