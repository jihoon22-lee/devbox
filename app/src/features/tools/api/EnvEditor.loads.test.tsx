import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it } from "vitest";
import type { CollectionRoot } from "../../../rpc/gen/rpc";
import { ApiBackendProvider } from "./backend";
import { EnvEditor, resetEnvDrafts } from "./EnvEditor";
import { useApiTabs } from "./store";
import { FakeBackend } from "./test-backend";

const personal = { kind: "personal" } as const;
const project = { kind: "project", projectId: "owned-other" } as const;
const env = (name: string) =>
  JSON.stringify({ version: 1, name, variables: [{ name: "baseUrl", value: `https://${name}.invalid` }] });
const active = () => useApiTabs.getState().environment;
const input = () => screen.getByLabelText("변수 1 값");
const selection = () => screen.getByLabelText("환경");
afterEach(() => {
  cleanup();
  resetEnvDrafts();
  useApiTabs.getState().reset();
});
function setup() {
  const backend = new FakeBackend();
  for (const name of ["a", "b"]) backend.seed(personal, `env/${name}.json`, env(name));
  backend.seed(project, "env/c.json", env("c"));
  const view = (root: CollectionRoot = personal) => (
    <ApiBackendProvider backend={backend}>
      <EnvEditor root={root} />
    </ApiBackendProvider>
  );
  return { backend, view, ...render(view()), user: userEvent.setup() };
}
async function choose(name: string) {
  await screen.findByRole("option", { name });
  await userEvent.setup().selectOptions(selection(), `env/${name}.json`);
}
async function loaded(name: string) {
  await choose(name);
  await waitFor(() => expect(input()).toHaveValue(`https://${name}.invalid`));
}
function gate() {
  let release!: () => void;
  const wait = new Promise<void>((resolve) => {
    release = resolve;
  });
  return {
    wait,
    finish: () =>
      act(async () => {
        release();
        await wait;
      }),
  };
}
function holdRead(backend: FakeBackend, failure = false) {
  const held = gate();
  const original = backend.read.getMockImplementation()!;
  backend.read.mockImplementationOnce(async (...args) => {
    const result = await original(...args);
    await held.wait;
    if (failure) throw new Error("owned read failure");
    return result;
  });
  return held;
}

it("a late same-root environment read cannot discard newer edits or publish its environment", async () => {
  const { backend, user } = setup();
  const pending = holdRead(backend);
  await choose("a");
  await loaded("b");
  await user.type(input(), "/unsaved");
  await pending.finish();
  expect(input()).toHaveValue("https://b.invalid/unsaved");
  expect(selection()).toHaveValue("env/b.json");
  expect(screen.getByLabelText("저장하지 않은 변경")).toBeInTheDocument();
  expect(active()).toMatchObject({ name: "b", variables: [{ name: "baseUrl", value: "https://b.invalid/unsaved" }] });
  expect(backend.write).not.toHaveBeenCalled();
});

it("latest selection wins even when neither pending read has changed the draft yet", async () => {
  const { backend } = setup();
  const first = holdRead(backend);
  const second = holdRead(backend);
  await choose("a");
  await choose("b");
  await first.finish();
  expect(active()).toBeNull();
  expect(screen.queryByLabelText("변수 1 값")).not.toBeInTheDocument();
  await second.finish();
  expect(input()).toHaveValue("https://b.invalid");
  expect(active()?.name).toBe("b");
});

it.each([false, true])("new draft survives a pending read (read failure: %s)", async (failure) => {
  const { backend, user } = setup();
  const pending = holdRead(backend, failure);
  await choose("a");
  await user.click(screen.getByRole("button", { name: "새 환경" }));
  await user.type(screen.getByLabelText("환경 이름"), " owned");
  await pending.finish();
  expect(screen.getByLabelText("환경 이름")).toHaveValue("새 환경 owned");
  expect(screen.getByLabelText("저장하지 않은 변경")).toBeInTheDocument();
  expect(active()?.name).toBe("새 환경 owned");
  expect(screen.queryByText("환경 파일을 읽지 못했습니다.")).not.toBeInTheDocument();
});

it("editing during a same-selection reload invalidates the response", async () => {
  const { backend, user } = setup();
  await loaded("a");
  const pending = holdRead(backend);
  await choose("a");
  await user.type(input(), "/newer");
  await pending.finish();
  expect(input()).toHaveValue("https://a.invalid/newer");
  expect(screen.getByLabelText("저장하지 않은 변경")).toBeInTheDocument();
  expect(active()?.variables[0]?.value).toBe("https://a.invalid/newer");
});

it("a stale failure cannot replace the current successful selection with an error", async () => {
  const { backend } = setup();
  const pending = holdRead(backend, true);
  await choose("a");
  await loaded("b");
  await pending.finish();
  expect(input()).toHaveValue("https://b.invalid");
  expect(screen.queryByText("환경 파일을 읽지 못했습니다.")).not.toBeInTheDocument();
});

it("an empty selection invalidates pending reads without writing a draft", async () => {
  const { backend, user } = setup();
  const pending = holdRead(backend);
  await choose("a");
  await user.selectOptions(selection(), "");
  await pending.finish();
  expect(selection()).toHaveValue("");
  expect(active()).toBeNull();
  expect(screen.queryByLabelText("변수 1 값")).not.toBeInTheDocument();
});

it("root change without remount invalidates reads even after returning to that root", async () => {
  const { backend, view, rerender } = setup();
  const pending = holdRead(backend);
  await choose("a");
  rerender(view(project));
  await loaded("c");
  await pending.finish();
  expect(active()?.name).toBe("c");
  rerender(view());
  expect(selection()).toHaveValue("");
  expect(active()).toBeNull();
});

it("unmounted read cannot modify the draft restored by a new editor", async () => {
  const { backend, view, unmount } = setup();
  const pending = holdRead(backend);
  await choose("a");
  unmount();
  render(view());
  await pending.finish();
  expect(selection()).toHaveValue("");
  expect(active()).toBeNull();
});

it("a completed save invalidates an earlier reload and retains the saved baseline", async () => {
  const { backend, user } = setup();
  await loaded("a");
  const pending = holdRead(backend);
  await choose("a");
  await user.type(input(), "/saved");
  await user.click(screen.getByRole("button", { name: "환경 저장" }));
  await screen.findByText("환경을 저장했습니다.");
  await pending.finish();
  expect(input()).toHaveValue("https://a.invalid/saved");
  expect(screen.queryByLabelText("저장하지 않은 변경")).not.toBeInTheDocument();
  expect(backend.text(personal, "env/a.json")).toContain("https://a.invalid/saved");
});

it("typing during save remains dirty while only the captured content is saved", async () => {
  const { backend, user } = setup();
  await loaded("a");
  await user.type(input(), "/saved");
  const pending = gate();
  const original = backend.write.getMockImplementation()!;
  backend.write.mockImplementationOnce(async (...args) => {
    await pending.wait;
    return original(...args);
  });
  await user.click(screen.getByRole("button", { name: "환경 저장" }));
  await user.type(input(), "/later");
  await pending.finish();
  expect(input()).toHaveValue("https://a.invalid/saved/later");
  expect(screen.getByLabelText("저장하지 않은 변경")).toBeInTheDocument();
  expect(backend.text(personal, "env/a.json")).toContain("https://a.invalid/saved");
  expect(backend.text(personal, "env/a.json")).not.toContain("/later");
});

it("the current read failure is visible and a subsequent successful load clears it", async () => {
  const { backend } = setup();
  const pending = holdRead(backend, true);
  await choose("a");
  await pending.finish();
  expect(screen.getByText("환경 파일을 읽지 못했습니다.")).toBeInTheDocument();
  await loaded("b");
  expect(screen.queryByText("환경 파일을 읽지 못했습니다.")).not.toBeInTheDocument();
});

it("changing roots dismisses an old dirty-discard action without discarding that draft", async () => {
  const { user, view, rerender } = setup();
  await loaded("a");
  await user.type(input(), "/dirty");
  await choose("b");
  expect(screen.getByRole("dialog", { name: "환경 변경을 버릴까요?" })).toBeInTheDocument();
  rerender(view(project));
  expect(screen.queryByRole("dialog", { name: "환경 변경을 버릴까요?" })).not.toBeInTheDocument();
  await loaded("c");
  rerender(view());
  expect(input()).toHaveValue("https://a.invalid/dirty");
  expect(screen.getByLabelText("저장하지 않은 변경")).toBeInTheDocument();
  expect(active()?.name).toBe("a");
});
