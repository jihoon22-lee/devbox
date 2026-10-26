import { cleanup, render, screen } from "@testing-library/react";
import { useRpcQuery } from "../../rpc/query";
import { AboutSettings } from "./AboutSettings";

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: native.listen }));
vi.mock("../../rpc/query", () => ({ useRpcQuery: vi.fn() }));
beforeEach(() => {
  native.invoke.mockReset();
  native.listen.mockReset().mockResolvedValue(vi.fn());
});
afterEach(() => {
  cleanup();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});

test("About preserves daemon information and explains Windows updates without native browser calls", () => {
  vi.mocked(useRpcQuery).mockReturnValue({
    isPending: false,
    error: null,
    data: { version: "9.9.9", instance: "fixture", dataDir: "/fixture" },
  } as unknown as ReturnType<typeof useRpcQuery>);
  render(<AboutSettings />);
  expect(screen.getByRole("heading", { name: "데몬 정보" })).toBeVisible();
  expect(screen.getByText(/데몬 9.9.9/)).toBeVisible();
  expect(screen.getByRole("heading", { name: "앱 업데이트" })).toBeVisible();
  expect(screen.getByText("앱 업데이트는 Windows 앱에서 확인할 수 있습니다.")).toBeVisible();
  expect(native.invoke).not.toHaveBeenCalled();
  expect(native.listen).not.toHaveBeenCalled();
  expect(screen.queryByText("앱 9.9.9")).not.toBeInTheDocument();
});

test("About distinguishes the native app version from the daemon version", async () => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  native.invoke.mockResolvedValue({
    enabled: true,
    disabledReason: null,
    phase: "idle",
    generation: 0,
    revision: 0,
    currentVersion: "1.2.3",
    availableVersion: null,
    receivedBytes: 0,
    totalBytes: null,
    error: null,
    nextAction: "check",
  });
  vi.mocked(useRpcQuery).mockReturnValue({
    isPending: false,
    error: null,
    data: { version: "9.9.9", instance: "fixture", dataDir: "/fixture" },
  } as unknown as ReturnType<typeof useRpcQuery>);
  render(<AboutSettings />);
  expect(await screen.findByText("앱 1.2.3")).toBeVisible();
  expect(screen.getByText(/데몬 9.9.9/)).toBeVisible();
  expect(screen.queryByText("앱 9.9.9")).not.toBeInTheDocument();
});
