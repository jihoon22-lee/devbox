import { describe, expect, it } from "vitest";
import { lineDiff } from "./lineDiff";

describe("line diff for response comparison", () => {
  it("marks added, removed and context lines with line numbers", () => {
    const file = lineDiff("a\nb\nc", "a\nB\nc\nd", { oldLabel: "이전", newLabel: "지금" });
    expect(file.oldPath).toBe("이전");
    expect(file.newPath).toBe("지금");
    expect(file.hunks[0]!.lines).toEqual([
      { kind: "ctx", old: 1, new: 1, text: "a" },
      { kind: "del", old: 2, text: "b" },
      { kind: "add", new: 2, text: "B" },
      { kind: "ctx", old: 3, new: 3, text: "c" },
      { kind: "add", new: 4, text: "d" },
    ]);
  });

  it("identical texts have no hunks and huge inputs are cut with a meta line", () => {
    expect(lineDiff("x\ny", "x\ny").hunks).toEqual([]);
    const big = Array.from({ length: 6000 }, (_, i) => String(i)).join("\n");
    const file = lineDiff(big, `${big}\nend`, { maxLines: 100 });
    const lines = file.hunks.flatMap((h) => h.lines);
    expect(lines.length).toBeLessThanOrEqual(101);
    expect(lines.at(-1)).toMatchObject({ kind: "meta" });
  });
});
