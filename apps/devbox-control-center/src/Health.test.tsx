import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
const mock = vi.hoisted(() => ({ delivery: vi.fn(), invoke: vi.fn() }));
vi.mock("./delivery", () => ({ deliveryCall: mock.delivery }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke: mock.invoke }));
import Health from "./Health";
import { fixtureDescription } from "@devbox/product-shell/api";
import catalog from "../../../apps/products.json";
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
it("records on the visible recovery route and waits for refreshed inventory", async () => {
  let complete!: () => void;
  const recorded = vi.fn(
    () =>
      new Promise<void>((resolve) => {
        complete = resolve;
      }),
  );
  mock.delivery.mockImplementation(async (header, _method, { product }) => ({
    operation: {
      provenance: {
        product: "control-center",
        component: "control-center.delivery",
        requestId: header.requestId,
        revision: catalog.catalogRevision,
      },
      outcome: { state: "succeeded" },
    },
    value: {
      recorded: true,
      nativeStoreReady: true,
      report: {
        store: { owner: product, suiteVersion: "0.9.0", busy: false, reviewRequired: false, setupSelected: true },
      },
    },
  }));
  render(
    <Health
      description={{ ...fixtureDescription("control-center"), deliveryState: "import" }}
      route="recovery"
      onRecorded={recorded}
    />,
  );
  const button = screen.getByRole("button", { name: "상태 기록" });
  fireEvent.click(button);
  await waitFor(() => expect(recorded).toHaveBeenCalledOnce());
  expect((button as HTMLButtonElement).disabled).toBe(true);
  expect(mock.delivery.mock.calls.every(([header]) => header.route === "recovery")).toBe(true);
  await act(async () => complete());
  expect((button as HTMLButtonElement).disabled).toBe(false);
});
it("offers no forbidden record button on the products route", () => {
  render(
    <Health description={{ ...fixtureDescription("control-center"), deliveryState: "import" }} route="products" />,
  );
  expect(screen.queryByRole("button", { name: "상태 기록" })).toBeNull();
});
