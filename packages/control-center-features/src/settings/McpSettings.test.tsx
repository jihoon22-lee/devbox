import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { findA11yViolations } from "@devbox/a11y/testing";
import { toolsCall } from "../calls";
import McpSettings, { defaultWslPath } from "./McpSettings";
vi.mock("../calls", () => ({ toolsCall: vi.fn() }));
const call = vi.mocked(toolsCall);
const writeText = vi.fn(async (_text: string) => {});
const launcherPath = "C:\\Users\\me\\AppData\\Local\\Devbox\\bin\\devbox-mcp.exe";
let settings = { enabled: false, allowNoteCapture: false, allowTaskRun: false };
beforeEach(() => {
  settings = { enabled: false, allowNoteCapture: false, allowTaskRun: false };
  call.mockReset();
  writeText.mockClear();
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
  call.mockImplementation(async (method, args) => {
    if (method === "set_mcp_settings") settings = (args as { settings: typeof settings }).settings;
    return { settings: { ...settings }, launcherPath };
  });
});
afterEach(cleanup);
it("defaults off, disables writes, saves permissions and copies both registrations", async () => {
  const { container } = render(<McpSettings />);
  const enabled = await screen.findByRole("checkbox", { name: "MCP 서버 사용" });
  expect((enabled as HTMLInputElement).checked).toBe(false);
  expect((screen.getByRole("checkbox", { name: "노트 기록 허용" }) as HTMLInputElement).disabled).toBe(true);
  fireEvent.click(enabled);
  await waitFor(() =>
    expect(call).toHaveBeenCalledWith("set_mcp_settings", {
      settings: { enabled: true, allowNoteCapture: false, allowTaskRun: false },
    }),
  );
  await screen.findByText(launcherPath);
  fireEvent.click(screen.getByRole("button", { name: "Claude Code 등록 명령 복사" }));
  await waitFor(() =>
    expect(writeText).toHaveBeenCalledWith(
      `claude mcp add --scope user devbox -- "$(wslpath -u '${launcherPath}')" --mcp-stdio`,
    ),
  );
  await waitFor(() =>
    expect((screen.getByRole("button", { name: "Codex 설정 복사" }) as HTMLButtonElement).disabled).toBe(false),
  );
  fireEvent.click(screen.getByRole("button", { name: "Codex 설정 복사" }));
  await waitFor(() =>
    expect(writeText).toHaveBeenCalledWith(
      '[mcp_servers.devbox]\ncommand = "/mnt/c/Users/me/AppData/Local/Devbox/bin/devbox-mcp.exe"\nargs = ["--mcp-stdio"]',
    ),
  );
  await waitFor(() =>
    expect((screen.getByRole("checkbox", { name: "노트 기록 허용" }) as HTMLInputElement).disabled).toBe(false),
  );
  fireEvent.click(screen.getByRole("checkbox", { name: "노트 기록 허용" }));
  await waitFor(() => expect(settings.allowNoteCapture).toBe(true));
  await waitFor(() =>
    expect((screen.getByRole("checkbox", { name: "신뢰한 작업 실행 허용" }) as HTMLInputElement).disabled).toBe(false),
  );
  fireEvent.click(screen.getByRole("checkbox", { name: "신뢰한 작업 실행 허용" }));
  await waitFor(() => expect(settings.allowTaskRun).toBe(true));
  expect(await findA11yViolations(container)).toEqual([]);
});
it("shows only installed-suite guidance for portable", async () => {
  call.mockResolvedValue({ settings, launcherPath: null });
  const { container } = render(<McpSettings />);
  await screen.findByText("설치형에서만 사용할 수 있습니다.");
  expect(screen.queryByRole("checkbox")).toBeNull();
  expect(await findA11yViolations(container)).toEqual([]);
});
it("converts drive paths without assuming a custom WSL mount configuration", () => {
  expect(defaultWslPath(launcherPath)).toBe("/mnt/c/Users/me/AppData/Local/Devbox/bin/devbox-mcp.exe");
  expect(defaultWslPath("\\\\?\\D:\\Devbox\\bin\\devbox-mcp.exe")).toBe("/mnt/d/Devbox/bin/devbox-mcp.exe");
  expect(defaultWslPath("\\\\server\\share\\devbox.exe")).toBeNull();
});

it("retries a failed settings read without resetting saved permissions", async () => {
  call.mockRejectedValueOnce(new Error("synthetic failure"));
  render(<McpSettings />);
  await screen.findByRole("alert");
  fireEvent.click(screen.getByRole("button", { name: "다시 확인" }));
  await screen.findByRole("checkbox", { name: "MCP 서버 사용" });
  expect(call.mock.calls.filter(([method]) => method === "mcp_settings")).toHaveLength(2);
});

it("quotes apostrophes in the Windows path before copying the WSL registration", async () => {
  const path = launcherPath.replace("\\me\\", "\\O'Brien\\");
  call.mockResolvedValue({ settings: { ...settings, enabled: true }, launcherPath: path });
  render(<McpSettings />);
  fireEvent.click(await screen.findByRole("button", { name: "Claude Code 등록 명령 복사" }));
  await waitFor(() => expect(writeText).toHaveBeenCalled());
  expect(writeText.mock.calls[0][0]).toContain("O'\\''Brien");
});
