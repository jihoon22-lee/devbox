import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it } from "vitest";
import type { CollectionRoot } from "../../../rpc/gen/rpc";
import { ApiBackendProvider } from "./backend";
import { EnvEditor, resetEnvDrafts } from "./EnvEditor";
import { emptyDraft, setAuthKind, updateAuth } from "./model/draft";
import { rootKey, useApiTabs } from "./store";
import { FakeBackend } from "./test-backend";
import { useSend } from "./useSend";

const a = { kind: "project", projectId: "owned-a" } as const;
const b = { kind: "personal" } as const;
const env = (name: string) =>
  JSON.stringify({
    version: 1,
    name,
    variables: [
      { name: "baseUrl", value: `https://${name}.invalid` },
      { name: "credential", value: `${name}-credential` },
    ],
  });
afterEach(() => {
  cleanup();
  resetEnvDrafts();
  useApiTabs.getState().reset();
});
function Sender({ id }: { id: string }) {
  const sender = useSend();
  return (
    <button type="button" onClick={() => void sender.send(id)}>
      선택 환경으로 보내기
    </button>
  );
}
function setup(sendId?: string) {
  const backend = new FakeBackend();
  backend.seed(a, "env/a.json", env("a"));
  backend.seed(b, "env/b.json", env("b"));
  const view = (root: CollectionRoot) => (
    <ApiBackendProvider backend={backend}>
      <EnvEditor key={rootKey(root)} root={root} />
      {sendId && <Sender id={sendId} />}
    </ApiBackendProvider>
  );
  const rendered = render(view(a));
  return { backend, switchRoot: (root: CollectionRoot) => rendered.rerender(view(root)) };
}
async function select(name: string) {
  await screen.findByRole("option", { name });
  await userEvent.setup().selectOptions(screen.getByLabelText("환경"), `env/${name}.json`);
  await waitFor(() => expect(screen.getByLabelText("변수 1 값")).toHaveValue(`https://${name}.invalid`));
}
const active = () => useApiTabs.getState().environment;

it("root with no selection clears credentials before the real send pipeline can use them", async () => {
  useApiTabs.getState().openNew(b);
  const id = useApiTabs.getState().tabs[0]!.id;
  useApiTabs.getState().setDraftById(
    id,
    updateAuth(setAuthKind({ ...emptyDraft(), url: "{{baseUrl}}/path" }, "bearer"), "bearer", {
      token: "{{credential}}",
    }),
  );
  const { backend, switchRoot } = setup(id);
  await select("a");
  await userEvent.setup().click(screen.getByRole("button", { name: "검토하고 사용" }));
  expect(active()?.variables[0]?.value).toBe("https://a.invalid");
  switchRoot(b);
  expect(screen.getByLabelText("환경")).toHaveValue("");
  expect(active()).toBeNull();
  expect(backend.send).not.toHaveBeenCalled();
  await userEvent.setup().click(screen.getByRole("button", { name: "선택 환경으로 보내기" }));
  expect(backend.send).toHaveBeenCalledOnce();
  expect(backend.send.mock.calls[0]![0].env.variables).toEqual([]);
  expect(JSON.stringify(backend.send.mock.calls[0]![0])).not.toContain("a-credential");
  await select("b");
  expect(backend.send).toHaveBeenCalledOnce();
  await userEvent.setup().click(screen.getByRole("button", { name: "선택 환경으로 보내기" }));
  expect(backend.send).toHaveBeenCalledTimes(2);
  const sent = backend.send.mock.calls[1]![0];
  expect(sent.env.variables).toEqual(
    expect.arrayContaining([
      expect.objectContaining({ name: "baseUrl", value: "https://b.invalid" }),
      expect.objectContaining({ name: "credential", value: "b-credential" }),
    ]),
  );
  expect(JSON.stringify(sent)).not.toContain("a-credential");
});

it("root roundtrip restores dirty values and the review bit that the controls display", async () => {
  const { backend, switchRoot } = setup();
  const user = userEvent.setup();
  await select("a");
  await user.type(screen.getByLabelText("변수 1 값"), "/dirty");
  switchRoot(b);
  await select("b");
  expect(active()?.unreviewed).toBe(false);
  switchRoot(a);
  expect(screen.getByLabelText("변수 1 값")).toHaveValue("https://a.invalid/dirty");
  expect(screen.getByLabelText("저장하지 않은 변경")).toBeInTheDocument();
  expect(active()).toMatchObject({
    unreviewed: true,
    variables: [
      { name: "baseUrl", value: "https://a.invalid/dirty" },
      { name: "credential", value: "a-credential" },
    ],
  });
  switchRoot(b);
  expect(screen.getByLabelText("변수 1 값")).toHaveValue("https://b.invalid");
  expect(active()?.variables[0]?.value).toBe("https://b.invalid");
  expect(backend.send).not.toHaveBeenCalled();
  expect(backend.write).not.toHaveBeenCalled();
});

it.each(["read", "review", "save"] as const)("late old-root %s updates only its saved draft", async (operation) => {
  const { backend, switchRoot } = setup();
  const user = userEvent.setup();
  switchRoot(b);
  await select("b");
  switchRoot(a);
  let release!: () => void;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  if (operation === "read") {
    const original = backend.read.getMockImplementation()!;
    backend.read.mockImplementationOnce(async (...args) => {
      await held;
      return original(...args);
    });
    await screen.findByRole("option", { name: "a" });
    await user.selectOptions(screen.getByLabelText("환경"), "env/a.json");
  } else {
    await select("a");
    if (operation === "review") {
      const original = backend.review.getMockImplementation()!;
      backend.review.mockImplementationOnce(async (...args) => {
        await held;
        return original(...args);
      });
      await user.click(screen.getByRole("button", { name: "검토하고 사용" }));
    } else {
      await user.type(screen.getByLabelText("변수 1 값"), "/saved");
      const original = backend.write.getMockImplementation()!;
      backend.write.mockImplementationOnce(async (...args) => {
        await held;
        return original(...args);
      });
      await user.click(screen.getByRole("button", { name: "환경 저장" }));
    }
  }
  switchRoot(b);
  await act(async () => {
    release();
    await held;
  });
  expect(screen.getByLabelText("환경")).toHaveValue("env/b.json");
  expect(screen.getByLabelText("변수 1 값")).toHaveValue("https://b.invalid");
  expect(active()).toMatchObject({
    name: "b",
    unreviewed: false,
    variables: [
      { name: "baseUrl", value: "https://b.invalid" },
      { name: "credential", value: "b-credential" },
    ],
  });
  expect(backend.send).not.toHaveBeenCalled();
  switchRoot(a);
  expect(screen.getByLabelText("변수 1 값")).toHaveValue(
    operation === "save" ? "https://a.invalid/saved" : "https://a.invalid",
  );
  expect(active()?.unreviewed).toBe(operation === "read");
});
