// 종료 확인: 화면 메모리에만 있는 요청 탭·환경 초안의 저장하지 않은 변경을 종료 전에 묻는다.
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { ApiBackendProvider } from "./backend";
import { EnvEditor, resetEnvDrafts } from "./EnvEditor";
import { emptyDraft } from "./model/draft";
import { fileFromDraft } from "./model/fileDraft";
import { ApiQuitDialog, confirmApiQuit, useQuitGuard } from "./quitGuard";
import { useApiTabs } from "./store";
import { FakeBackend } from "./test-backend";

const personal = { kind: "personal" } as const;
const TITLE = "저장하지 않은 API 변경이 있습니다";
afterEach(() => {
  cleanup();
  useQuitGuard.getState().request?.answer(false);
  resetEnvDrafts();
  useApiTabs.getState().reset();
});
function ask(signal = new AbortController().signal) {
  let answer: Promise<boolean> | undefined;
  act(() => {
    answer = confirmApiQuit(signal);
  });
  return answer;
}
async function editEnvironment() {
  render(
    <ApiBackendProvider backend={new FakeBackend()}>
      <EnvEditor root={personal} />
    </ApiBackendProvider>,
  );
  await act(async () => {});
  fireEvent.click(screen.getByRole("button", { name: "새 환경" }));
  fireEvent.change(screen.getByLabelText("환경 이름"), { target: { value: "dev" } });
}

test("quit_guard_names_only_the_kinds_with_changes", async () => {
  render(<ApiQuitDialog />);
  // 저장한 요청 탭은 변경이 아니다. 묻지 않으니 문서 확인이 바로 이어진다.
  useApiTabs
    .getState()
    .openFile(personal, "saved.request.json", JSON.stringify(fileFromDraft(emptyDraft(), "A")), "d1");
  expect(ask()).toBeUndefined();
  expect(screen.queryByRole("dialog")).toBeNull();

  act(() => {
    useApiTabs.getState().openNew(personal);
    useApiTabs.getState().openNew(personal);
  });
  let answer = ask();
  expect(screen.getByRole("dialog", { name: TITLE })).toHaveAccessibleDescription(
    "요청 2개의 변경을 저장하지 않았습니다. 종료하면 이 변경은 사라집니다.",
  );
  fireEvent.click(screen.getByRole("button", { name: "취소" }));
  await answer;

  act(() => useApiTabs.getState().reset());
  await editEnvironment();
  answer = ask();
  expect(screen.getByRole("dialog", { name: TITLE })).toHaveAccessibleDescription(
    "환경 1개의 변경을 저장하지 않았습니다. 종료하면 이 변경은 사라집니다.",
  );
  fireEvent.click(screen.getByRole("button", { name: "취소" }));
  await answer;

  act(() => {
    useApiTabs.getState().openNew(personal);
  });
  answer = ask();
  const dialog = screen.getByRole("dialog", { name: TITLE });
  expect(dialog).toHaveAccessibleDescription(
    "요청 1개와 환경 1개의 변경을 저장하지 않았습니다. 종료하면 이 변경은 사라집니다.",
  );
  expect(
    within(dialog)
      .getAllByRole("button")
      .map((button) => button.textContent),
  ).toEqual(["취소", "변경 버리고 종료"]);
  fireEvent.click(within(dialog).getByRole("button", { name: "취소" }));
  await answer;
});

test("quit_guard_cancel_resolves_false_and_discard_resolves_true", async () => {
  render(<ApiQuitDialog />);
  act(() => {
    useApiTabs.getState().openNew(personal);
  });
  const tabs = useApiTabs.getState().tabs;

  let answer = ask();
  // 실수로 누른 Enter가 변경을 버리지 않게 [취소]가 먼저 초점을 받는다.
  expect(screen.getByRole("button", { name: "취소" })).toHaveFocus();
  fireEvent.click(screen.getByRole("button", { name: "취소" }));
  expect(await answer).toBe(false);
  expect(screen.queryByRole("dialog")).toBeNull();

  answer = ask();
  fireEvent.click(screen.getByRole("button", { name: "변경 버리고 종료" }));
  expect(await answer).toBe(true);
  expect(screen.queryByRole("dialog")).toBeNull();
  // 버리기를 골라도 지우지 않는다. 이어지는 문서 확인에서 취소하면 초안이 그대로 남아야 한다.
  expect(useApiTabs.getState().tabs).toBe(tabs);

  answer = ask();
  fireEvent.keyDown(screen.getByRole("button", { name: "취소" }), { key: "Escape" });
  expect(await answer).toBe(false);

  // 데몬 쪽 종료 취소(30초 기한 등)는 창을 닫고 취소로 끝낸다.
  const native = new AbortController();
  answer = ask(native.signal);
  expect(screen.getByRole("dialog", { name: TITLE })).toBeInTheDocument();
  act(() => native.abort());
  expect(await answer).toBe(false);
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(useQuitGuard.getState().request).toBeUndefined();
  expect(useApiTabs.getState().tabs).toBe(tabs);
});
