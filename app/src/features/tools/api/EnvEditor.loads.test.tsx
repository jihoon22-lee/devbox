import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
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

/** Holds the next write until the returned gate finishes, so a test can act while a save is in flight. */
function holdWrite(backend: FakeBackend) {
  const held = gate();
  const original = backend.write.getMockImplementation()!;
  backend.write.mockImplementationOnce(async (...args) => {
    await held.wait;
    return original(...args);
  });
  return held;
}
const discardAndCreate = async (user: ReturnType<typeof userEvent.setup>) => {
  await user.click(screen.getByRole("button", { name: "새 환경" }));
  const ask = await screen.findByRole("dialog", { name: "환경 변경을 버릴까요?" });
  await user.click(within(ask).getByRole("button", { name: "변경 버리기" }));
};
const rename = async (user: ReturnType<typeof userEvent.setup>, name: string) => {
  await user.clear(screen.getByLabelText("환경 이름"));
  await user.type(screen.getByLabelText("환경 이름"), name);
};

it("late_save_of_a_does_not_overwrite_new_env_b_draft", async () => {
  const { backend, user } = setup();
  await user.click(screen.getByRole("button", { name: "새 환경" }));
  await rename(user, "alpha");
  const pending = holdWrite(backend);
  await user.click(screen.getByRole("button", { name: "환경 저장" }));
  // A is still being written when the user drops it and starts another new environment.
  await discardAndCreate(user);
  await rename(user, "beta");
  await pending.finish();
  // The notice is about alpha, which is not what the editor shows: it must not read as B's.
  await screen.findByText("alpha 환경을 저장했습니다.");
  expect(screen.queryByText("환경을 저장했습니다.")).not.toBeInTheDocument();
  expect(backend.text(personal, "env/alpha.json")).toContain('"name": "alpha"');
  // B is still a new, unsaved draft: A's file, digest and baseline did not land on it.
  expect(screen.getByLabelText("환경 이름")).toHaveValue("beta");
  expect(selection()).toHaveValue("");
  expect(screen.getByLabelText("저장하지 않은 변경")).toBeInTheDocument();
  expect(active()?.name).toBe("beta");
  // Saving B creates its own file instead of replacing A's.
  await user.click(screen.getByRole("button", { name: "환경 저장" }));
  await waitFor(() => expect(backend.text(personal, "env/beta.json")).toContain('"name": "beta"'));
  expect(backend.text(personal, "env/alpha.json")).toContain('"name": "alpha"');
  expect(backend.write).toHaveBeenCalledTimes(2);
  expect(backend.write.mock.calls[1]![1]).toBe("env/beta.json");
  expect(backend.write.mock.calls[1]![3]).toEqual({ kind: "absent" });
});

it("a late save does not touch a draft that was read again from the same file", async () => {
  const { backend, user } = setup();
  await loaded("a");
  await user.type(input(), "/saved");
  const pending = holdWrite(backend);
  await user.click(screen.getByRole("button", { name: "환경 저장" }));
  // Another environment, then the same file again: the new draft is a fresh read of env/a.json.
  await choose("b");
  await user.click(
    within(await screen.findByRole("dialog", { name: "환경 변경을 버릴까요?" })).getByRole("button", {
      name: "변경 버리기",
    }),
  );
  await waitFor(() => expect(input()).toHaveValue("https://b.invalid"));
  await loaded("a");
  expect(input()).toHaveValue("https://a.invalid");
  await pending.finish();
  await screen.findByText("a 환경을 저장했습니다.");
  // The reread draft keeps what it read; the save's text is not claimed as its baseline.
  expect(input()).toHaveValue("https://a.invalid");
  expect(screen.queryByLabelText("저장하지 않은 변경")).not.toBeInTheDocument();
  expect(backend.text(personal, "env/a.json")).toContain("https://a.invalid/saved");
});

it("an edit made while a save is in flight still receives the saved digest for the next save", async () => {
  const { backend, user } = setup();
  await loaded("a");
  await user.type(input(), "/first");
  const pending = holdWrite(backend);
  await user.click(screen.getByRole("button", { name: "환경 저장" }));
  await user.type(input(), "/second");
  await pending.finish();
  await screen.findByText("환경을 저장했습니다.");
  expect(input()).toHaveValue("https://a.invalid/first/second");
  expect(screen.getByLabelText("저장하지 않은 변경")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "환경 저장" }));
  await waitFor(() => expect(screen.queryByLabelText("저장하지 않은 변경")).not.toBeInTheDocument());
  expect(backend.text(personal, "env/a.json")).toContain("https://a.invalid/first/second");
  expect(backend.write).toHaveBeenCalledTimes(2);
  expect(screen.queryByText("환경 형식이 올바르지 않습니다.")).not.toBeInTheDocument();
});

const addVariable = async (user: ReturnType<typeof userEvent.setup>, name: string, value: string) => {
  await user.click(screen.getByRole("button", { name: "변수 추가" }));
  await user.type(screen.getByLabelText("변수 1 이름"), name);
  await user.type(screen.getByLabelText("변수 1 값"), value);
};

it("secret values left out of a saved file are reported to the draft they came from", async () => {
  const { backend, user } = setup();
  await user.click(screen.getByRole("button", { name: "새 환경" }));
  await rename(user, "alpha");
  await addVariable(user, "API_TOKEN", "plain-value");
  await user.click(screen.getByRole("button", { name: "환경 저장" }));
  expect(
    await screen.findByText("비밀 값 1개는 파일에 저장하지 않았습니다: API_TOKEN. {{secret:이름}} 참조로 바꿔 주세요."),
  ).toBeInTheDocument();
  expect(backend.text(personal, "env/alpha.json")).toContain('"secret": true');
});

it("a late save that left secret values out names its environment instead of speaking to another draft", async () => {
  const { backend, user } = setup();
  await user.click(screen.getByRole("button", { name: "새 환경" }));
  await rename(user, "alpha");
  await addVariable(user, "API_TOKEN", "plain-value");
  const pending = holdWrite(backend);
  await user.click(screen.getByRole("button", { name: "환경 저장" }));
  await discardAndCreate(user);
  await rename(user, "beta");
  await pending.finish();
  // Replacing them with references means opening alpha; beta has no such values to replace.
  expect(
    await screen.findByText(
      "alpha 환경의 비밀 값 1개는 파일에 저장하지 않았습니다: API_TOKEN. 그 환경을 열어 {{secret:이름}} 참조로 바꿔 주세요.",
    ),
  ).toBeInTheDocument();
  expect(screen.getByLabelText("환경 이름")).toHaveValue("beta");
  expect(backend.text(personal, "env/alpha.json")).toContain('"secret": true');
});
