import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { UpdateSnapshot } from "./nativeUpdate";
import { Update } from "./Update";

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), off: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: native.listen }));
let event: (event: { payload: unknown }) => void;
const initial: UpdateSnapshot = {
  enabled: true,
  disabledReason: null,
  phase: "idle",
  generation: 0,
  revision: 0,
  currentVersion: "1.2.3",
  availableVersion: null,
  receivedBytes: 0,
  totalBytes: null,
  error: null,
  nextAction: "check",
};
beforeEach(() => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  native.invoke.mockReset().mockResolvedValue(initial);
  native.off.mockReset();
  native.listen.mockReset().mockImplementation(async (_name, listener) => {
    event = listener;
    return native.off;
  });
});
afterEach(() => {
  cleanup();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});
function publish(value: Partial<UpdateSnapshot>) {
  act(() => event({ payload: { ...initial, ...value } }));
}

test("explicit check, download and cancel use native snapshots and generation; ready offers no installation", async () => {
  render(<Update />);
  expect(await screen.findByText("앱 1.2.3")).toBeVisible();
  expect(native.invoke).toHaveBeenCalledTimes(1);
  native.invoke.mockResolvedValueOnce({
    ...initial,
    phase: "checking",
    generation: 1,
    revision: 1,
    nextAction: "none",
  });
  fireEvent.click(screen.getByRole("button", { name: "새 버전 확인" }));
  await screen.findByText("새 버전을 확인하고 있습니다.");
  expect(native.invoke).toHaveBeenLastCalledWith("update_check");
  publish({ phase: "available", generation: 1, revision: 2, availableVersion: "1.3.0", nextAction: "download" });
  expect(screen.getByText("새 버전 1.3.0")).toBeVisible();
  native.invoke.mockResolvedValueOnce({
    ...initial,
    phase: "downloading",
    generation: 2,
    revision: 3,
    nextAction: "cancel",
    totalBytes: 100,
  });
  fireEvent.click(screen.getByRole("button", { name: "업데이트 다운로드" }));
  await screen.findByRole("button", { name: "다운로드 취소" });
  expect(native.invoke).toHaveBeenLastCalledWith("update_download", { generation: 1 });
  publish({
    phase: "downloading",
    generation: 2,
    revision: 4,
    nextAction: "cancel",
    receivedBytes: 50,
    totalBytes: 100,
  });
  expect(screen.getByRole("progressbar", { name: "업데이트 다운로드 진행" })).toHaveAttribute("value", "50");
  native.invoke.mockResolvedValueOnce({
    ...initial,
    phase: "available",
    generation: 3,
    revision: 5,
    nextAction: "download",
    availableVersion: "1.3.0",
  });
  fireEvent.click(screen.getByRole("button", { name: "다운로드 취소" }));
  await screen.findByRole("button", { name: "업데이트 다운로드" });
  expect(native.invoke).toHaveBeenLastCalledWith("update_cancel", { generation: 2 });
  publish({ phase: "ready", generation: 4, revision: 9, nextAction: "none", availableVersion: "1.3.0" });
  expect(screen.getByText("업데이트 파일 준비 완료")).toBeVisible();
  expect(screen.queryByRole("button")).not.toBeInTheDocument();
});

test("pending check prevents duplicate clicks and unknown errors expose only fixed recovery text", async () => {
  render(<Update />);
  const check = await screen.findByRole("button", { name: "새 버전 확인" });
  let reject!: (reason: unknown) => void;
  native.invoke.mockImplementationOnce(
    () =>
      new Promise((_resolve, no) => {
        reject = no;
      }),
  );
  fireEvent.click(check);
  fireEvent.click(check);
  expect(check).toBeDisabled();
  expect(native.invoke.mock.calls.filter(([name]) => name === "update_check")).toHaveLength(1);
  await act(async () => reject({ url: "https://private.invalid", body: "secret body" }));
  expect(screen.getByRole("alert")).toHaveTextContent("상태를 다시 불러오세요");
  expect(document.body).not.toHaveTextContent("private.invalid");
  expect(document.body).not.toHaveTextContent("secret body");
  fireEvent.click(screen.getByRole("button", { name: "상태 다시 불러오기" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "새 버전 확인" })).toBeEnabled());
  expect(native.off).toHaveBeenCalledOnce();
});

test.each([
  ["channel", "이 채널에서는 공개 업데이트를 사용할 수 없습니다."],
  ["developmentVersion", "개발 버전에서는 공개 업데이트를 사용할 수 없습니다."],
] as const)("disabled %s explains its cause without check or download actions", async (disabledReason, text) => {
  native.invoke.mockResolvedValueOnce({
    ...initial,
    enabled: false,
    disabledReason,
    currentVersion: "1.2.3-dev",
    nextAction: "none",
  });
  render(<Update />);
  expect(await screen.findByText("앱 1.2.3-dev")).toBeVisible();
  expect(screen.getByText(new RegExp(text))).toBeVisible();
  expect(screen.queryByRole("button")).not.toBeInTheDocument();
});

test("Rust chooses the retry action and an unknown download length stays indeterminate", async () => {
  native.invoke.mockResolvedValueOnce({
    ...initial,
    phase: "failed",
    generation: 7,
    revision: 8,
    error: "digest",
    nextAction: "download",
  });
  render(<Update />);
  expect(await screen.findByRole("alert")).toHaveTextContent("파일 검증에 실패했습니다. 다시 내려받으세요.");
  expect(screen.getByRole("button", { name: "업데이트 다운로드" })).toBeEnabled();
  expect(screen.queryByRole("button", { name: "새 버전 확인" })).not.toBeInTheDocument();
  publish({ phase: "downloading", generation: 8, revision: 9, receivedBytes: 25, nextAction: "cancel" });
  expect(screen.getByRole("progressbar")).not.toHaveAttribute("value");
  expect(screen.getByText("25바이트")).toBeVisible();
});
