import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import Daily from "./Daily";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@devbox/product-shell/api", () => ({ nativeMode: true }));
vi.mock("@devbox/knowledge-features/transport", () => ({ componentInvoke: () => invoke }));
afterEach(cleanup);
beforeEach(() => { invoke.mockReset(); });
const preview = { path: "Journal/2024-02-29.md", content: "# 2024-02-29", previewId: "daily-1", exists: false };
function show() {
  const onOpen = vi.fn();
  const onDateChange = vi.fn();
  const view = render(<Daily date="2024-02-29" onDateChange={onDateChange} onOpen={onOpen} onActivity={vi.fn()}/>);
  return { ...view, onOpen, onDateChange };
}
it("requires a preview and explicit approval, consumes it once and opens only on request", async () => {
  invoke.mockImplementation(async (method: string) => method === "preview_daily" ? preview : method === "save_daily" ? { path: preview.path, indexed: true } : null);
  const { onOpen } = show();
  expect(invoke).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "일일 노트 확인" }));
  const save = await screen.findByRole("button", { name: "확인 후 새 노트 만들기" });
  expect(invoke).toHaveBeenCalledWith("preview_daily", { date: "2024-02-29" });
  expect(invoke.mock.calls.some(call => call[0] === "save_daily")).toBe(false);
  fireEvent.click(save); fireEvent.click(save);
  expect(await screen.findByText("일일 노트를 만들었습니다.")).toBeTruthy();
  expect(invoke.mock.calls.filter(call => call[0] === "save_daily")).toEqual([["save_daily", { previewId: "daily-1" }]]);
  expect(onOpen).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "노트 열기" }));
  expect(onOpen).toHaveBeenCalledWith(preview.path);
});
it("cancels an unused approval and discards a preview that arrives after unmount", async () => {
  invoke.mockResolvedValue(preview);
  const first = show();
  fireEvent.click(screen.getByRole("button", { name: "일일 노트 확인" }));
  fireEvent.click(await screen.findByRole("button", { name: "취소" }));
  expect(invoke).toHaveBeenCalledWith("discard_daily", { previewId: "daily-1" });
  first.unmount();
  invoke.mockReset();
  let resolve!: (value: typeof preview) => void;
  invoke.mockImplementation((method: string) => method === "preview_daily" ? new Promise(done => { resolve = done; }) : Promise.resolve(null));
  const second = show();
  fireEvent.click(screen.getByRole("button", { name: "일일 노트 확인" }));
  second.unmount();
  await act(async () => { resolve(preview); });
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("discard_daily", { previewId: "daily-1" }));
  expect(invoke.mock.calls.some(call => call[0] === "save_daily")).toBe(false);
});
it("offers only opening for an existing note and date selection has no native side effect", async () => {
  invoke.mockResolvedValue({ ...preview, previewId: null, exists: true });
  const { onDateChange } = show();
  fireEvent.change(screen.getByLabelText("일일 기록 날짜"), { target: { value: "2024-03-01" } });
  expect(onDateChange).toHaveBeenCalledWith("2024-03-01");
  expect(invoke).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "일일 노트 확인" }));
  expect(await screen.findByRole("button", { name: "노트 열기" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "확인 후 새 노트 만들기" })).toBeNull();
});
