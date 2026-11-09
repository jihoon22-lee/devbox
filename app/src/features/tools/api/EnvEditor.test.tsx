// 검토 3라운드 차단: 저장하지 않은 환경 편집이 확인 없이 사라지지 않는다.
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it } from "vitest";
import { ApiBackendProvider } from "./backend";
import { EnvEditor, resetEnvDrafts } from "./EnvEditor";
import { looksLikeSecret } from "./lib/secretPatterns";
import { useApiTabs } from "./store";
import { FakeBackend } from "./test-backend";

const personal = { kind: "personal" } as const;
const env = (name: string, value: string) =>
  `${JSON.stringify({ version: 1, name, variables: [{ name: "host", value }] }, null, 2)}\n`;
afterEach(() => {
  cleanup();
  resetEnvDrafts();
  useApiTabs.getState().reset();
});
function setup(seed?: (backend: FakeBackend) => void) {
  const backend = new FakeBackend();
  seed?.(backend);
  backend.seed(personal, "env/dev.json", env("dev", "dev.test"));
  backend.seed(personal, "env/prod.json", env("prod", "prod.test"));
  const view = (
    <ApiBackendProvider backend={backend}>
      <EnvEditor root={personal} />
    </ApiBackendProvider>
  );
  return { backend, ...render(view), view };
}
const dirty = () => screen.queryAllByLabelText("저장하지 않은 변경");

it("switching_environment_with_unsaved_edits_asks_first", async () => {
  const user = userEvent.setup();
  const { backend, unmount, view } = setup();
  await screen.findByRole("option", { name: "dev" });
  await user.selectOptions(screen.getByLabelText("환경"), "env/dev.json");
  await waitFor(() => expect(screen.getByLabelText("변수 1 값")).toHaveValue("dev.test"));
  expect(dirty()).toHaveLength(0);
  await user.type(screen.getByLabelText("변수 1 값"), "-edited");
  expect(dirty()).toHaveLength(1);

  // 다른 환경을 고르면 먼저 묻고, 취소하면 편집이 남는다.
  await user.selectOptions(screen.getByLabelText("환경"), "env/prod.json");
  let confirm = await screen.findByRole("dialog", { name: "환경 변경을 버릴까요?" });
  expect(confirm).toHaveTextContent("저장하지 않은 환경 변경은 사라집니다");
  await user.click(within(confirm).getByRole("button", { name: "취소" }));
  expect(screen.getByLabelText("변수 1 값")).toHaveValue("dev.test-edited");
  expect(screen.getByLabelText("환경")).toHaveValue("env/dev.json");

  // [새 환경]도 같은 확인을 거친다.
  await user.click(screen.getByRole("button", { name: "새 환경" }));
  confirm = await screen.findByRole("dialog", { name: "환경 변경을 버릴까요?" });
  await user.click(within(confirm).getByRole("button", { name: "취소" }));
  expect(screen.getByLabelText("변수 1 값")).toHaveValue("dev.test-edited");

  // 루트가 바뀌어 편집기가 다시 그려져도 편집 상태는 남는다.
  unmount();
  render(view);
  await waitFor(() => expect(screen.getByLabelText("변수 1 값")).toHaveValue("dev.test-edited"));
  expect(dirty()).toHaveLength(1);

  // 버리기를 고르면 그때 바꾼다.
  await user.selectOptions(screen.getByLabelText("환경"), "env/prod.json");
  confirm = await screen.findByRole("dialog", { name: "환경 변경을 버릴까요?" });
  await user.click(within(confirm).getByRole("button", { name: "변경 버리기" }));
  await waitFor(() => expect(screen.getByLabelText("변수 1 값")).toHaveValue("prod.test"));
  expect(dirty()).toHaveLength(0);
  expect(backend.write).not.toHaveBeenCalled();
});

it("environment_saves_do_not_overlap", async () => {
  const user = userEvent.setup();
  const { backend } = setup();
  await screen.findByRole("option", { name: "dev" });
  await user.selectOptions(screen.getByLabelText("환경"), "env/dev.json");
  await waitFor(() => expect(screen.getByLabelText("변수 1 값")).toHaveValue("dev.test"));
  await user.type(screen.getByLabelText("변수 1 값"), "-x");
  const write = backend.write.getMockImplementation()!;
  let release!: () => void;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  backend.write.mockImplementationOnce(async (...args) => {
    await held;
    return write(...args);
  });
  await user.click(screen.getByRole("button", { name: "환경 저장" }));
  await user.click(screen.getByRole("button", { name: "환경 저장" }));
  release();
  await waitFor(() => expect(screen.getByText("환경을 저장했습니다.")).toBeInTheDocument());
  expect(backend.write).toHaveBeenCalledOnce();
  expect(dirty()).toHaveLength(0);
});

it("unchecked_row_stays_unchecked_after_save_and_reopen", async () => {
  const user = userEvent.setup();
  const variables = [
    { name: "host", value: "prod.test" },
    { name: "host", value: "dev.test" },
  ];
  setup((backend) => backend.seed(personal, "env/dup.json", JSON.stringify({ version: 1, name: "dup", variables })));
  await screen.findByRole("option", { name: "dup" });
  await user.selectOptions(screen.getByLabelText("환경"), "env/dup.json");
  await waitFor(() => expect(screen.getByLabelText("변수 1 값")).toHaveValue("prod.test"));
  await user.click(screen.getByLabelText("변수 1 사용"));
  await user.click(screen.getByRole("button", { name: "환경 저장" }));
  await waitFor(() => expect(screen.getByText("환경을 저장했습니다.")).toBeInTheDocument());
  await user.selectOptions(screen.getByLabelText("환경"), "");
  await user.selectOptions(screen.getByLabelText("환경"), "env/dup.json");
  await waitFor(() => expect(screen.getByLabelText("변수 1 값")).toHaveValue("prod.test"));
  expect(screen.getByLabelText("변수 1 사용")).not.toBeChecked();
  expect(screen.getByLabelText("변수 2 사용")).toBeChecked();
  expect(useApiTabs.getState().environment?.variables).toEqual([{ name: "host", value: "dev.test" }]);
});

it("비밀 표시는 읽기와 저장을 지나도 남는다", async () => {
  const user = userEvent.setup();
  const { backend } = setup((b) =>
    b.seed(
      personal,
      "env/sec.json",
      JSON.stringify({ version: 1, name: "sec", variables: [{ name: "opaque", value: "", secret: true }] }),
    ),
  );
  await screen.findByRole("option", { name: "sec" });
  await user.selectOptions(screen.getByLabelText("환경"), "env/sec.json");
  await waitFor(() => expect(screen.getByLabelText("변수 1 이름")).toHaveValue("opaque"));
  const written = () => JSON.parse(backend.write.mock.calls.at(-1)![2] as string).variables;

  // 손대지 않고 저장해도 secret이 남는다.
  await user.click(screen.getByRole("button", { name: "환경 저장" }));
  await waitFor(() => expect(backend.write).toHaveBeenCalledOnce());
  expect(written()).toEqual([{ name: "opaque", value: "", secret: true }]);

  // 평문을 넣고 저장하면 지우고, 알림이 그 이름을 말한다.
  await user.type(screen.getByLabelText("변수 1 값"), "hunter2");
  await user.click(screen.getByRole("button", { name: "환경 저장" }));
  await waitFor(() => expect(backend.write).toHaveBeenCalledTimes(2));
  expect(written()).toEqual([{ name: "opaque", value: "", secret: true }]);
  expect(JSON.stringify(backend.write.mock.calls.at(-1))).not.toContain("hunter2");
  expect(await screen.findByRole("status")).toHaveTextContent("opaque");
});

it("a row the save cleared as a secret stays secret for the rest of the draft", async () => {
  const user = userEvent.setup();
  const token = "ghp_abcdefghijklmnop1234";
  const credential = "Basic dXNlcjpwYXNz";
  expect(looksLikeSecret(token)).toBe(true);
  expect(looksLikeSecret(credential)).toBe(false);
  const { backend } = setup((b) =>
    b.seed(
      personal,
      "env/auth.json",
      JSON.stringify({ version: 1, name: "auth", variables: [{ name: "AUTH", value: token }] }),
    ),
  );
  await screen.findByRole("option", { name: "auth" });
  await user.selectOptions(screen.getByLabelText("환경"), "env/auth.json");
  await waitFor(() => expect(screen.getByLabelText("변수 1 값")).toHaveValue(token));
  const written = () => JSON.parse(backend.write.mock.calls.at(-1)![2] as string).variables;
  await user.click(screen.getByRole("button", { name: "환경 저장" }));
  await waitFor(() => expect(backend.write).toHaveBeenCalledTimes(1));
  expect(written()).toEqual([{ name: "AUTH", value: "", secret: true }]);

  // The same draft: a credential that does not look like a token is still not written.
  await user.clear(screen.getByLabelText("변수 1 값"));
  await user.type(screen.getByLabelText("변수 1 값"), credential);
  await user.click(screen.getByRole("button", { name: "환경 저장" }));
  await waitFor(() => expect(backend.write).toHaveBeenCalledTimes(2));
  expect(written()).toEqual([{ name: "AUTH", value: "", secret: true }]);
  expect(JSON.stringify(backend.write.mock.calls.at(-1))).not.toContain("dXNlcjpwYXNz");
});
