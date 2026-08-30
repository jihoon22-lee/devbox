import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  acceptLogSource,
  classifyHandoffError,
  discardLogSource,
  HandoffApiError,
  handoffErrorCode,
  listSavedViews,
  onOpenRequest,
  previewLogSource,
  removeSavedView,
  renewLogSource,
  saveSavedView,
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
    await expect(previewLogSource("log-source/v1", "a".repeat(32))).rejects.toThrow("desktop-only");
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
    await expect(previewLogSource("log-source/v1", "a".repeat(32))).resolves.toMatchObject({
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

    await expect(previewLogSource("log-source/v1", "a".repeat(32))).rejects.toMatchObject({
      code: "handoff-response-invalid",
    });
    await expect(acceptLogSource("a".repeat(32))).rejects.toMatchObject({
      code: "handoff-response-invalid",
    });
    await expect(acceptLogSource("a".repeat(32))).rejects.toMatchObject({
      code: "handoff-response-invalid",
    });
  });

  it("accepts only the Run source family when Port Manager routes a verified log handoff", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
    invokeMock
      .mockResolvedValueOnce({
        id: "a".repeat(32),
        kind: "log-source/v1",
        sourceApp: "port-manager",
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
        id: "a".repeat(32),
        kind: "log-source/v1",
        sourceApp: "port-manager",
        expiresAtMs: 10_000,
        leaseUntilMs: 5_000,
        source: {
          sourceId: "log-source:0123456789abcdef",
          kind: "wslFile",
          displayName: "WSL file",
          readOnly: true,
          handoff: true,
        },
      });

    await expect(previewLogSource("log-source/v1", "a".repeat(32))).resolves.toMatchObject({
      sourceApp: "port-manager",
      source: { kind: "run" },
    });
    await expect(previewLogSource("log-source/v1", "a".repeat(32))).rejects.toMatchObject({
      code: "handoff-response-invalid",
    });
  });

  it("accepts the strict Webhook Lab projection without previewing request content", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
    const id = "c".repeat(32);
    invokeMock
      .mockResolvedValueOnce({
        target: { kind: "handoff", handoffKind: "webhook-log/v1", id },
        from: "webhook-lab",
      })
      .mockResolvedValueOnce({
        id,
        kind: "webhook-log/v1",
        sourceApp: "webhook-lab",
        expiresAtMs: 10_000,
        leaseUntilMs: 5_000,
        source: {
          sourceId: "log-source:0123456789abcdef",
          kind: "webhookCapture",
          displayName: "Webhook capture",
          readOnly: true,
          handoff: true,
        },
      })
      .mockResolvedValueOnce({
        kind: "webhookCapture",
        capture: {
          schemaVersion: 1,
          method: "POST",
          target: "/hook?token=[REDACTED]",
          receivedAtMs: 1,
          headerNames: ["content-type", "authorization"],
          bodyPreview: "{\"token\":\"[REDACTED]\"}",
          redacted: true,
          truncated: false,
        },
      })
      .mockResolvedValueOnce({
        kind: "webhookCapture",
        capture: {
          schemaVersion: 1,
          method: "POST",
          target: "/hook",
          receivedAtMs: 1,
          headerNames: [],
          bodyPreview: "safe",
          body: "raw-not-allowed",
          redacted: false,
          truncated: false,
        },
      })
      .mockResolvedValueOnce({
        kind: "webhookCapture",
        capture: {
          schemaVersion: 1,
          method: "GET",
          target: "/hooks/%2e%2e/private",
          receivedAtMs: 1,
          headerNames: [],
          bodyPreview: "safe",
          redacted: false,
          truncated: false,
        },
      })
      .mockResolvedValueOnce({
        kind: "webhookCapture",
        capture: {
          schemaVersion: 1,
          method: "GET",
          target: "/hooks",
          receivedAtMs: 1,
          headerNames: ["Authorization"],
          bodyPreview: "safe",
          redacted: false,
          truncated: false,
        },
      });

    await expect(takePendingOpen()).resolves.toMatchObject({ target: { handoffKind: "webhook-log/v1", id } });
    await expect(previewLogSource("webhook-log/v1", id)).resolves.toEqual(expect.objectContaining({
      sourceApp: "webhook-lab",
      source: expect.objectContaining({ kind: "webhookCapture" }),
    }));
    expect(invokeMock).toHaveBeenCalledWith("preview_log_source", { handoffKind: "webhook-log/v1", id });
    await expect(acceptLogSource(id)).resolves.toMatchObject({
      kind: "webhookCapture",
      capture: { redacted: true },
    });
    await expect(acceptLogSource(id)).rejects.toMatchObject({ code: "handoff-response-invalid" });
    await expect(acceptLogSource(id)).rejects.toMatchObject({ code: "handoff-response-invalid" });
    await expect(acceptLogSource(id)).rejects.toMatchObject({ code: "handoff-response-invalid" });
  });

  it("validates app-local saved-view documents and preserves revision arguments", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
    const view = {
      name: "errors",
      sources: [{ kind: "localFile" as const, path: "C:\\logs\\app.log" }],
      filter: { text: "error", regex: false },
    };
    const nativeView = {
      ...view,
      filter: {
        text: "error",
        regex: false,
        sourceId: null,
        level: null,
        startAt: null,
        endAt: null,
        field: null,
        fieldValue: null,
      },
    };
    invokeMock
      .mockResolvedValueOnce({ schemaVersion: 1, revision: 3, views: [nativeView] })
      .mockResolvedValueOnce({ schemaVersion: 1, revision: 4, views: [nativeView] })
      .mockResolvedValueOnce({ schemaVersion: 1, revision: 5, views: [] })
      .mockResolvedValueOnce({
        schemaVersion: 1,
        revision: 6,
        views: [{ name: "unsafe", sources: [{ kind: "wslFile", distro: "Ubuntu", path: "/secret" }], filter: { text: "", regex: false } }],
      });

    await expect(listSavedViews()).resolves.toMatchObject({ revision: 3, views: [view] });
    await expect(saveSavedView(3, view)).resolves.toMatchObject({ revision: 4 });
    expect(invokeMock).toHaveBeenCalledWith("save_saved_view", { expectedRevision: 3, view });
    await expect(removeSavedView(4, "errors")).resolves.toEqual({ schemaVersion: 1, revision: 5, views: [] });
    expect(invokeMock).toHaveBeenCalledWith("delete_saved_view", { expectedRevision: 4, name: "errors" });
    await expect(listSavedViews()).rejects.toThrow("저장된 뷰 응답이 유효하지 않습니다");
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
