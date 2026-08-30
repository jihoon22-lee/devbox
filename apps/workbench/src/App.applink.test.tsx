import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { onOpenRequest, takePendingOpen, type OpenRequest } from "./api";

const mocks = vi.hoisted(() => ({
  openHandler: null as ((request: OpenRequest) => void) | null,
  order: [] as string[],
}));

vi.mock("./api", () => ({
  cancelDependencyHealth: vi.fn().mockResolvedValue(false),
  cancelProjectEnvironment: vi.fn().mockResolvedValue(false),
  cancelProjectHealth: vi.fn().mockResolvedValue(false),
  cancelStartWorkspace: vi.fn().mockResolvedValue(false),
  cancelWorkspacePreflight: vi.fn().mockResolvedValue(false),
  listProfiles: vi.fn().mockResolvedValue([
    {
      id: "p-1",
      name: "devbox",
      windowsPath: "C:\\projects\\devbox",
      wsl: { distro: "Ubuntu", path: "/mnt/e/projects/devbox" },
      gitRoot: "C:\\projects\\devbox",
      expectedPorts: [1420],
      runManagerServiceIds: ["devbox-dev"],
      environment: null,
    },
  ]),
  createProfile: vi.fn(),
  createProfileFromTemplate: vi.fn(),
  createProfileTemplate: vi.fn(),
  currentWorkspaceRun: vi.fn().mockResolvedValue(null),
  dispatchWorkspaceTaskControl: vi.fn(),
  deleteProfile: vi.fn(),
  deleteProfileTemplate: vi.fn(),
  dependencyHealth: vi.fn().mockResolvedValue({ profileId: "p-1", ready: true, items: [] }),
  getWorkspaceTaskControlReceipt: vi.fn().mockResolvedValue(null),
  listWorkspaceTaskControls: vi.fn().mockResolvedValue([]),
  listProfileTemplates: vi.fn().mockResolvedValue({ revision: "a".repeat(64), templates: [] }),
  updateProfile: vi.fn(),
  updateProfileTemplate: vi.fn(),
  projectHealth: vi.fn().mockResolvedValue({ profileId: "p-1", items: [] }),
  retryWorkspace: vi.fn(),
  startWorkspace: vi.fn(),
  stopWorkspace: vi.fn(),
  workspacePreflight: vi.fn().mockResolvedValue({ profileId: "p-1", ready: true, items: [] }),
  profileOpenTargets: vi.fn().mockResolvedValue([]),
  profileCopyPath: vi.fn(),
  openProfileIn: vi.fn(),
  packageDependencySummary: vi.fn().mockImplementation(async (profileId: string) => ({
    profileId,
    source: "Repo Manager dependency-summary/v1",
    status: "missing",
    producerVersion: null,
    freshnessMs: null,
    revision: null,
    packageCount: 0,
    directCount: 0,
    transitiveCount: 0,
    duplicateCount: 0,
    unresolvedDependencyCount: 0,
    missingLockfileCount: 0,
    staleLockfileCount: 0,
    unsupportedCount: 0,
    invalidCount: 0,
    truncated: false,
    ecosystems: [],
  })),
  previewProjectEnvironment: vi.fn(),
  takePendingOpen: vi.fn().mockImplementation(async () => {
    mocks.order.push("take");
    return null;
  }),
  onOpenRequest: vi.fn().mockImplementation(async (handler: (request: OpenRequest) => void) => {
    mocks.order.push("listen");
    mocks.openHandler = handler;
    return () => undefined;
  }),
}));

const takePendingOpenMock = vi.mocked(takePendingOpen);
const onOpenRequestMock = vi.mocked(onOpenRequest);

beforeEach(() => {
  mocks.openHandler = null;
  mocks.order.length = 0;
  takePendingOpenMock.mockClear();
  onOpenRequestMock.mockClear();
});

afterEach(() => cleanup());

describe("App app-link delivery", () => {
  it("listens before the initial take and consumes the pending slot on relaunch", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.openHandler).not.toBeNull());
    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(1));
    expect(mocks.order.slice(0, 2)).toEqual(["listen", "take"]);

    takePendingOpenMock.mockResolvedValueOnce({
      target: { kind: "path", path: "/fresh/project", line: null, column: null },
      from: "repo-manager",
    });
    await act(async () => {
      mocks.openHandler?.({ target: { kind: "query", text: "stale" }, from: "test" });
    });

    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(2));
    expect(await screen.findByDisplayValue("/fresh/project")).toBeTruthy();
  });

  it("consumes a cold-start request when app-link listener registration fails", async () => {
    onOpenRequestMock.mockRejectedValueOnce(new Error("listener unavailable"));
    takePendingOpenMock.mockResolvedValueOnce({
      target: { kind: "path", path: "/fresh/project", line: null, column: null },
      from: "repo-manager",
    });

    render(<App />);

    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(1));
    expect(await screen.findByDisplayValue("/fresh/project")).toBeTruthy();
  });
});
