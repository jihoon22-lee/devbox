import { describe, expect, it } from "vitest";
import { filterQuickOpenFiles, scoreQuickOpen } from "./quickOpen";
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
});
