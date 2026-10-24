// 검토 차단 1(송신 수명)·권장 2(검토 확인)·권장 3(충돌 다시 읽기).
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it } from "vitest";
import type { CollectionRoot } from "../../../rpc/gen/rpc";
import { ApiWorkspace } from "./ApiWorkspace";
import { ApiBackendProvider, SendFailure, type SentResponse } from "./backend";
import { useApiTabs } from "./store";
import { FakeBackend, textResponse } from "./test-backend";

const personal = { kind: "personal" } as const;
const project = { kind: "project", projectId: "p1" } as const;
const text = (name: string, url: string) =>
  `${JSON.stringify(
    {
      version: 1,
      name,
      request: {
        method: "GET",
        url,
        query: [],
        headers: [],
        cookies: [],
        body: { kind: "none" },
        auth: { kind: "none" },
        tls: { verify: true, caPath: null, clientCertPath: null, clientKeyPath: null },
        timeoutMs: 30_000,
        followRedirects: true,
      },
      assertions: [],
      captures: [],
    },
    null,
    2,
  )}\n`;
function open(backend: FakeBackend, root: CollectionRoot, path: string, name: string, reviewed = true) {
  const body = text(name, `https://x.test/${name.toLowerCase()}`);
  const digest = backend.seed(root, path, body);
  act(() => {
    useApiTabs.getState().openFile(root, path, body, digest, reviewed);
  });
}
function pending(backend: FakeBackend) {
  const calls: { signal: AbortSignal; resolve: (response: SentResponse) => void }[] = [];
  backend.reply = (_request, signal) =>
    new Promise((resolve, reject) => {
      signal.addEventListener("abort", () => reject(new SendFailure("cancelled")), { once: true });
      calls.push({ signal, resolve });
    });
  return calls;
}
function setup(backend: FakeBackend) {
  render(
    <ApiBackendProvider backend={backend}>
      <ApiWorkspace />
    </ApiBackendProvider>,
  );
}
async function saveAsNew(user: ReturnType<typeof userEvent.setup>, name: string) {
  await user.click(screen.getByRole("button", { name: "새 요청으로 저장" }));
  const dialog = await screen.findByRole("dialog", { name: "새 요청으로 저장" });
  await user.clear(within(dialog).getByLabelText("요청 이름"));
  await user.type(within(dialog).getByLabelText("요청 이름"), name);
  await user.click(within(dialog).getByRole("button", { name: "저장" }));
  await waitFor(() =>
    expect(screen.getByRole("tab", { name: new RegExp(`^${name}`) })).toHaveAttribute("aria-selected", "true"),
  );
}
afterEach(() => {
  cleanup();
  useApiTabs.getState().reset();
});

it("saving_while_sending_keeps_the_response_and_cancel", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  const calls = pending(backend);
  open(backend, personal, "a.request.json", "A");
  setup(backend);
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await waitFor(() => expect(calls).toHaveLength(1));
  // 저장으로 탭 key가 바뀌어도 진행 중인 송신을 잃지 않는다.
  await saveAsNew(user, "B");
  await user.click(screen.getByRole("button", { name: "취소" }));
  expect(calls[0]!.signal.aborted).toBe(true);
  expect(await screen.findByRole("button", { name: "보내기" })).toBeInTheDocument();

  await user.click(screen.getByRole("button", { name: "보내기" }));
  await waitFor(() => expect(calls).toHaveLength(2));
  await saveAsNew(user, "C");
  act(() => calls[1]!.resolve(textResponse('{"ok":true}')));
  expect(await screen.findByText("200 OK")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "보내기" })).toBeInTheDocument();
});

it("cancel_still_works_after_switching_tabs", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  const calls = pending(backend);
  open(backend, personal, "a.request.json", "A");
  open(backend, personal, "b.request.json", "B");
  act(() => useApiTabs.getState().select("personal:a.request.json"));
  setup(backend);
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await waitFor(() => expect(calls).toHaveLength(1));
  await user.click(screen.getByRole("tab", { name: /^B/ }));
  expect(screen.getByRole("button", { name: "보내기" })).toBeInTheDocument();
  await user.click(screen.getByRole("tab", { name: /^A/ }));
  await user.click(screen.getByRole("button", { name: "취소" }));
  expect(calls[0]!.signal.aborted).toBe(true);
  expect(await screen.findByRole("button", { name: "보내기" })).toBeInTheDocument();
});

it("unreviewed_project_file_needs_explicit_review_before_the_first_save", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  open(backend, project, "a.request.json", "A", false);
  setup(backend);
  expect(screen.getByText(/이 PC에서 아직 검토하지 않은 파일입니다/)).toBeInTheDocument();
  await user.type(screen.getByLabelText("URL"), "/b");
  await user.click(screen.getByRole("button", { name: "저장" }));
  const dialog = await screen.findByRole("dialog", { name: "검토하고 저장할까요?" });
  expect(backend.write).not.toHaveBeenCalled();
  await user.click(within(dialog).getByRole("button", { name: "검토하고 저장" }));
  await waitFor(() => expect(backend.review).toHaveBeenCalledOnce());
  expect(backend.review.mock.calls[0]!.slice(0, 2)).toEqual([project, "a.request.json"]);
  await waitFor(() => expect(screen.queryByText(/이 PC에서 아직 검토하지 않은 파일입니다/)).toBeNull());
  // 검토한 뒤의 저장은 다시 묻지 않는다.
  await user.type(screen.getByLabelText("URL"), "/c");
  await user.click(screen.getByRole("button", { name: "저장" }));
  await waitFor(() => expect(backend.write).toHaveBeenCalledTimes(2));
});

it("save_conflict_offers_reloading_the_file", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  open(backend, personal, "a.request.json", "A");
  setup(backend);
  backend.seed(personal, "a.request.json", text("A", "https://x.test/changed"), "d9");
  await user.type(screen.getByLabelText("URL"), "/b");
  await user.click(screen.getByRole("button", { name: "저장" }));
  await user.click(await screen.findByRole("button", { name: "다시 읽기" }));
  // 고친 내용이 있으면 한 번 더 묻는다(검토 권장 2-1).
  const confirm = await screen.findByRole("dialog", { name: "파일을 다시 읽을까요?" });
  expect(screen.getByLabelText("URL")).toHaveValue("https://x.test/a/b");
  await user.click(within(confirm).getByRole("button", { name: "다시 읽기" }));
  await waitFor(() => expect(screen.getByLabelText("URL")).toHaveValue("https://x.test/changed"));
  expect(screen.queryAllByLabelText("저장하지 않은 변경")).toHaveLength(0);
  await user.type(screen.getByLabelText("URL"), "/d");
  await user.click(screen.getByRole("button", { name: "저장" }));
  await waitFor(() => expect(backend.write.mock.calls.at(-1)![3]).toEqual({ kind: "digest", digest: "d9" }));
});

function gate() {
  let release!: () => void;
  const opened = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { opened, release };
}

it("edit_during_save_keeps_the_dirty_mark", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  open(backend, personal, "a.request.json", "A");
  const write = backend.write.getMockImplementation()!;
  const hold = gate();
  backend.write.mockImplementationOnce(async (...args) => {
    await hold.opened;
    return write(...args);
  });
  setup(backend);
  await user.type(screen.getByLabelText("URL"), "/one");
  await user.keyboard("{Control>}s{/Control}");
  await waitFor(() => expect(backend.write).toHaveBeenCalledOnce());
  await user.type(screen.getByLabelText("URL"), "/two");
  await act(async () => hold.release());
  await waitFor(() =>
    expect(JSON.parse(backend.text(personal, "a.request.json")!).request.url).toBe("https://x.test/a/one"),
  );
  expect(screen.getByLabelText("URL")).toHaveValue("https://x.test/a/one/two");
  expect(screen.queryAllByLabelText("저장하지 않은 변경")).toHaveLength(1);
  // 쓰지 않은 변경이 있으므로 닫을 때 묻는다.
  await user.click(screen.getByRole("button", { name: "A 닫기" }));
  expect(await screen.findByRole("dialog", { name: "A 탭을 닫을까요?" })).toBeInTheDocument();
});

it("edit_during_move_to_secret_is_not_overwritten", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  open(backend, personal, "a.request.json", "A");
  const setSecret = backend.setSecret.getMockImplementation()!;
  const hold = gate();
  backend.setSecret.mockImplementationOnce(async (...args) => {
    await hold.opened;
    return setSecret(...args);
  });
  setup(backend);
  await user.click(screen.getByRole("tab", { name: "헤더" }));
  await user.click(screen.getByRole("button", { name: "헤더 추가" }));
  await user.type(screen.getByLabelText("헤더 1 이름"), "Authorization");
  await user.type(screen.getByLabelText("헤더 1 값"), "Bearer typed-secret");
  await user.click(screen.getByRole("button", { name: "저장" }));
  const dialog = await screen.findByRole("dialog", { name: "직접 입력한 비밀 1개는 파일에 저장하지 않습니다" });
  await user.click(within(dialog).getByRole("button", { name: "비밀로 옮기기" }));
  await waitFor(() => expect(backend.setSecret).toHaveBeenCalledOnce());
  await user.type(screen.getByLabelText("URL"), "/later");
  await act(async () => hold.release());
  // 옮긴 참조는 반영하되 그 사이 고친 URL은 지우지 않고, 바뀐 내용은 저장하지 않는다.
  await waitFor(() => expect(screen.getByLabelText("헤더 1 값")).toHaveValue("{{secret:A_AUTHORIZATION}}"));
  expect(screen.getByLabelText("URL")).toHaveValue("https://x.test/a/later");
  expect(backend.write).not.toHaveBeenCalled();
  expect(screen.getByText(/옮기는 동안 내용이 바뀌어 저장하지 않았습니다/)).toBeInTheDocument();
});

it("ctrl_enter_while_sending_explains_why_nothing_happened", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  const calls = pending(backend);
  open(backend, personal, "a.request.json", "A");
  setup(backend);
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await waitFor(() => expect(calls).toHaveLength(1));
  await user.click(screen.getByLabelText("URL"));
  await user.keyboard("{Control>}{Enter}{/Control}");
  expect(await screen.findByText(/이미 보내는 중입니다/)).toBeInTheDocument();
  expect(calls).toHaveLength(1);
});

it("move_to_secret_while_saving_as_new_keeps_the_reference", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  open(backend, personal, "a.request.json", "A");
  const setSecret = backend.setSecret.getMockImplementation()!;
  const hold = gate();
  backend.setSecret.mockImplementationOnce(async (...args) => {
    await hold.opened;
    return setSecret(...args);
  });
  setup(backend);
  await user.click(screen.getByRole("tab", { name: "헤더" }));
  await user.click(screen.getByRole("button", { name: "헤더 추가" }));
  await user.type(screen.getByLabelText("헤더 1 이름"), "Authorization");
  await user.type(screen.getByLabelText("헤더 1 값"), "Bearer typed-secret");
  await user.click(screen.getByRole("button", { name: "저장" }));
  let dialog = await screen.findByRole("dialog", { name: "직접 입력한 비밀 1개는 파일에 저장하지 않습니다" });
  await user.click(within(dialog).getByRole("button", { name: "비밀로 옮기기" }));
  await waitFor(() => expect(backend.setSecret).toHaveBeenCalledOnce());
  // 비밀을 옮기는 동안 새 요청으로 저장해 탭 key가 바뀐다.
  await user.click(screen.getByRole("button", { name: "새 요청으로 저장" }));
  const named = await screen.findByRole("dialog", { name: "새 요청으로 저장" });
  await user.clear(within(named).getByLabelText("요청 이름"));
  await user.type(within(named).getByLabelText("요청 이름"), "B");
  await user.click(within(named).getByRole("button", { name: "저장" }));
  dialog = await screen.findByRole("dialog", { name: "직접 입력한 비밀 1개는 파일에 저장하지 않습니다" });
  await user.click(within(dialog).getByRole("button", { name: "가리고 저장" }));
  await waitFor(() => expect(screen.getByRole("tab", { name: /^B/ })).toHaveAttribute("aria-selected", "true"));
  await act(async () => hold.release());
  // 만든 비밀의 참조가 초안에 반영되고(평문이 남지 않음), 바뀐 탭은 따로 저장하지 않는다.
  await waitFor(() => expect(screen.getByLabelText("헤더 1 값")).toHaveValue("{{secret:A_AUTHORIZATION}}"));
  expect(screen.getByText(/저장하지 않았습니다/)).toBeInTheDocument();
  expect(backend.write).toHaveBeenCalledOnce();
  expect(backend.text(personal, "B.request.json")).not.toContain("typed-secret");
});

it("move_to_secret_blocked_by_another_save_still_applies_the_reference", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  open(backend, personal, "a.request.json", "A");
  const write = backend.write.getMockImplementation()!;
  const hold = gate();
  backend.write.mockImplementationOnce(async (...args) => {
    await hold.opened;
    return write(...args);
  });
  setup(backend);
  await user.type(screen.getByLabelText("URL"), "/one");
  await user.keyboard("{Control>}s{/Control}");
  await waitFor(() => expect(backend.write).toHaveBeenCalledOnce());
  await user.click(screen.getByRole("tab", { name: "헤더" }));
  await user.click(screen.getByRole("button", { name: "헤더 추가" }));
  await user.type(screen.getByLabelText("헤더 1 이름"), "Authorization");
  await user.type(screen.getByLabelText("헤더 1 값"), "Bearer typed-secret");
  await user.keyboard("{Control>}s{/Control}");
  const dialog = await screen.findByRole("dialog", { name: "직접 입력한 비밀 1개는 파일에 저장하지 않습니다" });
  await user.click(within(dialog).getByRole("button", { name: "비밀로 옮기기" }));
  // 다른 저장이 진행 중이라 쓰지 못해도 만든 비밀은 참조로 남기고 알린다(고아 비밀·평문 잔류 방지).
  await waitFor(() => expect(screen.getByLabelText("헤더 1 값")).toHaveValue("{{secret:A_AUTHORIZATION}}"));
  expect(screen.getByText(/다른 저장이 진행 중이라 저장하지 않았습니다/)).toBeInTheDocument();
  await act(async () => hold.release());
  await waitFor(() => expect(backend.write).toHaveBeenCalledOnce());
});
