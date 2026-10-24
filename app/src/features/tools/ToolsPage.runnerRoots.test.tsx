import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, expect, it } from "vitest";
import type { CollectionRoot } from "../../rpc/gen/rpc";
import { ApiBackendProvider } from "./api/backend";
import { resetEnvDrafts } from "./api/EnvEditor";
import { emptyDraft, setAuthKind, updateAuth } from "./api/model/draft";
import { fileFromDraft } from "./api/model/fileDraft";
import { useApiTabs } from "./api/store";
import { FakeBackend, textResponse } from "./api/test-backend";
import type { ToolsSearch } from "./search";
import { ToolsView } from "./ToolsPage";

const project = { kind: "project", projectId: "owned-project" } as const;
const personal = { kind: "personal" } as const;
const name = (root: CollectionRoot) => root.kind;
const credential = (root: CollectionRoot) => `OWNED_${name(root).toUpperCase()}_ONLY_CREDENTIAL`;
const endpoint = (root: CollectionRoot) => `https://${name(root)}-fixture.invalid/receive`;
const active = () => useApiTabs.getState().environment;
afterEach(() => {
  cleanup();
  resetEnvDrafts();
  useApiTabs.getState().reset();
});

async function setup(tabRoot: CollectionRoot = project) {
  const backend = new FakeBackend();
  for (const root of [personal, project]) {
    const draft = updateAuth(
      setAuthKind({ ...emptyDraft(), method: "POST", url: endpoint(root) }, "bearer"),
      "bearer",
      { token: "{{credential}}" },
    );
    const digest = backend.seed(
      root,
      "receive.request.json",
      JSON.stringify(fileFromDraft(draft, `${name(root)} receiver`)),
    );
    if (root.kind === "project") await backend.review(root, "receive.request.json", digest);
    backend.seed(
      root,
      `env/${name(root)}.json`,
      JSON.stringify({ version: 1, name: name(root), variables: [{ name: "credential", value: credential(root) }] }),
    );
  }
  useApiTabs.getState().openNew(tabRoot);
  let navigate!: (search: ToolsSearch) => void;
  function Harness() {
    const [search, setSearch] = useState<ToolsSearch>({ view: "api" });
    navigate = setSearch;
    return (
      <ApiBackendProvider backend={backend}>
        <ToolsView projectId={project.projectId} search={search} navigate={setSearch} />
      </ApiBackendProvider>
    );
  }
  render(<Harness />);
  const user = userEvent.setup();
  await user.click(screen.getByText("환경", { selector: "summary" }));
  return { backend, user, route: (search: ToolsSearch) => act(() => navigate(search)) };
}
async function select(root: CollectionRoot, review = true) {
  const user = userEvent.setup();
  await screen.findByRole("option", { name: name(root) });
  await user.selectOptions(screen.getByRole("combobox", { name: "환경" }), `env/${name(root)}.json`);
  await waitFor(() => expect(screen.getByLabelText("변수 1 값")).toHaveValue(credential(root)));
  if (root.kind === "project" && review) {
    await user.click(screen.getByRole("button", { name: "검토하고 사용" }));
    await waitFor(() => expect(active()?.unreviewed).toBe(false));
  }
}
async function runner(root: CollectionRoot) {
  await userEvent
    .setup()
    .click(
      screen.getByRole("button", { name: root.kind === "personal" ? "개인 컬렉션 실행" : "프로젝트 컬렉션 실행" }),
    );
  await within(screen.getByRole("region", { name: "본문" })).findByRole("listitem", {
    name: `1. ${name(root)} receiver`,
  });
}
const run = () =>
  userEvent.setup().click(
    within(screen.getByRole("region", { name: "본문" })).getByRole("button", {
      name: "실행",
    }),
  );

it.each([
  [project, personal],
  [personal, project],
] as const)("runner root overrides opposite active tab %j -> %j before actual send", async (source, target) => {
  const { backend } = await setup(source);
  await select(source);
  const tab = useApiTabs.getState().active;
  await runner(target);
  expect(backend.send).not.toHaveBeenCalled();
  expect(screen.getByRole("combobox", { name: "환경" })).toHaveValue("");
  expect(active()).toBeNull();
  await run();
  await screen.findByText("변수 없음: credential");
  expect(backend.send).not.toHaveBeenCalled();
  await select(target);
  await run();
  await waitFor(() => expect(backend.send).toHaveBeenCalledOnce());
  const sent = backend.send.mock.calls[0]![0];
  expect(sent.snapshot.url).toBe(endpoint(target));
  expect(sent.cookieJar).toBe(target.kind === "project" ? target.projectId : "personal");
  expect(sent.snapshot.auth).toEqual({ kind: "bearer", token: "{{credential}}" });
  expect(sent.env.variables).toContainEqual(expect.objectContaining({ name: "credential", value: credential(target) }));
  expect(JSON.stringify(sent)).not.toContain(credential(source));
  expect(useApiTabs.getState().active).toBe(tab);
});

it("runner route changes restore dirty root drafts, ignore active-tab changes, and return to the editor root", async () => {
  const { backend, user, route } = await setup(project);
  await select(project);
  await user.type(screen.getByLabelText("변수 1 값"), "-project-dirty");
  await runner(personal);
  await select(personal);
  await user.type(screen.getByLabelText("변수 1 값"), "-personal-dirty");
  await runner(project);
  expect(screen.getByLabelText("변수 1 값")).toHaveValue(`${credential(project)}-project-dirty`);
  act(() => {
    useApiTabs.getState().openNew(personal);
  });
  expect(screen.getByLabelText("변수 1 값")).toHaveValue(`${credential(project)}-project-dirty`);
  expect(backend.send).not.toHaveBeenCalled();
  await run();
  await waitFor(() => expect(backend.send).toHaveBeenCalledOnce());
  expect(backend.send.mock.calls[0]![0]).toMatchObject({
    snapshot: { url: endpoint(project) },
    env: { variables: [expect.objectContaining({ value: `${credential(project)}-project-dirty` })] },
  });
  route({ view: "api" });
  expect(screen.getByLabelText("변수 1 값")).toHaveValue(`${credential(personal)}-personal-dirty`);
  expect(
    within(screen.getByRole("complementary", { name: "목록" })).getByLabelText("저장하지 않은 변경"),
  ).toBeInTheDocument();
  expect(backend.write).not.toHaveBeenCalled();
});

it("undefined runner root is personal and selecting none stays cleared across root roundtrips", async () => {
  const { backend, user, route } = await setup(project);
  await select(project);
  route({ tool: "runner" });
  await within(screen.getByRole("region", { name: "본문" })).findByRole("listitem", {
    name: "1. personal receiver",
  });
  expect(active()).toBeNull();
  await select(personal);
  await user.selectOptions(screen.getByRole("combobox", { name: "환경" }), "");
  expect(active()).toBeNull();
  await runner(project);
  expect(active()?.variables[0]?.value).toBe(credential(project));
  route({ view: "api", tool: "runner" });
  await within(screen.getByRole("region", { name: "본문" })).findByRole("listitem", {
    name: "1. personal receiver",
  });
  expect(active()).toBeNull();
  expect(backend.send).not.toHaveBeenCalled();
});

it("project runner environment needs its own explicit review before it can reach send", async () => {
  const { backend, user } = await setup(personal);
  await select(personal);
  await runner(project);
  await select(project, false);
  await run();
  await screen.findByText("비밀 검토가 필요합니다", { exact: false });
  expect(backend.send).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "검토하고 사용" }));
  await waitFor(() => expect(active()?.unreviewed).toBe(false));
  await run();
  await waitFor(() => expect(backend.send).toHaveBeenCalledOnce());
  const sent = backend.send.mock.calls[0]![0];
  expect(sent.snapshot.url).toBe(endpoint(project));
  expect(sent.env.variables).toContainEqual(
    expect.objectContaining({ name: "credential", value: credential(project) }),
  );
  expect(JSON.stringify(sent)).not.toContain(credential(personal));
});

it("late environment read from the old runner root cannot publish into the current runner", async () => {
  const { backend, user } = await setup(personal);
  await select(personal);
  await runner(project);
  let release!: () => void;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const read = backend.read.getMockImplementation()!;
  backend.read.mockImplementation(async (root, path) => {
    if (root.kind === "project" && path === "env/project.json") await held;
    return read(root, path);
  });
  await screen.findByRole("option", { name: "project" });
  await user.selectOptions(screen.getByRole("combobox", { name: "환경" }), "env/project.json");
  await runner(personal);
  await act(async () => {
    release();
    await held;
  });
  expect(active()?.variables[0]?.value).toBe(credential(personal));
  expect(screen.getByRole("combobox", { name: "환경" })).toHaveValue("env/personal.json");
  await run();
  await waitFor(() => expect(backend.send).toHaveBeenCalledOnce());
  expect(JSON.stringify(backend.send.mock.calls[0]![0])).not.toContain(credential(project));
  await runner(project);
  expect(screen.getByLabelText("변수 1 값")).toHaveValue(credential(project));
  expect(active()?.unreviewed).toBe(true);
});

it("switching runner roots cannot leave an old endpoint runnable with the new root environment", async () => {
  const { backend, route } = await setup(personal);
  await select(personal);
  await runner(project);
  let release!: () => void;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const list = backend.list.getMockImplementation()!;
  backend.list.mockImplementation(async (root) => {
    if (root.kind === "personal") await held;
    return list(root);
  });
  route({ view: "api", tool: "runner", root: "personal" });
  const body = within(screen.getByRole("region", { name: "본문" }));
  expect(body.queryByRole("listitem", { name: "1. project receiver" })).not.toBeInTheDocument();
  const button = body.queryByRole("button", { name: "실행" });
  if (button) expect(button).toBeDisabled();
  await act(async () => {
    release();
    await held;
  });
  await body.findByRole("listitem", { name: "1. personal receiver" });
  expect(backend.send).not.toHaveBeenCalled();
  await run();
  await waitFor(() => expect(backend.send).toHaveBeenCalledOnce());
  expect(backend.send.mock.calls[0]![0].snapshot.url).toBe(endpoint(personal));
});

it("changing a running collection aborts its send and rejects late results without auto-running the new root", async () => {
  const { backend } = await setup(project);
  const file = JSON.parse(backend.text(project, "receive.request.json")!);
  file.captures = [{ id: "owned-capture", enabled: true, variable: "late", source: "jsonPath", target: "$.token" }];
  const digest = backend.seed(project, "receive.request.json", JSON.stringify(file));
  await backend.review(project, "receive.request.json", digest);
  await select(project);
  await runner(project);
  let release!: () => void;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  backend.reply = async () => {
    await held;
    return textResponse('{"token":"OWNED_LATE_CAPTURE"}');
  };
  await run();
  await waitFor(() => expect(backend.send).toHaveBeenCalledOnce());
  const signal = backend.send.mock.calls[0]![1];
  expect(signal.aborted).toBe(false);
  await runner(personal);
  expect(signal.aborted).toBe(true);
  expect(backend.send).toHaveBeenCalledOnce();
  await act(async () => {
    release();
    await held;
  });
  expect(useApiTabs.getState().session.get("late")).toBeUndefined();
  const body = within(screen.getByRole("region", { name: "본문" }));
  expect(body.queryByRole("table", { name: "실행 결과" })).not.toBeInTheDocument();
  expect(body.queryByText("OWNED_LATE_CAPTURE")).not.toBeInTheDocument();
  expect(backend.send).toHaveBeenCalledOnce();
  backend.reply = async () => textResponse("{}");
  await select(personal);
  await run();
  await waitFor(() => expect(backend.send).toHaveBeenCalledTimes(2));
  expect(backend.send.mock.calls[1]![0].snapshot.url).toBe(endpoint(personal));
});
