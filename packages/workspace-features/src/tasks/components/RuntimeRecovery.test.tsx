import { StrictMode } from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import RuntimeRecovery from "./RuntimeRecovery";
import { listRuntimeControls, reconcileRuntimeControls, reviewRuntimeControl } from "../api";
vi.mock("../../transport", () => ({ isProductHosted: () => true }));
vi.mock("../api", () => ({
  listRuntimeControls: vi.fn(),
  reconcileRuntimeControls: vi.fn().mockResolvedValue([]),
  reviewRuntimeControl: vi.fn(),
}));
vi.mock("../runtimeControls", () => ({ forgetReviewedControl: vi.fn() }));
afterEach(cleanup);
const receipt = {
  operationId: "owned-receipt",
  method: "stop_service",
  targetId: "service",
  state: "interrupted",
  result: null,
  createdAt: 1,
  reviewed: false,
};
it("identifies the interrupted request and opens the target without another runtime control", async () => {
  vi.mocked(listRuntimeControls).mockResolvedValue([receipt]);
  const target = vi.fn();
  render(<RuntimeRecovery active busy={false} onReviewed={vi.fn()} onTarget={target} />);
  await screen.findByText(/서비스 중지 요청/);
  fireEvent.click(screen.getByRole("button", { name: "대상 실행 상태 보기" }));
  expect(target).toHaveBeenCalledWith(receipt);
  expect(reviewRuntimeControl).not.toHaveBeenCalled();
});
it("clears an old read error after successful status refresh", async () => {
  vi.mocked(listRuntimeControls).mockRejectedValueOnce(new Error("offline")).mockResolvedValue([receipt]);
  render(<RuntimeRecovery active busy={false} onReviewed={vi.fn()} />);
  await screen.findByRole("alert");
  fireEvent.click(screen.getByRole("button", { name: "상태 새로고침" }));
  await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
});

it("shows native confirmation for a lost stop reply without submitting another control", async () => {
  vi.mocked(listRuntimeControls).mockResolvedValue([]);
  vi.mocked(reconcileRuntimeControls).mockResolvedValueOnce([
    { operationId: "confirmed", method: "stop_active_run", targetId: "job", state: "completed" },
  ]);
  render(<RuntimeRecovery active busy={false} onReviewed={vi.fn()} />);
  await screen.findByText(/실행 요청의 완료 상태를 확인했습니다/);
  expect(reviewRuntimeControl).not.toHaveBeenCalled();
});

it("retains a completed lost-reply observation during StrictMode effect replay", async () => {
  vi.mocked(listRuntimeControls).mockResolvedValue([]);
  vi.mocked(reconcileRuntimeControls)
    .mockReset()
    .mockResolvedValue([])
    .mockResolvedValueOnce([
      { operationId: "original-operation", method: "run_job", targetId: "owned-job", state: "completed" },
    ]);
  render(
    <StrictMode>
      <RuntimeRecovery active busy={false} onReviewed={vi.fn()} />
    </StrictMode>,
  );
  await screen.findByText(/실행 요청의 완료 상태를 확인했습니다/);
  expect(reconcileRuntimeControls).toHaveBeenCalledTimes(1);
  expect(reviewRuntimeControl).not.toHaveBeenCalled();
});

it("retains a consuming confirmation when the route hides before the read finishes", async () => {
  let finish!: (value: Awaited<ReturnType<typeof reconcileRuntimeControls>>) => void;
  vi.mocked(reconcileRuntimeControls)
    .mockReset()
    .mockResolvedValue([])
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
  vi.mocked(listRuntimeControls).mockResolvedValue([]);
  const view = render(<RuntimeRecovery active busy={false} onReviewed={vi.fn()} />);
  view.rerender(<RuntimeRecovery active={false} busy={false} onReviewed={vi.fn()} />);
  finish([{ operationId: "original-operation", method: "run_job", targetId: "owned-job", state: "completed" }]);
  await waitFor(() => expect(listRuntimeControls).toHaveBeenCalled());
  view.rerender(<RuntimeRecovery active busy={false} onReviewed={vi.fn()} />);
  await screen.findByText(/실행 요청의 완료 상태를 확인했습니다/);
});

it("retains confirmed completion when the later receipt inventory read fails", async () => {
  vi.mocked(reconcileRuntimeControls)
    .mockReset()
    .mockResolvedValue([])
    .mockResolvedValueOnce([
      { operationId: "original-operation", method: "run_job", targetId: "owned-job", state: "completed" },
    ]);
  vi.mocked(listRuntimeControls).mockRejectedValueOnce(new Error("inventory unavailable")).mockResolvedValue([]);
  render(<RuntimeRecovery active busy={false} onReviewed={vi.fn()} />);
  await screen.findByRole("alert");
  fireEvent.click(screen.getByRole("button", { name: "상태 새로고침" }));
  await screen.findByText(/실행 요청의 완료 상태를 확인했습니다/);
});
