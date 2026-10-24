import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { RpcProvider } from "../../rpc/RpcProvider";
import { NotesWire } from "../notes/test-wire";
import { useNotifications } from "../notifications/store";
import { ActivitySettings } from "./ActivitySettings";

const native = vi.hoisted(() => ({
  invoke: vi.fn(async () => ({ consent: true, paused: false, legacyKnowledge: false })),
  listen: vi.fn(async () => () => {}),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: native.listen }));
afterEach(() => {
  cleanup();
  useNotifications.getState().clear();
  native.invoke.mockClear();
});

test("storage_failure_shows_banner_tray_and_one_notice", async () => {
  const wire = new NotesWire();
  wire.handlers["activity.status"] = () => ({ rev: "1", state: "ok", pending: 0, dropped: 0, lastError: null });
  wire.handlers["activity.rules"] = () => ({
    rev: "1",
    idleThresholdMs: 300000,
    rules: { maskAllTitles: false, excludedProcesses: [], excludedTitlePatterns: [], redactTitlePatterns: [] },
  });
  render(
    <RpcProvider client={wire.client}>
      <ActivitySettings />
    </RpcProvider>,
  );
  await screen.findByRole("button", { name: "일시중지" });
  const failed = {
    rev: "2",
    state: "storage_failed" as const,
    pending: 3,
    dropped: 0,
    lastError: "disk_full" as const,
  };
  act(() => {
    wire.emit("activity.status_changed", failed, "2");
    wire.emit("activity.status_changed", failed, "2");
  });
  expect(await screen.findByText("저장 중단 · 3건 대기")).toBeVisible();
  await waitFor(() => expect(native.invoke).toHaveBeenCalledWith("activity_tray", { storageFailed: true, pending: 3 }));
  expect(useNotifications.getState().items.filter((n) => n.kind === "error")).toHaveLength(1);
  act(() =>
    wire.emit("activity.status_changed", { rev: "3", state: "ok", pending: 0, dropped: 0, lastError: null }, "3"),
  );
  await waitFor(() => expect(screen.queryByText("저장 중단 · 3건 대기")).toBeNull());
  expect(native.invoke).toHaveBeenCalledWith("activity_tray", { storageFailed: false, pending: 0 });
});
