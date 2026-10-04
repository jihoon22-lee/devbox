import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { fixtureDescription } from "@devbox/product-shell/api";
import catalog from "../../../apps/products.json";
import Restore from "./Restore";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: () => true }));
vi.mock("@devbox/product-shell/api", async (original) => ({
  ...(await original<typeof import("@devbox/product-shell/api")>()),
  nativeMode: true,
}));
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});
const checkpoint = "fa2c49ba-1368-4c79-aafb-d7a6c7ef6c14";
function respond(
  activeOperation: string | null = null,
  installation?: { phase: string; committed: boolean; recordedOwners: number; clean: boolean; freshHealth: boolean },
) {
  vi.mocked(invoke).mockImplementation(async (_command, args) => {
    const { request } = args as { request: { header: { requestId: string }; method: string } };
    return {
      operation: {
        provenance: {
          product: "control-center",
          component: "control-center.delivery",
          requestId: request.header.requestId,
          revision: catalog.catalogRevision,
        },
        outcome: { state: "succeeded" },
      },
      value:
        request.method === "restore_inventory"
          ? {
              installation,
              checkpoints: [{ id: checkpoint, bytes: 512, files: 2 }],
              operations: activeOperation ? [{ id: activeOperation, phase: "health" }] : [],
              activeOperation,
            }
          : { accepted: true },
    } as never;
  });
}
it("requires explicit review before closing the shell and sends only the selected checkpoint id", async () => {
  respond();
  render(<Restore description={fixtureDescription("control-center")} route="recovery" />);
  fireEvent.click(await screen.findByRole("button", { name: "이 보존본으로 복원" }));
  const execute = screen.getByRole("button", { name: "Control Center를 닫고 실행" });
  expect((execute as HTMLButtonElement).disabled).toBe(true);
  expect(invoke).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(execute);
  await screen.findByText(/복구 도우미를 시작했습니다/);
  expect(invoke).toHaveBeenLastCalledWith(
    "plugin:control-center|delivery",
    expect.objectContaining({
      request: expect.objectContaining({ method: "restore_action", args: { action: "restore", id: checkpoint } }),
    }),
  );
  expect(invoke).toHaveBeenCalledTimes(2);
});
it("keeps a pending restore visible and prevents starting a second snapshot or restore", async () => {
  respond(checkpoint);
  render(<Restore description={fixtureDescription("control-center")} route="recovery" />);
  expect(((await screen.findByRole("button", { name: "이 보존본으로 복원" })) as HTMLButtonElement).disabled).toBe(
    true,
  );
  expect((screen.getByRole("button", { name: "현재 데이터 보존" }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "원본으로 복귀" }));
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(screen.getByRole("button", { name: "Control Center를 닫고 실행" }));
  await waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));
  expect(invoke).toHaveBeenLastCalledWith(
    "plugin:control-center|delivery",
    expect.objectContaining({
      request: expect.objectContaining({ method: "restore_action", args: { action: "rollback", id: checkpoint } }),
    }),
  );
});

it("offers the clean activation path for a prepared new installation", async () => {
  respond(null, { phase: "import", committed: false, recordedOwners: 4, clean: true, freshHealth: false });
  render(<Restore description={fixtureDescription("control-center")} route="recovery" />);
  fireEvent.click(await screen.findByRole("button", { name: "다음 단계" }));
  expect(screen.getByRole("heading", { name: "신규 설치 활성화 준비" })).toBeTruthy();
  expect(screen.queryByText("이전 검토를 반영한 설치 확정")).toBeNull();
  expect(screen.queryByText("데이터를 보존하고 이전 검토로 돌아가기")).toBeNull();
});

it("keeps product launch and fresh health recording reachable during reinstall", async () => {
  respond(null, {
    phase: "health",
    committed: false,
    recordedOwners: 4,
    clean: false,
    freshHealth: false,
    reinstall: true,
  } as never);
  render(
    <Restore description={{ ...fixtureDescription("control-center"), deliveryState: "health" }} route="recovery" />,
  );
  expect(await screen.findByRole("button", { name: "Devbox Knowledge 열기" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "상태 기록" })).toBeTruthy();
  expect((screen.getByRole("button", { name: "보존된 데이터로 재설치 확정" }) as HTMLButtonElement).disabled).toBe(
    true,
  );
});

it("puts unfinished clean setup before optional preservation details and keeps activation review visible", async () => {
  respond(null, { phase: "import", committed: false, recordedOwners: 4, clean: true, freshHealth: false });
  render(
    <Restore description={{ ...fixtureDescription("control-center"), deliveryState: "import" }} route="recovery" />,
  );
  const setup = await screen.findByRole("region", { name: "설치 준비 안내" });
  const details = screen.getByText("데이터 보존·복원 상세").closest("details");
  expect(details).toBeTruthy();
  expect(details?.open).toBe(false);
  expect(setup.compareDocumentPosition(details!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "다음 단계" }));
  const review = screen.getByRole("region", { name: "복구 작업 검토" });
  expect(review.closest("details")).toBeNull();
  expect(review.compareDocumentPosition(details!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect((screen.getByRole("button", { name: "Control Center를 닫고 실행" }) as HTMLButtonElement).disabled).toBe(true);
});
