import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import RuntimeRecovery from "./RuntimeRecovery";
import { listRuntimeControls, reviewRuntimeControl } from "../api";
vi.mock("../../transport", () => ({ isProductHosted: () => true }));
vi.mock("../api", () => ({ listRuntimeControls: vi.fn(), reviewRuntimeControl: vi.fn() }));
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
