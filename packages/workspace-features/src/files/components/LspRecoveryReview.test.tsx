import { useLayoutEffect } from "react";
import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { applyLspRecovery, cancelLspRecovery, listLspRecovery, previewLspRecovery, type LspRecoveryPreview } from "../api";
import LspRecoveryReview from "./LspRecoveryReview";
vi.mock("../api", () => ({ applyLspRecovery: vi.fn(), cancelLspRecovery: vi.fn(), listLspRecovery: vi.fn(), previewLspRecovery: vi.fn() }));
const preview: LspRecoveryPreview = { previewId:"once", journalId:"journal", files:[{path:"src/fixture.rs", restore:true, current:"after", original:"before", currentSize:5, originalSize:6}] };
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(listLspRecovery).mockResolvedValue({records:[{journalId:"journal", files:1, available:true}], truncated:false});
  vi.mocked(previewLspRecovery).mockResolvedValue(preview);
  vi.mocked(cancelLspRecovery).mockResolvedValue();
  vi.mocked(applyLspRecovery).mockResolvedValue({complete:true, restored:["src/fixture.rs"], cleanupPending:false, error:null});
});
afterEach(cleanup);
it("keeps a recovery listing requested immediately after paint", async () => {
  function PaintedRecovery() {
    useLayoutEffect(() => {
      document.querySelector<HTMLButtonElement>('section[aria-label="이름 변경 복구"] button')!.click();
    }, []);
    return <LspRecoveryReview disabled={false}/>;
  }
  const view = render(<PaintedRecovery/>);
  await view.findByText("기록 1 복구 검토");
  expect(listLspRecovery).toHaveBeenCalledOnce();
});
async function review(view: ReturnType<typeof render>) {
  fireEvent.click(view.getByText("복구 기록 확인"));
  fireEvent.click(await view.findByText("기록 1 복구 검토"));
  await view.findByText("검토한 원본 복원");
}
it("requires explicit metadata, preview and apply actions without server configuration", async () => {
  const view = render(<LspRecoveryReview disabled={false}/>);
  expect(listLspRecovery).not.toHaveBeenCalled();
  expect(previewLspRecovery).not.toHaveBeenCalled();
  await review(view);
  expect(previewLspRecovery).toHaveBeenCalledWith("journal");
  expect(view.getByText("before")).toBeTruthy();
  expect(view.getByText("after")).toBeTruthy();
  expect(applyLspRecovery).not.toHaveBeenCalled();
  fireEvent.click(view.getByText("검토한 원본 복원"));
  await waitFor(() => expect(applyLspRecovery).toHaveBeenCalledWith("once"));
  await waitFor(() => expect(view.getByRole("status").textContent).toContain("완료되었습니다"));
});
it("retires cancelled and late unmounted reviews without applying", async () => {
  const view = render(<LspRecoveryReview disabled={false}/>);
  await review(view);
  fireEvent.click(view.getByText("복구 검토 취소"));
  await waitFor(() => expect(cancelLspRecovery).toHaveBeenCalledWith("once"));
  await waitFor(() => expect(view.queryByText("검토한 원본 복원")).toBeNull());
  let resolve!: (value: LspRecoveryPreview) => void;
  vi.mocked(previewLspRecovery).mockReturnValue(new Promise(done => { resolve = done; }));
  fireEvent.click(view.getByText("기록 1 복구 검토"));
  view.unmount();
  await act(async () => { resolve({...preview, previewId:"late"}); });
  expect(cancelLspRecovery).toHaveBeenCalledWith("late");
  expect(applyLspRecovery).not.toHaveBeenCalled();
});
it("consumes a failed apply and reports partial recovery with retained backups", async () => {
  vi.mocked(applyLspRecovery).mockRejectedValueOnce(new Error("파일이 변경되었습니다"));
  const view = render(<LspRecoveryReview disabled={false}/>);
  await review(view);
  fireEvent.click(view.getByText("검토한 원본 복원"));
  await waitFor(() => expect(view.getByRole("alert").textContent).toContain("파일이 변경"));
  expect(view.queryByText("검토한 원본 복원")).toBeNull();
  vi.mocked(applyLspRecovery).mockResolvedValueOnce({complete:false, restored:["first.rs"], cleanupPending:true, error:"권한 변경"});
  await review(view);
  fireEvent.click(view.getByText("검토한 원본 복원"));
  await waitFor(() => expect(view.getByRole("status").textContent).toContain("일부 복원을 완료하지 못했습니다"));
  expect(view.getByRole("status").textContent).toContain("first.rs");
  expect(view.getByRole("status").textContent).toContain("복구 기록 정리가 남아");
});
it("does not offer malformed journals for recovery", async () => {
  vi.mocked(listLspRecovery).mockResolvedValue({records:[{journalId:"bad", files:0, available:false}], truncated:true});
  const view = render(<LspRecoveryReview disabled={false}/>);
  fireEvent.click(view.getByText("복구 기록 확인"));
  const button = await view.findByText("기록 1 복구 검토") as HTMLButtonElement;
  expect(button.disabled).toBe(true);
  fireEvent.click(button);
  expect(previewLspRecovery).not.toHaveBeenCalled();
  expect(view.getByText(/손상된 기록/)).toBeTruthy();
});
