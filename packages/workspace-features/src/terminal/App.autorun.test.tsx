import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";
import App from "./App";
import { startSession, writeInitialCommand } from "./api";
import { configureProductTransport } from "../transport";
import { configureTerminalStorage, initializeTerminalPreferences } from "./lib/storageNamespace";
import { initializeProductLayout } from "./lib/workspace";

// Keep the real App → PaneCanvas → TermPane command flow; replace only canvas
// rendering and native IO unavailable in jsdom.
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    rows = 24;
    cols = 80;
    unicode = { activeVersion: "" };
    parser = { registerOscHandler: () => ({ dispose() {} }) };
    constructor(public options: Record<string, unknown>) {}
    loadAddon() {}
    open() {}
    attachCustomKeyEventHandler() {}
    clear() {}
    reset() {}
    focus() {}
    dispose() {}
    write(_data: string, done?: () => void) {
      done?.();
    }
    getSelection() {
      return "";
    }
    hasSelection() {
      return false;
    }
    onData() {
      return { dispose() {} };
    }
    onSelectionChange() {
      return { dispose() {} };
    }
    onTitleChange() {
      return { dispose() {} };
    }
    onBell() {
      return { dispose() {} };
    }
  },
}));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit() {}
  },
}));
vi.mock("@xterm/addon-search", () => ({
  SearchAddon: class {
    onDidChangeResults() {
      return { dispose() {} };
    }
    clearDecorations() {}
    findNext() {}
    findPrevious() {}
  },
}));
vi.mock("@xterm/addon-unicode11", () => ({ Unicode11Addon: class {} }));
vi.mock("@xterm/addon-web-links", () => ({ WebLinksAddon: class {} }));
vi.mock("@xterm/addon-webgl", () => ({
  WebglAddon: class {
    onContextLoss() {
      return { dispose() {} };
    }
    dispose() {}
  },
}));
vi.mock("./api", () => ({
  configureQuickSummon: vi.fn().mockResolvedValue({
    shortcutRegistered: false,
    activeShortcut: null,
    trayEnabled: false,
    closeBehavior: "exit",
    issues: [],
  }),
  getDashboardSnapshot: vi.fn().mockImplementation(async () => ({
    revision: 1,
    capturedAtMs: Date.now(),
    staleAfterMs: 30000,
    distros: [
      {
        name: "Ubuntu",
        version: 2,
        default: true,
        state: "Running",
        terminalCount: 0,
        dockerAvailability: "unavailable",
        containers: [],
        resource: null,
      },
    ],
  })),
  listDistros: vi.fn().mockResolvedValue([{ name: "Ubuntu", version: 2, default: true, state: "Running" }]),
  startSession: vi.fn().mockResolvedValue({ sessionId: "fixture-session", resumed: false, multiplexer: "native" }),
  detectMultiplexers: vi.fn().mockResolvedValue([{ kind: "native", status: "available", version: null, source: null }]),
  listWorkspaceProfiles: vi.fn().mockResolvedValue([]),
  saveWorkspaceProfile: vi.fn(),
  deleteWorkspaceProfile: vi.fn(),
  closeSession: vi.fn().mockResolvedValue(undefined),
  onTerminalClosed: vi.fn().mockResolvedValue(() => undefined),
  onTerminalOutput: vi.fn().mockResolvedValue(() => undefined),
  getWindowsBuildNumber: vi.fn().mockResolvedValue(null),
  onOpenRequest: vi.fn().mockResolvedValue(() => undefined),
  takePendingOpen: vi.fn().mockResolvedValue(null),
  attachSession: vi.fn().mockResolvedValue(undefined),
  resizeSession: vi.fn().mockResolvedValue(undefined),
  connectProductTerminalOutput: vi.fn(() => () => undefined),
  writeInitialCommand: vi.fn().mockResolvedValue(undefined),
  writeSession: vi.fn().mockResolvedValue(undefined),
  broadcast: vi.fn().mockResolvedValue(undefined),
  readClipboardText: vi.fn().mockResolvedValue(""),
  openTerminalLink: vi.fn().mockResolvedValue(undefined),
}));
const owner = "12345678-1234-1234-1234-123456789abc";
beforeAll(() => {
  configureProductTransport(
    async <T,>(_component: unknown, method: string) =>
      (method === "terminal_preferences"
        ? { "wsl-desktop:settings": JSON.stringify({ version: 1, openTerminalOnStart: false }) }
        : { revision: "a".repeat(64) }) as T,
    "fixture-installation",
  );
  configureTerminalStorage("fixture-installation", owner);
});
beforeEach(async () => {
  localStorage.clear();
  vi.clearAllMocks();
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  await initializeTerminalPreferences();
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
function layout(autoRun?: boolean, restoreOnly = false) {
  initializeProductLayout(owner, {
    revision: "a".repeat(64),
    restoreOnly,
    layout: {
      autoRun,
      tabs: [
        {
          id: "agent",
          title: "Agent",
          customTitle: true,
          layout: "grid",
          paneKeys: ["agent"],
          sizing: { columns: [1], rows: [1] },
        },
      ],
      panes: [{ key: "agent", distro: "Ubuntu", cwd: "/owned/task", startCommand: "claude", multiplexer: "native" }],
      activeTabId: "agent",
      activePaneKey: "agent",
    },
  });
}
it("starts an agent command once without another confirmation", async () => {
  layout(true);
  const view = render(<App />);
  await waitFor(() => expect(writeInitialCommand).toHaveBeenCalledWith("fixture-session", "claude\r"));
  expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  view.rerender(<App />);
  await act(async () => undefined);
  expect(writeInitialCommand).toHaveBeenCalledTimes(1);
  expect(startSession).toHaveBeenCalledTimes(1);
});
it("keeps the start-command confirmation for ordinary profiles", async () => {
  layout();
  render(<App />);
  const dialog = await screen.findByRole("alertdialog");
  expect(within(dialog).getByText("시작 명령 1개를 실행할까요?")).toBeInTheDocument();
  expect(writeInitialCommand).not.toHaveBeenCalled();
  fireEvent.click(within(dialog).getByRole("button", { name: "실행" }));
  await waitFor(() => expect(writeInitialCommand).toHaveBeenCalledTimes(1));
});
it("never sends an agent command from a restoration-only window", async () => {
  layout(true, true);
  render(<App />);
  await waitFor(() => expect(startSession).toHaveBeenCalledTimes(1));
  await act(async () => undefined);
  expect(writeInitialCommand).not.toHaveBeenCalled();
  expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
});
