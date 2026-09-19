import { describe, expect, it, vi } from "vitest";
import type { WorkspaceDefinition } from "../types";
import {
  orderWorkspacePanes,
  RESTORE_START_CONCURRENCY,
  runWithConcurrencyLimit,
} from "./workspaceRestore";

function sizing(count: number) {
  return { columns: Array.from({ length: count }, () => 1 / count), rows: [1] };
}

describe("workspace restore scheduling", () => {
  it("selects the requested active pane before definition order", () => {
    const workspace: WorkspaceDefinition = {
      tabs: [{
        id: "tab-1",
        title: "dev",
        customTitle: false,
        layout: "cols",
        paneKeys: ["pane-1", "pane-2"],
        sizing: sizing(2),
      }],
      panes: [
        { key: "pane-1", distro: "Ubuntu", cwd: null, startCommand: null, multiplexer: "native" },
        { key: "pane-2", distro: "Ubuntu", cwd: null, startCommand: null, multiplexer: "zellij" },
      ],
      activeTabId: "tab-1",
      activePaneKey: "pane-2",
    };

    const plan = orderWorkspacePanes(workspace);
    expect(plan.active.key).toBe("pane-2");
    expect(plan.remaining.map((pane) => pane.key)).toEqual(["pane-1"]);
  });

  it("never exceeds the restore concurrency bound", async () => {
    let active = 0;
    let peak = 0;
    const releases: Array<() => void> = [];
    const running = runWithConcurrencyLimit(
      [1, 2, 3, 4],
      RESTORE_START_CONCURRENCY,
      async () => {
        active += 1;
        peak = Math.max(peak, active);
        await new Promise<void>((resolve) => releases.push(resolve));
        active -= 1;
      },
    );

    await vi.waitFor(() => expect(releases).toHaveLength(2));
    releases.shift()?.();
    await vi.waitFor(() => expect(releases).toHaveLength(2));
    releases.shift()?.();
    await vi.waitFor(() => expect(releases).toHaveLength(2));
    releases.splice(0).forEach((release) => release());
    await running;

    expect(peak).toBe(2);
  });
});
