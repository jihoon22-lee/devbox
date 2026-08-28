import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  acceptLogSource,
  discardLogSource,
  onOpenRequest,
  previewLogSource,
  renewLogSource,
  takePendingOpen,
} from "./api";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const invokeMock = vi.mocked(invoke);
const listenMock = vi.mocked(listen);

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.clearAllMocks();
});

describe("Log Lens handoff API", () => {
  it("keeps browser fixtures from publishing, claiming, or listening for handoffs", async () => {
    expect(await takePendingOpen()).toBeNull();
    await expect(previewLogSource("a".repeat(32))).rejects.toThrow("desktop-only");
    await expect(acceptLogSource("a".repeat(32))).rejects.toThrow("desktop-only");
    await expect(discardLogSource("a".repeat(32))).rejects.toThrow("desktop-only");
    await expect(renewLogSource("a".repeat(32))).rejects.toThrow("desktop-only");

    const unlisten = await onOpenRequest(vi.fn());
    expect(unlisten()).toBeUndefined();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(listenMock).not.toHaveBeenCalled();
  });

  it("rejects malformed native responses before they reach the source UI", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
    invokeMock
      .mockResolvedValueOnce({
        target: { kind: "handoff", handoffKind: "log-source/v1", id: "../secret" },
        from: "run-manager",
      })
      .mockResolvedValueOnce({
        id: "a".repeat(32),
        kind: "log-source/v1",
        sourceApp: "run-manager",
        expiresAtMs: 10_000,
        leaseUntilMs: 5_000,
        source: {
          sourceId: "log-source:0123456789abcdef",
          kind: "run",
          displayName: "Run Manager handoff",
          readOnly: true,
          handoff: true,
        },
      })
      .mockResolvedValueOnce({
        kind: "wslFile",
        distro: "Ubuntu",
        path: "/var/log/app.log",
      })
      .mockResolvedValueOnce({
        kind: "wslJournal",
        distro: "Ubuntu",
        unit: null,
      })
      .mockResolvedValueOnce({ leaseUntilMs: "not-a-number" });

    await expect(takePendingOpen()).resolves.toBeNull();
    await expect(previewLogSource("a".repeat(32))).resolves.toMatchObject({
      sourceApp: "run-manager",
    });
    await expect(acceptLogSource("a".repeat(32))).resolves.toEqual({
      kind: "wslFile",
      distro: "Ubuntu",
      path: "/var/log/app.log",
    });
    await expect(acceptLogSource("a".repeat(32))).resolves.toEqual({
      kind: "wslJournal",
      distro: "Ubuntu",
    });
    await expect(renewLogSource("a".repeat(32))).rejects.toThrow("response is invalid");
  });

  it("binds preview responses to the requested opaque id and rejects unsafe WSL values", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
    invokeMock
      .mockResolvedValueOnce({
        id: "b".repeat(32),
        kind: "log-source/v1",
        sourceApp: "run-manager",
        expiresAtMs: 10_000,
        leaseUntilMs: 5_000,
        source: {
          sourceId: "log-source:0123456789abcdef",
          kind: "run",
          displayName: "Run Manager handoff",
          readOnly: true,
          handoff: true,
        },
      })
      .mockResolvedValueOnce({ kind: "wslFile", distro: "--help", path: "/var/log/app.log" })
      .mockResolvedValueOnce({ kind: "wslFile", distro: "Ubuntu", path: "/" });

    await expect(previewLogSource("a".repeat(32))).rejects.toThrow("response is invalid");
    await expect(acceptLogSource("a".repeat(32))).rejects.toThrow("response is invalid");
    await expect(acceptLogSource("a".repeat(32))).rejects.toThrow("response is invalid");
  });
});
