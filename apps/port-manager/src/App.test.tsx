import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App, {
  listenerKillRequest,
  isCurrentRequest,
  localhostUrl,
  matches,
  portRowKey,
  shouldIgnoreComposingShortcut,
} from "./App";
import {
  getProcessInfo,
  handoffContainerStop,
  killListener,
  listPorts,
  openBrowser,
  revealProcess,
} from "./api";
import type { PortRow } from "./types";

vi.mock("./api", () => ({
  listPorts: vi.fn(),
  killListener: vi.fn(),
  handoffContainerStop: vi.fn(),
  openBrowser: vi.fn(),
  getProcessInfo: vi.fn(),
  revealProcess: vi.fn(),
}));

const LISTENING_ROW: PortRow = {
  proto: "TCP",
  local_addr: "127.0.0.1:3000",
  port: 3000,
  state: "LISTENING",
  pid: 1234,
  process_name: "node.exe",
  source: "windows",
  process_start_time: "100",
  command_line: "node server.js --port 3000",
  executable_path: "C:\\Program Files\\nodejs\\node.exe",
  identity: { kind: "windows", pid: 1234, start_time: "100" },
};

const ESTABLISHED_ROW: PortRow = {
  proto: "TCP",
  local_addr: "10.0.0.5:50261",
  port: 50261,
  state: "ESTABLISHED",
  pid: 4321,
  process_name: "browser.exe",
  source: "windows",
  process_start_time: "200",
  identity: { kind: "windows", pid: 4321, start_time: "200" },
};

const listPortsMock = vi.mocked(listPorts);
const killListenerMock = vi.mocked(killListener);
const handoffContainerStopMock = vi.mocked(handoffContainerStop);
const openBrowserMock = vi.mocked(openBrowser);
const getProcessInfoMock = vi.mocked(getProcessInfo);
const revealProcessMock = vi.mocked(revealProcess);
const writeTextMock = vi.fn<(text: string) => Promise<void>>();
const confirmMock = vi.fn<(message?: string) => boolean>();

function row(overrides: Partial<PortRow> = {}): PortRow {
  return {
    proto: "tcp",
    local_addr: "127.0.0.1",
    port: 3000,
    state: "LISTENING",
    pid: 1234,
    process_name: "node.exe",
    source: "windows",
    process_start_time: "100",
    identity: { kind: "windows", pid: 1234, start_time: "100" },
    ...overrides,
  };
}

function renderedRow(processName: string): HTMLTableRowElement {
  const element = screen
    .getAllByText(processName)
    .map((candidate) => candidate.closest("tr"))
    .find((candidate): candidate is HTMLTableRowElement => candidate instanceof HTMLTableRowElement);
  if (!(element instanceof HTMLTableRowElement)) throw new Error("port row was not rendered");
  return element;
}

function openRowMenu(processName: string): HTMLTableRowElement {
  const target = renderedRow(processName);
  fireEvent.contextMenu(target, { clientX: 12, clientY: 18 });
  return target;
}

beforeEach(() => {
  listPortsMock.mockReset().mockResolvedValue([LISTENING_ROW, ESTABLISHED_ROW]);
  killListenerMock.mockReset().mockResolvedValue({ kind: "terminated" });
  handoffContainerStopMock.mockReset().mockResolvedValue({
    target_app: "wsl-desktop",
    action: "stop-container",
    engine: "docker",
    container_id: "aabbccdd",
    distro: "docker-desktop",
  });
  openBrowserMock.mockReset().mockResolvedValue(undefined);
  getProcessInfoMock.mockReset().mockImplementation(async (pid) => ({
    pid,
    name: pid === 1234 ? "node.exe" : "browser.exe",
    exe: pid === 1234 ? "C:\\Program Files\\nodejs\\node.exe" : "C:\\Browser\\browser.exe",
    start_time: 1,
    memory_bytes: 2,
  }));
  revealProcessMock.mockReset().mockResolvedValue(undefined);
  writeTextMock.mockReset().mockResolvedValue(undefined);
  confirmMock.mockReset().mockReturnValue(false);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: writeTextMock },
  });
  Object.defineProperty(window, "confirm", {
    configurable: true,
    value: confirmMock,
  });
});

afterEach(() => cleanup());

describe("matches", () => {
  it("빈 쿼리(또는 공백만)는 항상 true", () => {
    expect(matches(row(), "")).toBe(true);
    expect(matches(row(), "   ")).toBe(true);
  });

  it("proto/state/port/local_addr/process_name 각 필드로 대소문자 구분 없이 매치한다", () => {
    expect(matches(row(), "TCP")).toBe(true);
    expect(matches(row(), "listening")).toBe(true);
    expect(matches(row(), "3000")).toBe(true);
    expect(matches(row(), "127.0.0")).toBe(true);
    expect(matches(row(), "NODE")).toBe(true);
  });

  it("pid로도 매치한다", () => {
    expect(matches(row({ pid: 4321 }), "4321")).toBe(true);
  });

  it("pid가 null이면 pid 매치 조건에서 예외 없이 건너뛴다", () => {
    expect(matches(row({ pid: null }), "1234")).toBe(false);
    expect(matches(row({ pid: null }), "tcp")).toBe(true);
  });

  it("process_name이 null이어도 예외 없이 처리한다", () => {
    expect(matches(row({ process_name: null }), "node")).toBe(false);
    expect(matches(row({ process_name: null }), "tcp")).toBe(true);
  });

  it("searches bounded command, executable, distro, and container metadata", () => {
    expect(matches(row({ command_line: "node server.js" }), "server.js")).toBe(true);
    expect(matches(row({ executable_path: "C:\\Tools\\node.exe" }), "tools")).toBe(true);
    expect(matches(row({ wsl_distro: "Ubuntu" }), "ubuntu")).toBe(true);
    expect(matches(row({ container_name: "api" }), "api")).toBe(true);
  });

  it("어느 필드에도 없는 문자열은 false", () => {
    expect(matches(row(), "no-such-thing")).toBe(false);
  });
});

describe("port row helpers", () => {
  it("creates a stable row key and localhost URL without exposing another address", () => {
    expect(portRowKey(LISTENING_ROW)).toBe("TCP:127.0.0.1:3000:1234:100");
    expect(localhostUrl(LISTENING_ROW)).toBe("http://localhost:3000");
    expect(localhostUrl(row({ port: 0 }))).toBeNull();
  });
});

describe("port context menu", () => {
  it("selects the right-clicked row first and exposes every app-owned action", async () => {
    render(<App />);
    await screen.findByText("node.exe");

    const target = openRowMenu("browser.exe");

    expect(target.getAttribute("aria-selected")).toBe("true");
    expect(screen.getByRole("menu", { name: "Port actions" })).toBeTruthy();
    for (const label of [
      "Copy port",
      "Copy PID",
      "Copy localhost URL",
      "Open localhost",
      "Copy process path",
      "Show in Explorer",
      "Kill listener",
    ]) {
      expect(screen.getByRole("menuitem", { name: label })).toBeTruthy();
    }
    expect(
      screen.getByRole("menuitem", { name: "Open localhost" }).getAttribute("aria-disabled"),
    ).toBe("true");
    await waitFor(() => expect(getProcessInfoMock).toHaveBeenCalledWith(4321));
  });

  it("opens from Shift+F10, copies exact row values, and restores focus", async () => {
    render(<App />);
    await screen.findByText("node.exe");
    const target = renderedRow("node.exe");
    target.focus();

    fireEvent.keyDown(target, { key: "F10", code: "F10", shiftKey: true });
    const copyPort = screen.getByRole("menuitem", { name: "Copy port" });
    await waitFor(() => expect(document.activeElement).toBe(copyPort));
    fireEvent.click(copyPort);

    await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith("3000"));
    await waitFor(() => expect(document.activeElement).toBe(target));
  });

  it("copies and reveals only the lazily resolved executable for the selected PID", async () => {
    render(<App />);
    await screen.findByText("node.exe");
    openRowMenu("node.exe");

    const copyPath = screen.getByRole("menuitem", { name: "Copy process path" });
    await waitFor(() => expect(copyPath.getAttribute("aria-disabled")).toBeNull());
    fireEvent.click(copyPath);
    await waitFor(() =>
      expect(writeTextMock).toHaveBeenCalledWith("C:\\Program Files\\nodejs\\node.exe"),
    );

    openRowMenu("node.exe");
    const reveal = screen.getByRole("menuitem", { name: "Show in Explorer" });
    await waitFor(() => expect(reveal.getAttribute("aria-disabled")).toBeNull());
    fireEvent.click(reveal);
    await waitFor(() => expect(revealProcessMock).toHaveBeenCalledWith(1234));
  });

  it("keeps path actions disabled when process lookup fails", async () => {
    getProcessInfoMock.mockRejectedValueOnce(new Error("process exited"));
    render(<App />);
    await screen.findByText("node.exe");
    openRowMenu("node.exe");

    await waitFor(() => expect(getProcessInfoMock).toHaveBeenCalledWith(1234));
    expect(
      screen.getByRole("menuitem", { name: "Copy process path" }).getAttribute("aria-disabled"),
    ).toBe("true");
    expect(
      screen.getByRole("menuitem", { name: "Show in Explorer" }).getAttribute("aria-disabled"),
    ).toBe("true");
  });

  it("requires confirmation before Kill and refreshes only after an accepted kill", async () => {
    render(<App />);
    await screen.findByText("node.exe");
    openRowMenu("node.exe");
    fireEvent.click(screen.getByRole("menuitem", { name: "Kill listener" }));

    expect(confirmMock).toHaveBeenCalledWith("127.0.0.1:3000 (node.exe) listener 종료할까요?");
    expect(killListenerMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    openRowMenu("node.exe");
    fireEvent.click(screen.getByRole("menuitem", { name: "Kill listener" }));

    await waitFor(() =>
      expect(killListenerMock).toHaveBeenCalledWith(listenerKillRequest(LISTENING_ROW)),
    );
    await waitFor(() => expect(listPortsMock).toHaveBeenCalledTimes(2));
  });

  it("does not open a non-listening endpoint and reports action failures", async () => {
    openBrowserMock.mockRejectedValueOnce(new Error("browser unavailable"));
    render(<App />);
    await screen.findByText("node.exe");

    openRowMenu("browser.exe");
    fireEvent.click(screen.getByRole("menuitem", { name: "Open localhost" }));
    expect(openBrowserMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    expect(await screen.findByText("Action failed. Refresh the list and try again.")).toBeTruthy();
  });
});

describe("identity-safe listener UI boundaries", () => {
  it("does not create a kill request for a row without an identity", () => {
    expect(listenerKillRequest(row({ identity: null }))).toBeNull();
  });

  it("ignores shortcut handling while an IME composition is active", () => {
    expect(shouldIgnoreComposingShortcut(true, "Enter")).toBe(true);
    expect(shouldIgnoreComposingShortcut(true, "F10")).toBe(true);
    expect(shouldIgnoreComposingShortcut(false, "Enter")).toBe(false);
  });

  it("ignores an older refresh result after a newer request wins", () => {
    expect(isCurrentRequest(1, 2)).toBe(false);
    expect(isCurrentRequest(2, 2)).toBe(true);
  });

  it("does not update state when an in-flight refresh resolves after unmount", async () => {
    let resolveRefresh: ((rows: PortRow[]) => void) | undefined;
    listPortsMock.mockReset().mockImplementation(
      () =>
        new Promise<PortRow[]>((resolve) => {
          resolveRefresh = resolve;
        }),
    );
    const { unmount } = render(<App />);
    unmount();
    resolveRefresh?.([LISTENING_ROW]);
    await Promise.resolve();
  });

  it("exposes details and alert semantics for keyboard users", async () => {
    listPortsMock.mockResolvedValueOnce([LISTENING_ROW]);
    render(<App />);
    const target = await screen.findByText("node.exe");
    fireEvent.click(target.closest("tr") as HTMLTableRowElement);
    expect(screen.getByRole("complementary", { name: "Listener details" })).toBeTruthy();
    expect(screen.getByText("node server.js --port 3000")).toBeTruthy();
    expect(screen.getByRole("table", { name: "Listener list" })).toBeTruthy();
  });

  it("uses the WSL Desktop handoff for container rows and never calls listener kill", async () => {
    const container: PortRow = {
      ...row(),
      local_addr: "127.0.0.1:8080",
      port: 8080,
      pid: null,
      process_name: "api",
      source: "container",
      container_engine: "docker",
      container_id: "aabbccdd",
      container_name: "api",
      wsl_distro: "docker-desktop",
      identity: {
        kind: "container",
        engine: "docker",
        container_id: "aabbccdd",
        distro: "docker-desktop",
      },
    };
    listPortsMock.mockReset().mockResolvedValue([container]);
    confirmMock.mockReturnValue(true);
    render(<App />);
    await screen.findByText("api");
    fireEvent.click(screen.getByRole("button", { name: "Stop in WSL Desktop" }));
    await waitFor(() => expect(handoffContainerStopMock).toHaveBeenCalled());
    expect(killListenerMock).not.toHaveBeenCalled();
    expect((await screen.findByRole("status")).textContent).toContain("aabbccdd");
  });

  it("suppresses duplicate listener actions before React commits busy state", async () => {
    let resolveKill: (() => void) | undefined;
    killListenerMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveKill = () => resolve({ kind: "terminated" });
        }),
    );
    confirmMock.mockReturnValue(true);
    render(<App />);
    await screen.findByText("node.exe");

    const button = screen.getByRole("button", { name: "Kill listener" });
    fireEvent.click(button);
    fireEvent.click(button);

    expect(confirmMock).toHaveBeenCalledTimes(1);
    expect(killListenerMock).toHaveBeenCalledTimes(1);
    resolveKill?.();
    await waitFor(() => expect(listPortsMock).toHaveBeenCalledTimes(2));
  });
});
