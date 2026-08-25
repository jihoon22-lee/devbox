import { describe, expect, it } from "vitest";
import type { RepoOpenTarget } from "../api";
import { buildRepositoryContextMenu } from "./contextMenu";

const targets: RepoOpenTarget[] = [
  { id: "code-pad", displayName: "Code Pad", payloadKind: "workspace" },
  { id: "wsl-desktop", displayName: "WSL Desktop", payloadKind: "path" },
];

describe("Repo Manager repository context menu contract", () => {
  it("설계의 exact repository 항목과 catalog submenu를 만든다", () => {
    const items = buildRepositoryContextMenu(targets, false);
    expect(items.map((item) => item.type === "separator" ? "separator" : item.label)).toEqual([
      "다른 앱으로 열기",
      "worktree 생성",
      "경로 복사",
      "탐색기에서 열기",
    ]);
    const open = items[0];
    expect(open.type).toBe("submenu");
    if (open.type !== "submenu") throw new Error("open-in submenu missing");
    expect(open.items.map((item) => item.type === "item" ? [item.id, item.label] : null)).toEqual([
      ["open-in:code-pad", "Code Pad"],
      ["open-in:wsl-desktop", "WSL Desktop"],
    ]);
    expect(items.some((item) => item.type !== "separator" && item.id.includes("remove"))).toBe(false);
  });

  it("target 확인 전과 설치 target 부재는 submenu를 fail-closed로 둔다", () => {
    for (const current of [null, []] as const) {
      const open = buildRepositoryContextMenu(current, false)[0];
      expect(open.type === "submenu" && open.disabled).toBe(true);
    }
  });

  it("작업 진행 중에는 모든 repository action을 비활성화한다", () => {
    const items = buildRepositoryContextMenu(targets, true);
    expect(items.every((item) => item.type !== "separator" && item.disabled)).toBe(true);
  });
});
