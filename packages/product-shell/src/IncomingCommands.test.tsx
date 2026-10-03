import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
const tauri = vi.hoisted(() => ({ invoke: vi.fn(), refresh: undefined as undefined | (() => void) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke, isTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_event, callback) => {
    tauri.refresh = callback;
    return () => {
      tauri.refresh = undefined;
    };
  }),
}));
import IncomingCommands from "./IncomingCommands";
import { fixtureDescription } from "./api";
import catalog from "../../../apps/products.json";
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
it("clears a recovered pending read while preserving an unrelated decision failure", async () => {
  let failRead = false;
  const description = fixtureDescription("workspace");
  const review = {
    operationId: "op",
    revision: "rev",
    commandRevision: "a".repeat(64),
    route: "overview",
    context: null,
    target: { kind: "route", route: "overview" },
    label: "합성 요청",
  };
  tauri.invoke.mockImplementation(async (_command, { request: { header, method } }) => {
    if (method.kind === "decide" || failRead) throw new Error("owned failure");
    return {
      operation: {
        provenance: {
          product: "workspace",
          component: "workspace.commands",
          requestId: header.requestId,
          revision: catalog.catalogRevision,
        },
        outcome: { state: "succeeded" },
      },
      value: [review],
    };
  });
  render(<IncomingCommands description={description} route="overview" navigate={vi.fn()} onReview={vi.fn()} />);
  fireEvent.click(await screen.findByRole("button", { name: "거절" }));
  await screen.findByText(/요청이 변경되었거나 만료/);
  failRead = true;
  act(() => tauri.refresh?.());
  await screen.findByText("다른 제품의 열기 요청을 확인하지 못했습니다.");
  failRead = false;
  act(() => tauri.refresh?.());
  await waitFor(() => expect(screen.queryByText("다른 제품의 열기 요청을 확인하지 못했습니다.")).toBeNull());
  expect(screen.getByText(/요청이 변경되었거나 만료/)).toBeTruthy();
});
it("does not subscribe or read pending work while stores are unprepared", async () => {
  render(
    <IncomingCommands
      description={{ ...fixtureDescription("workspace"), deliveryState: "import" }}
      route="overview"
      navigate={vi.fn()}
      onReview={vi.fn()}
    />,
  );
  await act(async () => {});
  expect(tauri.invoke).not.toHaveBeenCalled();
  expect(tauri.refresh).toBeUndefined();
});
