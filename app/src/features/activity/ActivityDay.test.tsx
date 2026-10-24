import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { RpcProvider } from "../../rpc/RpcProvider";
import { NotesWire } from "../notes/test-wire";
import { ActivityView } from "./ActivityView";

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: native.listen }));
afterEach(cleanup);

test("late_day_response_does_not_override_tracking_status", async () => {
  const wire = new NotesWire();
  let day!: (v: unknown) => void;
  wire.handlers["activity.day"] = () =>
    new Promise((r) => {
      day = r;
    });
  wire.handlers["activity.status"] = () => ({ rev: "1", state: "ok", pending: 0, dropped: 0, lastError: null });
  native.invoke.mockImplementation(async (method: string) => ({
    consent: true,
    paused: method === "activity_state",
    legacyKnowledge: false,
  }));
  render(
    <RpcProvider client={wire.client}>
      <ActivityView />
    </RpcProvider>,
  );
  await userEvent.click(await screen.findByRole("button", { name: "다시 시작" }));
  expect(await screen.findByRole("button", { name: "일시중지" })).toBeVisible();
  await act(async () =>
    day({ rev: "99", date: "2026-10-24", totalMs: 60000, apps: [], projects: [], titles: [], hours: [] }),
  );
  expect(screen.getByRole("button", { name: "일시중지" })).toBeVisible();
});
