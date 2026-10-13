import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { NativeAutostartSettings, NativeShortcutSettings } from "./NativeSettings";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
beforeEach(() => {
  mocks.invoke.mockReset().mockImplementation(async (name) =>
    name === "app_preferences"
      ? { autostart: false, autostartAsked: true }
      : {
          palette: "Ctrl+Alt+Shift+Space",
          bottomTerminal: null,
          paletteError: "다른 프로그램이 쓰는 중",
          bottomTerminalError: null,
        },
  );
});
afterEach(cleanup);
test("general autostart persists explicit choice and failed native save leaves the prior value", async () => {
  render(<NativeAutostartSettings />);
  const box = await screen.findByRole("checkbox", { name: "Windows 시작 시 devbox 실행" });
  mocks.invoke.mockRejectedValueOnce({ kind: "io" });
  await userEvent.click(box);
  expect(await screen.findByRole("alert")).toHaveTextContent("바꾸지 못했습니다");
  expect(box).not.toBeChecked();
  mocks.invoke.mockResolvedValueOnce({ autostart: true });
  await userEvent.click(box);
  await waitFor(() => expect(box).toBeChecked());
  expect(mocks.invoke).toHaveBeenLastCalledWith("set_autostart", { enabled: true, firstRun: false });
});
test("shortcut errors are shown and failed reconfiguration retains editable drafts for retry", async () => {
  render(<NativeShortcutSettings />);
  const palette = await screen.findByLabelText("명령 팔레트 전역 단축키");
  expect(palette).toHaveValue("Ctrl+Alt+Shift+Space");
  expect(screen.getByRole("alert")).toHaveTextContent("다른 프로그램이 쓰는 중");
  expect(screen.getByLabelText("하단 터미널 전역 단축키")).toHaveValue("");
  await userEvent.clear(palette);
  await userEvent.type(palette, "Ctrl+Alt+P");
  mocks.invoke.mockRejectedValueOnce({ kind: "invalidShortcut" });
  await userEvent.click(screen.getByRole("button", { name: "전역 단축키 저장" }));
  expect(await screen.findByText(/단축키를 저장하지 못했습니다/)).toBeVisible();
  expect(palette).toHaveValue("Ctrl+Alt+P");
  mocks.invoke.mockResolvedValueOnce({
    palette: "Ctrl+Alt+P",
    bottomTerminal: "Ctrl+Alt+T",
    paletteError: null,
    bottomTerminalError: null,
  });
  await userEvent.type(screen.getByLabelText("하단 터미널 전역 단축키"), "Ctrl+Alt+T");
  await userEvent.click(screen.getByRole("button", { name: "전역 단축키 저장" }));
  await waitFor(() =>
    expect(mocks.invoke).toHaveBeenLastCalledWith("configure_shortcuts", {
      palette: "Ctrl+Alt+P",
      bottomTerminal: "Ctrl+Alt+T",
    }),
  );
});
test("pending shortcut saves are disabled and status load failure can be retried", async () => {
  mocks.invoke.mockRejectedValueOnce({ kind: "io" });
  render(<NativeShortcutSettings />);
  await screen.findByRole("alert");
  await userEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  await screen.findByLabelText("명령 팔레트 전역 단축키");
  let finish!: (value: unknown) => void;
  mocks.invoke.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await userEvent.click(screen.getByRole("button", { name: "전역 단축키 저장" }));
  expect(screen.getByRole("button", { name: "전역 단축키 저장" })).toBeDisabled();
  await act(async () =>
    finish({ palette: "Ctrl+Alt+Shift+Space", bottomTerminal: null, paletteError: null, bottomTerminalError: null }),
  );
});
test("native returned registration errors preserve attempted shortcut text instead of implying a saved configuration", async () => {
  render(<NativeShortcutSettings />);
  const palette = await screen.findByLabelText("명령 팔레트 전역 단축키");
  await userEvent.clear(palette);
  await userEvent.type(palette, "Ctrl+Alt+P");
  mocks.invoke.mockResolvedValueOnce({
    palette: "Ctrl+Alt+Shift+Space",
    bottomTerminal: null,
    paletteError: "다른 프로그램이 쓰는 중",
    bottomTerminalError: null,
  });
  await userEvent.click(screen.getByRole("button", { name: "전역 단축키 저장" }));
  await waitFor(() =>
    expect(mocks.invoke).toHaveBeenCalledWith("configure_shortcuts", { palette: "Ctrl+Alt+P", bottomTerminal: null }),
  );
  expect(palette).toHaveValue("Ctrl+Alt+P");
});
