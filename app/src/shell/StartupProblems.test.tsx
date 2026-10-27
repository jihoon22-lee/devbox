import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ProjectWire } from "../features/projects/test-wire";
import { RpcProvider } from "../rpc/RpcProvider";
import { StartupProblems } from "./StartupProblems";

const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));

// `startupProblems` is omitted from the wire when there are none.
const info = {
  version: "1.0.0-dev",
  instance: "test",
  pid: 1,
  startedMs: 1,
  dataDir: "/fixture/data",
  configPath: "/fixture/devbox/config.toml",
};
const configFile = {
  kind: "configFile",
  file: "/fixture/devbox/config.toml",
  message: "line 3, column 7: expected `=`",
};
const configSection = {
  kind: "configSection",
  file: "/fixture/devbox/config.local.toml",
  section: "notes",
  message: 'invalid type: string "yes", expected a boolean',
};
const agentsReconcile = { kind: "agentsReconcile", message: "tmux list-sessions: no server running" };
/** Matches the one element whose whole text is `text`, even when `<code>` splits it. */
const whole = (text: string) => (_: string, element: Element | null) => element?.textContent === text;

beforeEach(() => {
  native.invoke.mockReset().mockResolvedValue(undefined);
  Object.assign(window, { __TAURI_INTERNALS__: {} });
});
afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.useRealTimers();
});
async function show(problems?: unknown[]) {
  const wire = new ProjectWire();
  const view = render(
    <RpcProvider client={wire.client}>
      <StartupProblems />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("system.info")).toHaveLength(1));
  await act(async () => wire.reply("system.info", problems ? { ...info, startupProblems: problems } : info));
  // Query results reach React on a later task; wait until the banner lists them.
  if (problems) await screen.findAllByRole("listitem");
  return { wire, view };
}

test("each startup problem shows its fixed title and guidance with the raw message as detail", async () => {
  await show([configFile, configSection, agentsReconcile]);
  const [file, section, agents] = screen.getAllByRole("listitem");
  expect(within(file!).getByText("설정 파일을 읽지 못했습니다")).toBeInTheDocument();
  expect(
    within(file!).getByText(
      whole(
        "/fixture/devbox/config.toml에 문법 오류가 있어 기본값으로 실행 중입니다. 노트·검색·Windows 사본과 사용자 정의 도구 프로필이 꺼져 있습니다. 파일을 고친 뒤 [다시 설정]을 누르세요.",
      ),
    ),
  ).toBeInTheDocument();
  expect(within(file!).getByText(configFile.message)).toBeInTheDocument();
  expect(within(section!).getByText("설정 일부를 읽지 못했습니다")).toBeInTheDocument();
  expect(
    within(section!).getByText(
      whole(
        "/fixture/devbox/config.local.toml의 [notes] 항목에 오류가 있어 이 기능을 껐습니다. 고친 뒤 [다시 설정]을 누르세요.",
      ),
    ),
  ).toBeInTheDocument();
  expect(within(section!).getByText(configSection.message)).toBeInTheDocument();
  expect(within(agents!).getByText("에이전트 상태를 확인하지 못했습니다")).toBeInTheDocument();
  expect(
    within(agents!).getByText(
      "터미널 서버(tmux)에 연결하지 못했습니다. 자동으로 다시 시도합니다. 계속되면 설정 → 앱 진단을 확인하세요.",
    ),
  ).toBeInTheDocument();
  expect(within(agents!).getByText(agentsReconcile.message)).toBeInTheDocument();
  // Only settings files can be fixed and copied; tmux recovery points at 앱 진단 instead.
  for (const item of [file!, section!]) {
    expect(within(item).getByRole("button", { name: "다시 설정" })).toBeInTheDocument();
    expect(within(item).getByRole("button", { name: "경로 복사" })).toBeInTheDocument();
  }
  expect(within(agents!).queryByRole("button")).toBeNull();
  expect(document.body.textContent).not.toMatch(/데몬/);
});

test("nothing is drawn and nothing is polled without startup problems", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const { wire, view } = await show();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(30_000);
  });
  expect(view.container).toBeEmptyDOMElement();
  expect(wire.requests("system.info")).toHaveLength(1);
});

test("problems are read again every ten seconds until they clear", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const { wire, view } = await show([agentsReconcile]);
  expect(screen.getByText("에이전트 상태를 확인하지 못했습니다")).toBeInTheDocument();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(9_000);
  });
  expect(wire.requests("system.info")).toHaveLength(1);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1_500);
  });
  expect(wire.requests("system.info")).toHaveLength(2);
  await act(async () => wire.reply("system.info", info));
  await waitFor(() => expect(view.container).toBeEmptyDOMElement());
  await act(async () => {
    await vi.advanceTimersByTimeAsync(30_000);
  });
  expect(wire.requests("system.info")).toHaveLength(2);
});

test("다시 설정 confirms like 앱 진단 and calls the redeploy command once", async () => {
  await show([configFile]);
  await userEvent.click(screen.getByRole("button", { name: "다시 설정" }));
  const dialog = screen.getByRole("dialog");
  expect(dialog).toHaveTextContent("내장 WSL 실행 파일을 다시 배치하고");
  expect(dialog.textContent).not.toMatch(/데몬/);
  expect(native.invoke).not.toHaveBeenCalled();
  const run = within(dialog).getByRole("button", { name: "실행" });
  fireEvent.click(run);
  fireEvent.click(run);
  await waitFor(() => expect(native.invoke).toHaveBeenCalledOnce());
  expect(native.invoke).toHaveBeenCalledWith("redeploy_wsl");
  expect(await screen.findByText(/WSL 다시 설정을 완료했습니다/)).toBeInTheDocument();
});

test("경로 복사 copies the file of that problem", async () => {
  const writeText = vi.fn(async () => {});
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  await show([configSection]);
  await userEvent.click(screen.getByRole("button", { name: "경로 복사" }));
  expect(writeText).toHaveBeenCalledWith("/fixture/devbox/config.local.toml");
  expect(await screen.findByText("설정 파일 경로를 복사했습니다")).toBeInTheDocument();
});
