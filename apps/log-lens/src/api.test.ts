import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  acceptLogSource,
  classifyHandoffError,
  discardLogSource,
  HandoffApiError,
  handoffErrorCode,
  onOpenRequest,
  previewLogSource,
  renewLogSource,
  sendSelectionToToolbox,
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
    await expect(sendSelectionToToolbox("safe selected logs")).rejects.toThrow("desktop-only");

    const unlisten = await onOpenRequest(vi.fn());
    expect(unlisten()).toBeUndefined();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(listenMock).not.toHaveBeenCalled();
  });

  it("publishes only text through the typed Developer Toolbox command", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
    invokeMock.mockResolvedValueOnce({
      handoffId: "0123456789abcdef0123456789abcdef",
      redacted: true,
    });

    await expect(sendSelectionToToolbox("selected export\n")).resolves.toEqual({
      handoffId: "0123456789abcdef0123456789abcdef",
      redacted: true,
    });
    expect(invokeMock).toHaveBeenCalledWith("send_selection_to_toolbox", {
      text: "selected export\n",
    });
  });

  it("rejects malformed Toolbox dispatch responses", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
    invokeMock.mockResolvedValueOnce({ handoffId: "handoff-1", redacted: "yes" });

    await expect(sendSelectionToToolbox("selected export")).rejects.toThrow(
      "Developer Toolbox handoff response was invalid.",
    );
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
    await expect(renewLogSource("a".repeat(32))).rejects.toMatchObject({
      code: "handoff-response-invalid",
    });
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

    await expect(previewLogSource("a".repeat(32))).rejects.toMatchObject({
      code: "handoff-response-invalid",
    });
    await expect(acceptLogSource("a".repeat(32))).rejects.toMatchObject({
      code: "handoff-response-invalid",
    });
    await expect(acceptLogSource("a".repeat(32))).rejects.toMatchObject({
      code: "handoff-response-invalid",
    });
  });

  it("classifies only fixed terminal and retryable codes", () => {
    expect(classifyHandoffError(new HandoffApiError("handoff-missing"))).toBe("terminal");
    expect(classifyHandoffError(new HandoffApiError("handoff-expired"))).toBe("terminal");
    expect(classifyHandoffError(new HandoffApiError("handoff-lease-expired"))).toBe("terminal");
    expect(classifyHandoffError(new HandoffApiError("handoff-restore-failed"))).toBe("retryable");
    expect(classifyHandoffError(new HandoffApiError("handoff-storage-failed"))).toBe("retryable");
    expect(handoffErrorCode(new Error("C:\\Users\\alice\\secret.log"))).toBeNull();
    expect(handoffErrorCode(new Error("handoff-expired: C:\\secret.log"))).toBeNull();
  });

  it("sanitizes native rejection details into a fixed retryable code", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
    invokeMock.mockRejectedValueOnce(new Error("storage failed at C:\\Users\\alice\\token.log"));

    const rejection = discardLogSource("a".repeat(32));
    await expect(rejection).rejects.toMatchObject({
      name: "HandoffApiError",
      code: "handoff-restore-failed",
      message: "handoff-restore-failed",
    });
    await rejection.catch((error: unknown) => {
      expect(error instanceof Error ? error.message : String(error)).toBe("handoff-restore-failed");
    });
  });
});
