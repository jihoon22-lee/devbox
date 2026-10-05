import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
const mocks = vi.hoisted(() => ({ diff: vi.fn() }));
vi.mock("../api", () => mocks);
vi.mock("./common", () => ({
  ToolTextArea: () => null,
  ToolOutput: ({ children, ariaLabel }: { children: React.ReactNode; ariaLabel: string }) => (
    <div aria-label={ariaLabel}>{children}</div>
  ),
}));
import { DiffDraftContext, DiffTool } from "./diff";
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
it("keeps unchanged rows aligned after inserted and deleted lines", async () => {
  mocks.diff.mockResolvedValue([
    { kind: 1, old_start: 0, old_end: 0, new_start: 0, new_end: 2 },
    { kind: 0, old_start: 0, old_end: 1, new_start: 2, new_end: 3 },
    { kind: 2, old_start: 1, old_end: 2, new_start: 3, new_end: 3 },
  ]);
  render(
    <DiffDraftContext.Provider value={{ a: "same\nremoved", b: "one\ntwo\nsame", setA: vi.fn(), setB: vi.fn() }}>
      <DiffTool />
    </DiffDraftContext.Provider>,
  );
  await waitFor(() =>
    expect(screen.getByLabelText("이전 버전 차이 결과").querySelectorAll(".diff-line")).toHaveLength(4),
  );
  expect(screen.getByLabelText("새 버전 차이 결과").querySelectorAll(".diff-line")).toHaveLength(4);
  expect(screen.getByLabelText("이전 버전 차이 결과").children[2].textContent).toBe("same");
  expect(screen.getByLabelText("새 버전 차이 결과").children[2].textContent).toBe("same");
});
it("shows a safe error when the native diff fails", async () => {
  mocks.diff.mockRejectedValue(new Error("private native detail"));
  render(<DiffTool />);
  expect((await screen.findByRole("alert")).textContent).toBe(
    "차이를 계산하지 못했습니다. 입력 크기와 연결 상태를 확인해 주세요.",
  );
});
