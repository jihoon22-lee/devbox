import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { approveLspExecution, cancelLspExecutionReview, previewLspExecution, revokeLspExecution, type LspExecutionPreview } from "../api";
import LspExecutionReview from "./LspExecutionReview";

vi.mock("../api", () => ({ approveLspExecution: vi.fn(), cancelLspExecutionReview: vi.fn(), previewLspExecution: vi.fn(), revokeLspExecution: vi.fn() }));
const preview: LspExecutionPreview = { previewId: "review-1", approved: false, workspaceRoot: "C:/fixture", configRevision: "revision-1", commands: [{ languageId: "rust", executable: "C:/tools/node.exe", runtime: "C:/tools/node.exe", args: ["C:/tools/server.mjs", "--stdio"] }], environment: { PATH: "C:/tools", SystemRoot: "C:/Windows" }, definitionsDigest: "digest" };
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(previewLspExecution).mockResolvedValue(preview);
  vi.mocked(approveLspExecution).mockResolvedValue();
  vi.mocked(cancelLspExecutionReview).mockResolvedValue();
  vi.mocked(revokeLspExecution).mockResolvedValue();
});
afterEach(cleanup);

it("reads execution evidence only on request and displays it before approval", async () => {
  const view = render(<LspExecutionReview disabled={false} nativeRevision="revision-1"/>);
  expect(previewLspExecution).not.toHaveBeenCalled();
  fireEvent.click(view.getByText("실행 설정 검토"));
  await waitFor(() => expect(view.getByText("이 설정의 실행 승인")).toBeTruthy());
  expect(view.getByText(/server\.mjs/)).toBeTruthy();
  expect(view.getByText("SystemRoot")).toBeTruthy();
  expect(approveLspExecution).not.toHaveBeenCalled();
  fireEvent.click(view.getByText("이 설정의 실행 승인"));
  await waitFor(() => expect(approveLspExecution).toHaveBeenCalledWith("review-1"));
  await waitFor(() => expect(view.getByRole("status").textContent).toContain("시작 버튼"));
});

it("retires a late preview after unmount and does not approve it", async () => {
  let resolve!: (value: LspExecutionPreview) => void;
  vi.mocked(previewLspExecution).mockReturnValue(new Promise(done => { resolve = done; }));
  const view = render(<LspExecutionReview disabled={false} nativeRevision="revision-1"/>);
  fireEvent.click(view.getByText("실행 설정 검토"));
  view.unmount();
  await act(async () => { resolve(preview); });
  expect(cancelLspExecutionReview).toHaveBeenCalledWith("review-1");
  expect(approveLspExecution).not.toHaveBeenCalled();
});

it("rejects a review of another saved revision", async () => {
  const view = render(<LspExecutionReview disabled={false} nativeRevision="revision-2"/>);
  fireEvent.click(view.getByText("실행 설정 검토"));
  await waitFor(() => expect(view.getByRole("alert").textContent).toContain("설정이 변경"));
  expect(cancelLspExecutionReview).toHaveBeenCalledWith("review-1");
  expect(view.queryByText("이 설정의 실행 승인")).toBeNull();
});

it("consumes failed approvals and supports explicit revocation", async () => {
  vi.mocked(approveLspExecution).mockRejectedValue(new Error("실행 근거가 변경되었습니다."));
  const view = render(<LspExecutionReview disabled={false} nativeRevision="revision-1"/>);
  fireEvent.click(view.getByText("실행 설정 검토"));
  fireEvent.click(await view.findByText("이 설정의 실행 승인"));
  await waitFor(() => expect(view.getByRole("alert").textContent).toContain("실행 근거"));
  expect(view.queryByText("이 설정의 실행 승인")).toBeNull();
  fireEvent.click(view.getByText("실행 승인 해제"));
  await waitFor(() => expect(revokeLspExecution).toHaveBeenCalledOnce());
});

it("shows native WSL review with environment names and no values", async () => {
  vi.mocked(previewLspExecution).mockResolvedValue({ ...preview, workspaceRoot: "/home/fixture", environment: undefined, environmentKeys: ["HOME", "PATH"], commands: [{languageId: "rust", executable: "/usr/bin/server", args: ["--stdio"], runtime: null}] });
  const view = render(<LspExecutionReview disabled={false} nativeRevision="revision-1"/>);
  fireEvent.click(view.getByText("실행 설정 검토"));
  await view.findByText("이 설정의 실행 승인");
  expect(view.getByText("/usr/bin/server")).toBeTruthy();
  expect(view.getByText("변수 이름: HOME, PATH")).toBeTruthy();
});
