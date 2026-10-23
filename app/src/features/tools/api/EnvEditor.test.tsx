// 검토 3라운드 차단: 저장하지 않은 환경 편집이 확인 없이 사라지지 않는다.
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it } from "vitest";
import { ApiBackendProvider } from "./backend";
import { EnvEditor, resetEnvDrafts } from "./EnvEditor";
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
function setup() {
  const backend = new FakeBackend();
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
