import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../rpc/RpcProvider";
import { useCurrentProject } from "../projects/store";
import { ProjectWire, projects } from "../projects/test-wire";
import { trustPreview } from "../projects/trust-fixtures";
import { NewTaskDialog } from "./NewTaskDialog";
import { profiles, task } from "./test-fixtures";

beforeEach(() => {
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: vi.fn() });
  localStorage.clear();
});

const profileKey = (projectId: string) => `devbox.newTask.lastProfile.${projectId}`;
async function dialog(items = projects, saved: string | null = "claude", trustRequired = false) {
  if (saved !== null) localStorage.setItem(profileKey("a"), saved);
  useCurrentProject.getState().set("a");
  const wire = new ProjectWire({ trustNotRequired: !trustRequired }),
    created = vi.fn(),
    changed = vi.fn();
  const view = render(
    <RpcProvider client={wire.client}>
      <NewTaskDialog open onOpenChange={changed} onCreated={created} />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.profiles")).toHaveLength(1));
  await act(async () => {
    wire.reply("projects.list", { rev: "1", items });
    wire.reply("agents.profiles", { items: profiles });
  });
  if (items.length) {
    await waitFor(() => expect(wire.requests("projects.prompts")).toHaveLength(1));
    expect(wire.request("projects.prompts").params).toEqual({ id: "a" });
    await act(async () =>
      wire.reply("projects.prompts", {
        items: [{ name: "bugfix", body: "버그: {{title}}\n{{branch}} {{project}} {{other}}" }],
      }),
    );
  }
  return { wire, created, changed, view };
}

test("snapshots chosen profile, project, template, branch and worktree; guards IME and rapid duplicate submits", async () => {
  const { wire, created, changed } = await dialog();
  await userEvent.type(screen.getByLabelText("제목"), "Fix login");
  act(() => screen.getByRole("combobox", { name: "도구" }).focus());
  await userEvent.keyboard("{Enter}");
  await screen.findByRole("listbox");
  await userEvent.keyboard("{Escape}");
  expect(screen.getByRole("dialog", { name: "새 에이전트 작업" })).toBeInTheDocument();
  expect(screen.getByRole("combobox", { name: "도구" })).toHaveFocus();
  await userEvent.keyboard("{Enter}");
  await screen.findByRole("listbox");
  await userEvent.keyboard("{End}{Enter}");
  await userEvent.type(screen.getByLabelText("기준 브랜치"), "develop");
  await userEvent.click(screen.getByLabelText("작업 폴더 만들기"));
  await userEvent.click(screen.getByRole("button", { name: "템플릿: bugfix" }));
  expect(screen.getByLabelText("지시문")).toHaveValue("버그: Fix login\ndevelop devbox {{other}}");
  fireEvent.keyDown(screen.getByLabelText("지시문"), { key: "Enter", ctrlKey: true, isComposing: true });
  expect(wire.requests("agents.create")).toHaveLength(0);
  fireEvent.keyDown(screen.getByLabelText("지시문"), { key: "Enter", ctrlKey: true });
  fireEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  expect(wire.request("agents.create").params).toEqual({
    projectId: "a",
    title: "Fix login",
    profile: "codex",
    baseBranch: "develop",
    worktree: false,
    prompt: "버그: Fix login\ndevelop devbox {{other}}",
  });
  await userEvent.keyboard("{Escape}");
  expect(changed).not.toHaveBeenCalled();
  await act(async () => wire.reply("agents.create", task("new", "preparing")));
  await waitFor(() => expect(created).toHaveBeenCalledWith("new"));
  expect(changed).toHaveBeenCalledWith(false);
  expect(localStorage.getItem(profileKey("a"))).toBe("codex");
});

test("requires a registered project and never sends null project", async () => {
  const { wire } = await dialog([]);
  await userEvent.type(screen.getByLabelText("제목"), "Test");
  expect(screen.getByRole("button", { name: "시작" })).toBeDisabled();
  expect(screen.getByText("에이전트 작업을 만들 프로젝트를 등록해 주세요.")).toBeInTheDocument();
  expect(wire.requests("projects.prompts")).toHaveLength(0);
  expect(wire.requests("agents.create")).toHaveLength(0);
});

test("invalid saved profile falls back and a known failure starts a fresh request identity", async () => {
  const { wire } = await dialog(projects, "removed");
  await userEvent.type(screen.getByLabelText("제목"), "Retry");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  const first = wire.request("agents.create");
  expect((first.params as Record<string, unknown>).profile).toBe("claude");
  await act(async () => wire.deliver({ type: "response", id: first.id, error: { code: "agents.title_invalid" } }));
  expect(await screen.findByRole("alert")).toHaveTextContent("제목");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(2));
  expect(wire.request("agents.create", 1).clientRequestId).not.toBe(first.clientRequestId);
});

test("lost create response retains request identity after reconnect", async () => {
  const { wire, created } = await dialog();
  await userEvent.type(screen.getByLabelText("제목"), "Ambiguous");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  const first = wire.request("agents.create");
  await act(async () => wire.close());
  expect(await screen.findByRole("alert")).toHaveTextContent("실행 결과");
  await act(async () => wire.connect());
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(2));
  expect(wire.request("agents.create", 1).clientRequestId).toBe(first.clientRequestId);
  await act(async () => wire.reply("agents.create", task("new", "preparing")));
  expect(created).toHaveBeenCalledWith("new");
});

test("Ctrl+Enter while choosing a tool selects it without launching the previous profile", async () => {
  const { wire } = await dialog();
  await userEvent.type(screen.getByLabelText("제목"), "Choose tool");
  act(() => screen.getByRole("combobox", { name: "도구" }).focus());
  await userEvent.keyboard("{Enter}");
  await screen.findByRole("listbox");
  await userEvent.keyboard("{End}{Control>}{Enter}{/Control}");
  expect(wire.requests("agents.create")).toHaveLength(0);
  expect(screen.getByRole("combobox", { name: "도구" })).toHaveTextContent("Codex");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  expect((wire.request("agents.create").params as Record<string, unknown>).profile).toBe("codex");
});

test("a removed project is revalidated on refreshed lists and blocks stale creation", async () => {
  const { wire } = await dialog();
  await userEvent.type(screen.getByLabelText("제목"), "Removed project");
  const sub = wire.sent.find((m) => m.topic === "projects.changed")!;
  await act(async () => wire.deliver({ type: "event", subId: sub.id, rev: "2", payload: { rev: "2" } }));
  await waitFor(() => expect(wire.requests("projects.list")).toHaveLength(2));
  await act(async () => wire.reply("projects.list", { rev: "2", items: [projects[1]] }));
  expect(await screen.findByText("에이전트 작업을 만들 프로젝트를 선택해 주세요.")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "시작" })).toBeDisabled();
  expect(wire.requests("agents.create")).toHaveLength(0);
});

test("trust cancellation executes nothing and preview includes all commands and paths", async () => {
  const { wire } = await dialog(projects, "claude", true);
  await userEvent.type(screen.getByLabelText("제목"), "Trust commands");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("projects.trust_preview")).toHaveLength(1));
  await act(async () =>
    wire.reply(
      "projects.trust_preview",
      trustPreview("a", {
        config: {
          agent: { copy: [], setup: ["setup first", "setup second"], test: "test full", teardown: ["teardown full"] },
          launch: {
            services: [],
            terminals: [{ name: "shell", command: "terminal full" }],
            agents: [{ tool: "codex", worktree: true, prompt: "launch full prompt" }],
          },
        },
      }),
    ),
  );
  expect(await screen.findByRole("dialog", { name: "프로젝트 명령 실행 승인" })).toBeInTheDocument();
  for (const text of [
    "setup first",
    "setup second",
    "test full",
    "teardown full",
    "terminal full",
    "launch full prompt",
    "/fixture/devbox",
    "/fixture/devbox/.devbox/devbox.toml",
    "/fixture/devbox-agents/new",
  ])
    expect(screen.getByText(text)).toBeInTheDocument();
  expect(wire.requests("agents.create")).toHaveLength(0);
  await userEvent.click(screen.getByRole("button", { name: "승인 취소" }));
  expect(wire.requests("projects.trust_approve")).toHaveLength(0);
  expect(wire.requests("agents.create")).toHaveLength(0);
});

test("stale approval shows changed preview and requires another explicit approval", async () => {
  const { wire } = await dialog(projects, "claude", true);
  await userEvent.type(screen.getByLabelText("제목"), "Frozen title");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("projects.trust_preview")).toHaveLength(1));
  await act(async () => wire.reply("projects.trust_preview", trustPreview()));
  await userEvent.click(await screen.findByRole("button", { name: "명령 실행 승인" }));
  await waitFor(() => expect(wire.requests("projects.trust_approve")).toHaveLength(1));
  expect(wire.request("projects.trust_approve").params).toEqual({
    id: "a",
    expectedCanonicalPath: "/fixture/devbox",
    expectedDigest: "digest-first",
    expectedScopeVersion: 2,
  });
  await act(async () =>
    wire.deliver({
      type: "response",
      id: wire.request("projects.trust_approve").id,
      error: { code: "projects.trust_preview_stale" },
    }),
  );
  await waitFor(() => expect(wire.requests("projects.trust_preview")).toHaveLength(2));
  await act(async () =>
    wire.reply(
      "projects.trust_preview",
      trustPreview("a", {
        digest: "digest-second",
        config: {
          agent: { copy: [], setup: ["changed setup"], test: null, teardown: [] },
          launch: { services: [], terminals: [], agents: [] },
        },
      }),
    ),
  );
  expect(await screen.findByText("changed setup")).toBeInTheDocument();
  expect(screen.getByRole("alert")).toHaveTextContent("바뀌었습니다");
  expect(wire.requests("projects.trust_approve")).toHaveLength(1);
  expect(wire.requests("agents.create")).toHaveLength(0);
  await userEvent.click(screen.getByRole("button", { name: "명령 실행 승인" }));
  await waitFor(() => expect(wire.requests("projects.trust_approve")).toHaveLength(2));
  expect(wire.request("projects.trust_approve", -1).params).toMatchObject({ expectedDigest: "digest-second" });
  await act(async () => wire.reply("projects.trust_approve", { rev: "2" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  expect(wire.request("agents.create").params).toMatchObject({ title: "Frozen title" });
});

test("cancel during approval and a late acknowledgement never creates a task", async () => {
  const { wire } = await dialog(projects, "claude", true);
  await userEvent.type(screen.getByLabelText("제목"), "Cancel pending approval");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("projects.trust_preview")).toHaveLength(1));
  await act(async () => wire.reply("projects.trust_preview", trustPreview()));
  await userEvent.click(await screen.findByRole("button", { name: "명령 실행 승인" }));
  await waitFor(() => expect(wire.requests("projects.trust_approve")).toHaveLength(1));
  await userEvent.click(screen.getByRole("button", { name: "승인 취소" }));
  await act(async () => wire.reply("projects.trust_approve", { rev: "2" }));
  expect(wire.requests("agents.create")).toHaveLength(0);
  expect(screen.queryByRole("dialog", { name: "프로젝트 명령 실행 승인" })).toBeNull();
});

test("unmount cancels a pending preview before its late approval-free reply", async () => {
  const { wire, view } = await dialog(projects, "claude", true);
  await userEvent.type(screen.getByLabelText("제목"), "Close preview");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("projects.trust_preview")).toHaveLength(1));
  view.unmount();
  await act(async () => wire.reply("projects.trust_preview", trustPreview("a", { requiresApproval: false })));
  expect(wire.requests("agents.create")).toHaveLength(0);
});

test("removing the target while preview is pending cancels the frozen creation", async () => {
  const { wire } = await dialog(projects, "claude", true);
  await userEvent.type(screen.getByLabelText("제목"), "Removed during trust");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("projects.trust_preview")).toHaveLength(1));
  const sub = wire.sent.find((message) => message.topic === "projects.changed")!;
  await act(async () => wire.deliver({ type: "event", subId: sub.id, rev: "2", payload: { rev: "2" } }));
  await waitFor(() => expect(wire.requests("projects.list")).toHaveLength(2));
  await act(async () => wire.reply("projects.list", { rev: "2", items: [projects[1]] }));
  await act(async () => wire.reply("projects.trust_preview", trustPreview("a", { requiresApproval: false })));
  expect(wire.requests("agents.create")).toHaveLength(0);
  expect(screen.queryByRole("dialog", { name: "프로젝트 명령 실행 승인" })).toBeNull();
});

test("a note draft fills the prompt without sending or bypassing the normal form", async () => {
  const { useAgentsUi } = await import("./ui");
  useAgentsUi.getState().setCreating(true, { prompt: "selected note text", truncated: true });
  try {
    const { wire } = await dialog();
    expect(screen.getByLabelText("지시문")).toHaveValue("selected note text");
    expect(screen.getByText("노트 내용이 길어 처음 64 KiB만 지시문에 넣었습니다.")).toBeVisible();
    expect(wire.requests("agents.create")).toHaveLength(0);
    expect(screen.getByRole("button", { name: "시작" })).toBeDisabled();
  } finally {
    useAgentsUi.getState().setCreating(false);
  }
});

const tool = () => screen.getByRole("combobox", { name: "도구" });
const branchField = () => screen.getByLabelText("브랜치 이름");
/** Opens a select and takes its last option (or its first, with `{Home}`). */
async function pick(name: string, key = "{End}") {
  act(() => screen.getByRole("combobox", { name }).focus());
  await userEvent.keyboard("{Enter}");
  await screen.findByRole("listbox");
  await userEvent.keyboard(`${key}{Enter}`);
}

test("remembers_the_last_tool_per_project", async () => {
  localStorage.setItem(profileKey("a"), "codex");
  const { wire } = await dialog(projects, null);
  expect(tool()).toHaveTextContent("Codex");
  // Project b has no memory: it falls back to the first tool, not to what project a used.
  await pick("프로젝트");
  expect(screen.getByRole("combobox", { name: "프로젝트" })).toHaveTextContent("family");
  expect(tool()).toHaveTextContent("Claude Code");
  await userEvent.type(screen.getByLabelText("제목"), "Other project");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  expect(wire.request("agents.create").params).toMatchObject({ projectId: "b", profile: "claude" });
  await act(async () => wire.reply("agents.create", task("new", "preparing")));
  // The tool is remembered for the project it started in, and only for that one.
  await waitFor(() => expect(localStorage.getItem(profileKey("b"))).toBe("claude"));
  expect(localStorage.getItem(profileKey("a"))).toBe("codex");
  expect(localStorage.getItem("devbox.lastProfile")).toBeNull();
});

test("a project without a memory of its own starts from the old global key, which is never written", async () => {
  localStorage.setItem("devbox.lastProfile", "codex");
  const { wire } = await dialog(projects, null);
  expect(tool()).toHaveTextContent("Codex");
  // Starting with another tool remembers it for this project and leaves the old key as it was.
  await pick("도구", "{Home}");
  expect(tool()).toHaveTextContent("Claude Code");
  await userEvent.type(screen.getByLabelText("제목"), "Old key");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  expect(wire.request("agents.create").params).toMatchObject({ projectId: "a", profile: "claude" });
  await act(async () => wire.reply("agents.create", task("new", "preparing")));
  await waitFor(() => expect(localStorage.getItem(profileKey("a"))).toBe("claude"));
  expect(localStorage.getItem("devbox.lastProfile")).toBe("codex");
});

test("a project's own memory of a removed tool does not fall back to the old global key", async () => {
  localStorage.setItem("devbox.lastProfile", "codex");
  await dialog(projects, "removed");
  expect(tool()).toHaveTextContent("Claude Code");
});

test("a tool picked in the dialog wins over the remembered one when the project changes", async () => {
  localStorage.setItem(profileKey("b"), "codex");
  await dialog(projects, "codex");
  expect(tool()).toHaveTextContent("Codex");
  await pick("도구", "{Home}");
  expect(tool()).toHaveTextContent("Claude Code");
  // Project b remembers Codex, but the tool chosen here is the user's own.
  await pick("프로젝트");
  expect(screen.getByRole("combobox", { name: "프로젝트" })).toHaveTextContent("family");
  expect(tool()).toHaveTextContent("Claude Code");
});

test("the branch name starts from the title, shows the fixed agent/ prefix and is not sent while unedited", async () => {
  const { wire } = await dialog();
  expect(branchField()).toHaveValue("");
  await userEvent.type(screen.getByLabelText("제목"), "Fix Login  bug!");
  expect(screen.getByText("agent/")).toBeVisible();
  expect(branchField()).toHaveValue("fix-login-bug");
  // The note under the form and the template's {{branch}} are about a name nobody edited: it may be numbered.
  expect(
    screen.getByText(/브랜치 이름을 직접 고치지 않으면 템플릿의 예상 브랜치는 이름이 겹칠 때 바뀔 수 있습니다/),
  ).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "템플릿: bugfix" }));
  expect(screen.getByLabelText("지시문")).toHaveValue("버그: Fix Login  bug!\nagent/fix-login-bug devbox {{other}}");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  // It is only the title's own name, so the daemon still makes it and numbers it when it is taken.
  expect(wire.request("agents.create").params).not.toHaveProperty("branch");
});

test("a typed name stops following the title and is sent as typed", async () => {
  const { wire } = await dialog();
  await userEvent.type(screen.getByLabelText("제목"), "Fix");
  await userEvent.clear(branchField());
  expect(screen.getByRole("button", { name: "시작" })).toBeEnabled();
  await userEvent.type(branchField(), "my-fix");
  await userEvent.type(screen.getByLabelText("제목"), " more words");
  expect(branchField()).toHaveValue("my-fix");
  // A template's {{branch}} is the branch the task will get.
  await userEvent.click(screen.getByRole("button", { name: "템플릿: bugfix" }));
  expect(screen.getByLabelText("지시문")).toHaveValue("버그: Fix more words\nagent/my-fix devbox {{other}}");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  expect(wire.request("agents.create").params).toMatchObject({ title: "Fix more words", branch: "my-fix" });
  await act(async () => wire.reply("agents.create", task("new", "preparing")));
});

test("the app omits branch when the field is empty, and the empty field shows the name that will be used", async () => {
  const { wire } = await dialog();
  await userEvent.type(screen.getByLabelText("제목"), "Fix Login bug");
  await userEvent.clear(branchField());
  expect(branchField()).toHaveValue("");
  expect(branchField()).toHaveAttribute("placeholder", "fix-login-bug");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  expect(wire.request("agents.create").params).not.toHaveProperty("branch");
});

test("a title without a name to follow leaves an empty field with no placeholder to promise one", async () => {
  await dialog();
  await userEvent.type(screen.getByLabelText("제목"), "로그인 수정");
  expect(branchField()).toHaveAttribute("placeholder", "");
});

test("an uppercase letter typed in the branch name becomes lowercase instead of an error", async () => {
  const { wire } = await dialog();
  await userEvent.type(screen.getByLabelText("제목"), "Fix");
  await userEvent.clear(branchField());
  await userEvent.type(branchField(), "Fix-Login");
  expect(branchField()).toHaveValue("fix-login");
  expect(branchField()).toBeValid();
  expect(screen.getByRole("button", { name: "시작" })).toBeEnabled();
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  expect(wire.request("agents.create").params).toMatchObject({ branch: "fix-login" });
});

test("only ASCII capitals are lowercased, and the caret stays where the letter was typed", async () => {
  await dialog();
  await userEvent.type(screen.getByLabelText("제목"), "Fix");
  await userEvent.clear(branchField());
  await userEvent.type(branchField(), "ab-cd");
  // A capital typed in the middle keeps the caret after it (the field's value is rewritten).
  await userEvent.type(branchField(), "X", { initialSelectionStart: 2, initialSelectionEnd: 2 });
  expect(branchField()).toHaveValue("abx-cd");
  expect((branchField() as HTMLInputElement).selectionStart).toBe(3);
  // Other letters are not folded into ASCII: they are still named as the problem.
  await userEvent.clear(branchField());
  await userEvent.type(branchField(), "Ä");
  expect(branchField()).toHaveValue("Ä");
  expect(branchField()).toBeInvalid();
});

test("korean_title_requires_a_branch_name", async () => {
  const { wire } = await dialog();
  await userEvent.type(screen.getByLabelText("제목"), "로그인 수정");
  expect(branchField()).toHaveValue("");
  expect(screen.getByRole("button", { name: "시작" })).toBeDisabled();
  expect(screen.getByText(/영문이나 숫자가 없어/)).toBeInTheDocument();
  await userEvent.type(branchField(), "fix-login");
  expect(screen.queryByText(/영문이나 숫자가 없어/)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "시작" })).toBeEnabled();
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  expect(wire.request("agents.create").params).toEqual({
    projectId: "a",
    title: "로그인 수정",
    profile: "claude",
    baseBranch: null,
    worktree: true,
    prompt: "",
    branch: "fix-login",
  });
});

test.each([
  ["Bad Name", /영문 소문자, 숫자, 하이픈/],
  ["fix--login", /영문 소문자, 숫자, 하이픈/],
  ["-fix", /영문 소문자, 숫자, 하이픈/],
  ["agent/fix", /영문 소문자, 숫자, 하이픈/],
  ["x".repeat(41), /40자/],
])("an invalid branch name (%s) explains itself and cannot be started", async (name, message) => {
  const { wire } = await dialog();
  await userEvent.type(screen.getByLabelText("제목"), "Fix");
  await userEvent.clear(branchField());
  await userEvent.type(branchField(), name);
  expect(screen.getByText(message)).toBeInTheDocument();
  expect(branchField()).toBeInvalid();
  expect(screen.getByRole("button", { name: "시작" })).toBeDisabled();
  fireEvent.keyDown(branchField(), { key: "Enter", ctrlKey: true });
  expect(wire.requests("agents.create")).toHaveLength(0);
  await userEvent.clear(branchField());
  expect(screen.getByRole("button", { name: "시작" })).toBeEnabled();
});

test("a taken name is explained with the fixed code message and the typed name stays editable", async () => {
  const { wire } = await dialog();
  await userEvent.type(screen.getByLabelText("제목"), "Fix");
  await userEvent.type(branchField(), "-taken");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  const first = wire.request("agents.create");
  // An edited name is asked for exactly, so a taken one is refused instead of numbered.
  expect(first.params).toMatchObject({ branch: "fix-taken" });
  await act(async () =>
    wire.deliver({
      type: "response",
      id: first.id,
      error: { code: "agents.branch_exists", detail: { branch: "agent/fix-taken" } },
    }),
  );
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("같은 이름의 브랜치가 이미 있습니다.");
  expect(alert).toHaveTextContent("브랜치 이름을 바꿔");
  expect(branchField()).toHaveValue("fix-taken");
  await userEvent.type(branchField(), "-2");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(2));
  expect(wire.request("agents.create", 1).params).toMatchObject({ branch: "fix-taken-2" });
  expect(wire.request("agents.create", 1).clientRequestId).not.toBe(first.clientRequestId);
});

test("without a work folder there is no branch to name, so the field is hidden and nothing is sent", async () => {
  const { wire } = await dialog();
  await userEvent.type(screen.getByLabelText("제목"), "Fix");
  await userEvent.clear(branchField());
  await userEvent.type(branchField(), "my-fix");
  await userEvent.click(screen.getByLabelText("작업 폴더 만들기"));
  expect(screen.queryByLabelText("브랜치 이름")).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  expect(wire.request("agents.create").params).toMatchObject({ worktree: false });
  expect(wire.request("agents.create").params).not.toHaveProperty("branch");
});
