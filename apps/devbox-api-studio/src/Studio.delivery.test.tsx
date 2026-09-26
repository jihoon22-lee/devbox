import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ReactNode } from "react";
import type { ShellContentProps } from "@devbox/product-shell";
import type { Description } from "@devbox/product-shell/api";
import Studio from "./Studio";
const mode = vi.hoisted(() => ({
  phase: "import" as NonNullable<Description["deliveryState"]>,
  initialize: vi.fn(async () => ({ migrated: [] as string[], failed: [] as string[] })),
}));
vi.mock("@devbox/api-studio-features/storage/initialize", () => ({ initializeStudioDocuments: mode.initialize }));
vi.mock("@devbox/product-shell", async () => {
  const { fixtureDescription } = await import("@devbox/product-shell/api");
  return {
    ProductShell: ({ renderContent }: { renderContent: (props: ShellContentProps) => ReactNode }) =>
      renderContent({
        description: { ...fixtureDescription("api-studio"), deliveryState: mode.phase },
        route: "requests",
        navigate: () => {},
        refreshContext: async () => {},
      }),
  };
});
vi.mock("@devbox/api-studio-features/requests", async () => {
  const { useEffect } = await import("react");
  return {
    default: () => {
      useEffect(() => {
        localStorage.setItem("delivery-writer", "mounted");
      }, []);
      return <div>Business request view</div>;
    },
  };
});
beforeEach(() => {
  localStorage.setItem("delivery-writer", "preserved");
  mode.initialize.mockReset().mockResolvedValue({ migrated: [], failed: [] });
});
afterEach(() => {
  cleanup();
  localStorage.clear();
});
it.each(["import", "health", "recover", "unavailable"] as const)(
  "keeps browser-storage writers unmounted during %s",
  async (phase) => {
    mode.phase = phase;
    render(<Studio />);
    await act(async () => {
      await vi.dynamicImportSettled();
    });
    expect(screen.queryByText("Business request view")).toBeNull();
    expect(localStorage.getItem("delivery-writer")).toBe("preserved");
  },
);
it("mounts the ordinary view after committed activation", async () => {
  mode.phase = "committed";
  render(<Studio />);
  await screen.findByText("Business request view");
  expect(localStorage.getItem("delivery-writer")).toBe("mounted");
});

it("waits for migration before mounting writers and shows partial failure guidance", async () => {
  mode.phase = "committed";
  let finish!: (result: { migrated: string[]; failed: string[] }) => void;
  mode.initialize.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  render(<Studio />);
  expect(screen.getByText("저장된 데이터를 준비하고 있습니다…")).toBeTruthy();
  expect(localStorage.getItem("delivery-writer")).toBe("preserved");
  await waitFor(() => expect(mode.initialize).toHaveBeenCalledTimes(1));
  await act(async () => {
    finish({ migrated: [], failed: ["collections"] });
  });
  await screen.findByText("Business request view");
  expect(screen.getByText("일부 기존 데이터를 옮기지 못했습니다. 다음 실행에 다시 시도합니다.")).toBeTruthy();
});
