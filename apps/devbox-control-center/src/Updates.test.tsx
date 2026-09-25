import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { fixtureDescription } from "@devbox/product-shell/api";
import catalog from "../../../apps/products.json";
import Updates from "./Updates";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: () => true }));
vi.mock("@devbox/product-shell/api", async (original) => ({
  ...(await original<typeof import("@devbox/product-shell/api")>()),
  nativeMode: true,
}));
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});
const id = "a".repeat(64);
function respond(foreign = false) {
  vi.mocked(invoke).mockImplementation(async (_command, args) => {
    const { request } = args as { request: { header: { requestId: string }; method: string } };
    return {
      operation: {
        provenance: {
          product: foreign ? "workspace" : "control-center",
          component: "control-center.delivery",
          requestId: request.header.requestId,
          revision: catalog.catalogRevision,
        },
        outcome: { state: "succeeded" },
      },
      value:
        request.method === "launch_suite_update"
          ? { accepted: true }
          : {
              available: true,
              id,
              version: "0.9.0",
              bytes: 1024,
              received: request.method === "check_suite_update" ? 0 : 1024,
              state: request.method === "check_suite_update" ? "reviewed" : "ready",
            },
    } as never;
  });
}
it("checks and downloads only on request and requires a separate install confirmation", async () => {
  respond();
  render(<Updates description={fixtureDescription("control-center")} route="updates" />);
  expect(invoke).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "업데이트 확인" }));
  fireEvent.click(await screen.findByRole("button", { name: "검토한 설치 파일 다운로드" }));
  const launch = await screen.findByRole("button", { name: "Control Center를 닫고 설치 프로그램 열기" });
  expect((launch as HTMLButtonElement).disabled).toBe(true);
  expect(invoke).toHaveBeenCalledTimes(2);
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(launch);
  await screen.findByRole("status");
  expect(invoke).toHaveBeenLastCalledWith(
    "plugin:control-center|execute",
    expect.objectContaining({ request: expect.objectContaining({ method: "launch_suite_update", args: { id } }) }),
  );
});
it("rejects foreign native provenance before exposing executable actions", async () => {
  respond(true);
  render(<Updates description={fixtureDescription("control-center")} route="updates" />);
  fireEvent.click(screen.getByRole("button", { name: "업데이트 확인" }));
  await screen.findByRole("alert");
  expect(screen.queryByRole("button", { name: "검토한 설치 파일 다운로드" })).toBeNull();
  await waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
});
