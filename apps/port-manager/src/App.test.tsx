import {
  cleanup,
  createEvent,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App, {
  listenerKillRequest,
  isCurrentRequest,
  localhostUrl,
  matches,
  portRowKey,
  provenanceLabel,
  shouldIgnoreComposingShortcut,
} from "./App";
import {
  getProcessInfo,
  handoffContainerStop,
  loadPortManagerPreferences,
  killListener,
  listPorts,
  openBrowser,
  revealProcess,
  savePortManagerPreferences,
} from "./api";
import type { PortRow } from "./types";
import {
  DEFAULT_PREFERENCES,
  diffPortRows,
  isPinnedRow,
  isPortFavorite,
  isProcessFavorite,
  sameProcessFavorite,
} from "./refresh";

vi.mock("./api", () => ({
  listPorts: vi.fn(),
  loadPortManagerPreferences: vi.fn(),
  savePortManagerPreferences: vi.fn(),
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
const loadPreferencesMock = vi.mocked(loadPortManagerPreferences);
const savePreferencesMock = vi.mocked(savePortManagerPreferences);
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
  loadPreferencesMock.mockReset().mockResolvedValue({
    ...DEFAULT_PREFERENCES,
    favorite_ports: [],
    favorite_processes: [],
  });
  savePreferencesMock.mockReset().mockResolvedValue(undefined);
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

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

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

  it("keeps source-specific provenance visible for container rows", () => {
    expect(
      provenanceLabel(
        row({
          source: "container",
          container_engine: "docker",
          wsl_distro: "docker-desktop",
          container_id: "aabbccdd",
        }),
      ),
    ).toBe("Container · docker · docker-desktop · aabbccdd");
  });

  it("keeps container engine identities distinct in row keys", () => {
    const docker = row({
      source: "container",
      container_engine: "docker",
      wsl_distro: "docker-desktop",
      container_id: "aabbccdd",
      identity: {
        kind: "container",
        engine: "docker",
        container_id: "aabbccdd",
        distro: "docker-desktop",
      },
    });
    const podman = {
      ...docker,
      container_engine: "podman",
      identity: {
        kind: "container" as const,
        engine: "podman",
        container_id: "aabbccdd",
        distro: "docker-desktop",
      },
    };
    expect(portRowKey(docker)).not.toBe(portRowKey(podman));
  });
});

describe("refresh diff and favorite boundaries", () => {
  it("does not manufacture initial changes and reports new, closed, and changed rows by identity", () => {
    const changed = { ...LISTENING_ROW, state: "BOUND", process_name: "node-worker.exe" };
    const added = {
      ...LISTENING_ROW,
      local_addr: "127.0.0.1:4000",
      port: 4000,
      pid: 6789,
      process_name: "python.exe",
      process_start_time: "300",
      identity: { kind: "windows" as const, pid: 6789, start_time: "300" },
    };
    expect(diffPortRows(null, [LISTENING_ROW])).toEqual([]);
    expect(
      diffPortRows([LISTENING_ROW, ESTABLISHED_ROW], [changed, added]).map((item) => item.kind),
    ).toEqual(["new", "closed", "changed"]);
    const changedDiff = diffPortRows([LISTENING_ROW], [changed]);
    expect(changedDiff[0]?.before?.identity).toEqual(LISTENING_ROW.identity);
    expect(changedDiff[0]?.after?.state).toBe("BOUND");
  });

  it("uses endpoint fallback for identity-less rows without borrowing another process", () => {
    const first = row({ identity: null, pid: null, local_addr: "127.0.0.1:3000" });
    const moved = { ...first, local_addr: "127.0.0.1:4000", port: 4000 };
    expect(diffPortRows([first], [moved]).map((item) => item.kind)).toEqual(["new", "closed"]);
  });

  it("reports an endpoint move and display metadata update for the same identity as changed", () => {
    const moved = {
      ...LISTENING_ROW,
      local_addr: "127.0.0.1:4000",
      port: 4000,
      command_line: "node worker.js --port 4000",
    };
    const [change] = diffPortRows([LISTENING_ROW], [moved]);
    expect(change?.kind).toBe("changed");
    expect(change?.before?.local_addr).toBe(LISTENING_ROW.local_addr);
    expect(change?.after?.local_addr).toBe(moved.local_addr);
  });

  it("reserves exact endpoints before matching a same-process endpoint move", () => {
    const identity = LISTENING_ROW.identity;
    const previousExact = { ...LISTENING_ROW, local_addr: "127.0.0.1:3000", port: 3000 };
    const previousMoved = { ...LISTENING_ROW, local_addr: "127.0.0.1:4000", port: 4000 };
    const nextMoved = { ...LISTENING_ROW, local_addr: "127.0.0.1:2000", port: 2000, identity };
    const nextExact = { ...previousExact, identity };

    const changes = diffPortRows(
      [previousExact, previousMoved],
      [nextMoved, nextExact],
    );

    expect(changes).toHaveLength(1);
    expect(changes[0]?.kind).toBe("changed");
    expect(changes[0]?.before?.port).toBe(4000);
    expect(changes[0]?.after?.port).toBe(2000);
  });

  it("matches process favorites by identity rather than object property order", () => {
    const left = {
      source: "windows" as const,
      identity: { kind: "windows" as const, pid: 42, start_time: "100" },
    };
    const right = {
      source: "windows" as const,
      identity: { start_time: "100", pid: 42, kind: "windows" as const },
    };
    expect(sameProcessFavorite(left, right)).toBe(true);
    expect(
      isProcessFavorite(LISTENING_ROW, [
        {
          source: "windows",
          identity: { start_time: "100", pid: 1234, kind: "windows" },
        },
      ]),
    ).toBe(true);
  });

  it("keeps identity-less rows distinct when only source or port differs", () => {
    const windows = row({ identity: null, pid: null, port: 3000 });
    const wsl = row({ identity: null, pid: null, source: "wsl", port: 3000 });
    const otherPort = row({ identity: null, pid: null, port: 4000 });
    expect(portRowKey(windows)).not.toBe(portRowKey(wsl));
    expect(portRowKey(windows)).not.toBe(portRowKey(otherPort));
  });

  it("keeps port and process favorites independent and combines them for pinned filtering", () => {
    const preferences = {
      ...DEFAULT_PREFERENCES,
      favorite_ports: [
        { source: "windows" as const, proto: "TCP", local_addr: "127.0.0.1:3000", port: 3000 },
      ],
      favorite_processes: [],
    };
    expect(isPortFavorite(LISTENING_ROW, preferences.favorite_ports)).toBe(true);
    expect(isProcessFavorite(LISTENING_ROW, preferences.favorite_processes)).toBe(false);
    expect(isPinnedRow(LISTENING_ROW, preferences)).toBe(true);
    expect(isPinnedRow(ESTABLISHED_ROW, preferences)).toBe(false);
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

  it("does not prevent keyboard activation of action buttons nested in a row", async () => {
    render(<App />);
    await screen.findByText("node.exe");
    const target = renderedRow("node.exe");
    const favorite = within(target).getByRole("button", { name: "Favorite port" });
    const event = createEvent.keyDown(favorite, { key: "Enter", code: "Enter" });
    fireEvent(favorite, event);
    expect(event.defaultPrevented).toBe(false);
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

  it("queues a fresh snapshot after Kill when a pre-kill timer poll is already in flight", async () => {
    let resolvePoll: ((rows: PortRow[]) => void) | undefined;
    listPortsMock
      .mockReset()
      .mockResolvedValueOnce([LISTENING_ROW])
      .mockImplementationOnce(
        () =>
          new Promise<PortRow[]>((resolve) => {
            resolvePoll = resolve;
          }),
      )
      .mockResolvedValueOnce([]);
    confirmMock.mockReturnValue(true);
    render(<App />);
    await screen.findByText("node.exe");

    // Recreate the interval after fake timers are active; an interval created
    // by the real clock before `useFakeTimers` cannot be advanced by Vitest.
    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    vi.useFakeTimers();
    fireEvent.click(screen.getByRole("button", { name: "Resume" }));
    await vi.advanceTimersByTimeAsync(DEFAULT_PREFERENCES.refresh_interval_ms);
    expect(listPortsMock).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByRole("button", { name: "Kill listener" }));
    await Promise.resolve();
    expect(killListenerMock).toHaveBeenCalledTimes(1);
    expect(listPortsMock).toHaveBeenCalledTimes(2);

    resolvePoll?.([LISTENING_ROW]);
    await vi.waitFor(() => expect(listPortsMock).toHaveBeenCalledTimes(3));
  });

  it("pauses timer polls and keeps a slow native refresh single-flight", async () => {
    let resolvePoll: ((rows: PortRow[]) => void) | undefined;
    listPortsMock
      .mockReset()
      .mockResolvedValueOnce([LISTENING_ROW])
      .mockImplementation(
        () =>
          new Promise<PortRow[]>((resolve) => {
            resolvePoll = resolve;
          }),
    );
    render(<App />);
    await screen.findByText("node.exe");
    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    vi.useFakeTimers();
    vi.advanceTimersByTime(60_000);
    await Promise.resolve();
    expect(listPortsMock).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "Resume" }));
    vi.advanceTimersByTime(5_000);
    await Promise.resolve();
    expect(listPortsMock).toHaveBeenCalledTimes(2);
    vi.advanceTimersByTime(20_000);
    await Promise.resolve();
    expect(listPortsMock).toHaveBeenCalledTimes(2);
    resolvePoll?.([LISTENING_ROW]);
    await Promise.resolve();
  });

  it("preserves the last stable rows but locks listener actions after a poll failure", async () => {
    listPortsMock
      .mockReset()
      .mockResolvedValueOnce([LISTENING_ROW])
      .mockRejectedValueOnce(new Error("source unavailable"));
    render(<App />);
    await screen.findByText("node.exe");
    const refreshButton = screen.getByRole("button", { name: "Refresh" });
    fireEvent.click(refreshButton);
    await screen.findByText("Action failed. Refresh the list and try again.");
    expect(screen.getByText("node.exe")).toBeTruthy();
    const kill = screen.getByRole("button", { name: "Kill listener" });
    expect((kill as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(kill);
    expect(killListenerMock).not.toHaveBeenCalled();
  });

  it("persists bounded interval, pinned filter, and independent port/process favorites", async () => {
    render(<App />);
    await screen.findByText("node.exe");

    fireEvent.change(screen.getByRole("combobox", { name: "Auto-refresh interval" }), {
      target: { value: "10000" },
    });
    await waitFor(() =>
      expect(savePreferencesMock).toHaveBeenCalledWith(
        expect.objectContaining({ refresh_interval_ms: 10_000 }),
      ),
    );

    const listeningRow = renderedRow("node.exe");
    fireEvent.click(within(listeningRow).getByRole("button", { name: "Favorite port" }));
    await waitFor(() =>
      expect(savePreferencesMock).toHaveBeenCalledWith(
        expect.objectContaining({
          favorite_ports: [
            expect.objectContaining({ local_addr: LISTENING_ROW.local_addr, port: 3000 }),
          ],
        }),
      ),
    );
    fireEvent.click(within(listeningRow).getByRole("button", { name: "Favorite process" }));
    await waitFor(() =>
      expect(savePreferencesMock).toHaveBeenCalledWith(
        expect.objectContaining({
          favorite_processes: [
            expect.objectContaining({
              identity: LISTENING_ROW.identity,
            }),
          ],
        }),
      ),
    );

    fireEvent.click(screen.getByRole("button", { name: "Pinned" }));
    await waitFor(() =>
      expect(savePreferencesMock).toHaveBeenCalledWith(expect.objectContaining({ pinned_only: true })),
    );
    expect(screen.getAllByText("node.exe").length).toBeGreaterThanOrEqual(1);
    expect(screen.queryByText("browser.exe")).toBeNull();
  });

  it("shows WSL provenance and clears selection when a refreshed endpoint disappears", async () => {
    const wslRow: PortRow = {
      ...LISTENING_ROW,
      source: "wsl",
      wsl_distro: "Ubuntu",
      wsl_start_tick: 77,
      identity: { kind: "wsl", distro: "Ubuntu", pid: 1234, start_tick: 77 },
    };
    listPortsMock
      .mockReset()
      .mockResolvedValueOnce([wslRow])
      .mockResolvedValueOnce([]);
    render(<App />);
    const process = await screen.findByText("node.exe");
    fireEvent.click(process.closest("tr") as HTMLTableRowElement);
    expect(screen.getAllByText("WSL · Ubuntu").length).toBeGreaterThanOrEqual(1);
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => expect(screen.queryByRole("complementary", { name: "Listener details" })).toBeNull());
    expect(screen.getByRole("region", { name: "Refresh changes" })).toBeTruthy();
  });

  it("does not change the visible favorite when atomic preference persistence fails", async () => {
    savePreferencesMock.mockRejectedValueOnce(new Error("disk full"));
    render(<App />);
    await screen.findByText("node.exe");
    const listeningRow = renderedRow("node.exe");
    fireEvent.click(within(listeningRow).getByRole("button", { name: "Favorite port" }));
    await screen.findByText("Action failed. Refresh the list and try again.");
    expect(within(listeningRow).getByRole("button", { name: "Favorite port" })).toBeTruthy();
  });
});
