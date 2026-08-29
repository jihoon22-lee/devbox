import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, isTauriMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  isTauriMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("./lib/isTauri", () => ({ isTauri: isTauriMock }));

import {
  connectMcpHttp,
  invokeMcpHttp,
  safeMcpErrorCode,
} from "./mcpApi";

const timeline = [
  {
    sequence: 1,
    offsetMs: 0,
    direction: "outgoing",
    kind: "request",
    method: "server/discover",
    requestId: "discover-1",
    payload: {},
  },
  {
    sequence: 2,
    offsetMs: 1,
    direction: "incoming",
    kind: "response",
    method: null,
    requestId: "discover-1",
    payload: { resultType: "complete" },
  },
];

const invokeTimeline = (method: string, requestId: string) => [
  {
    sequence: 1,
    offsetMs: 0,
    direction: "outgoing",
    kind: "request",
    method,
    requestId,
    payload: {},
  },
  {
    sequence: 2,
    offsetMs: 1,
    direction: "incoming",
    kind: "response",
    method: null,
    requestId,
    payload: {},
  },
];

beforeEach(() => {
  invokeMock.mockReset();
  isTauriMock.mockReset().mockReturnValue(true);
});

describe("MCP typed IPC boundary", () => {
  it("never sends MCP network work from browser preview", async () => {
    isTauriMock.mockReturnValue(false);
    await expect(connectMcpHttp({
      endpoint: "https://example.test/mcp",
      era: "auto",
      headers: [],
      timeoutMs: 10_000,
    }, [])).rejects.toThrow("native_required");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("accepts the bounded connection contract and rejects a modern session", async () => {
    const fixture = {
      connectionId: "a".repeat(32),
      sessionManaged: false,
      server: {
        era: "modern",
        protocolVersion: "2026-07-28",
        serverName: "fixture",
        serverVersion: "1",
        capabilities: { tools: {} },
        supportedVersions: ["2026-07-28"],
      },
      timeline,
    };
    invokeMock.mockResolvedValueOnce(fixture);
    await expect(connectMcpHttp({
      endpoint: "https://example.test/mcp",
      era: "auto",
      headers: [],
      timeoutMs: 10_000,
    }, [])).resolves.toEqual(fixture);

    invokeMock.mockResolvedValueOnce({ ...fixture, sessionManaged: true });
    await expect(connectMcpHttp({
      endpoint: "https://example.test/mcp",
      era: "auto",
      headers: [],
      timeoutMs: 10_000,
    }, [])).rejects.toThrow("mcp_message_invalid");
    expect(invokeMock).toHaveBeenNthCalledWith(3, "disconnect_mcp_http", {
      connectionId: "a".repeat(32),
    });

    invokeMock.mockResolvedValueOnce({
      ...fixture,
      server: { ...fixture.server, protocolVersion: "2025-11-25" },
    });
    await expect(connectMcpHttp({
      endpoint: "https://example.test/mcp",
      era: "auto",
      headers: [],
      timeoutMs: 10_000,
    }, [])).rejects.toThrow("mcp_message_invalid");
    expect(invokeMock).toHaveBeenNthCalledWith(5, "disconnect_mcp_http", {
      connectionId: "a".repeat(32),
    });
  });

  it("rejects malformed timeline order and maps unknown native text to a stable code", async () => {
    invokeMock.mockResolvedValueOnce({
      result: { tools: [] },
      errorCode: null,
      rpcErrorCode: null,
      nextCursor: null,
      timeline: timeline.map((entry) => ({ ...entry, sequence: entry.sequence + 1 })),
    });
    await expect(invokeMcpHttp("a".repeat(32), "mcp-1", "tools/list", {}))
      .rejects.toThrow("mcp_message_invalid");
    expect(safeMcpErrorCode(new Error("C:\\Users\\name\\secret")))
      .toBe("mcp_transport_failed");
    expect(safeMcpErrorCode(new Error("mcp_request_timeout")))
      .toBe("mcp_request_timeout");
  });

  it("rejects internally inconsistent error and pagination projections", async () => {
    invokeMock.mockResolvedValueOnce({
      result: null,
      errorCode: "mcp_server_error",
      rpcErrorCode: null,
      nextCursor: null,
      timeline,
    });
    await expect(invokeMcpHttp("a".repeat(32), "mcp-1", "tools/call", {}))
      .rejects.toThrow("mcp_message_invalid");

    invokeMock.mockResolvedValueOnce({
      result: null,
      errorCode: "mcp_server_error",
      rpcErrorCode: -32_000,
      nextCursor: "must-not-survive-an-error",
      timeline,
    });
    await expect(invokeMcpHttp("a".repeat(32), "mcp-2", "tools/call", {}))
      .rejects.toThrow("mcp_message_invalid");

    invokeMock.mockResolvedValueOnce({
      result: { tools: [], nextCursor: "cursor-from-result" },
      errorCode: null,
      rpcErrorCode: null,
      nextCursor: "different-projection",
      timeline,
    });
    await expect(invokeMcpHttp("a".repeat(32), "mcp-3", "tools/list", {}))
      .rejects.toThrow("mcp_message_invalid");

    invokeMock.mockResolvedValueOnce({
      result: { tools: [], nextCursor: "[PRESENT]" },
      errorCode: null,
      rpcErrorCode: null,
      nextCursor: "opaque-cursor",
      timeline: invokeTimeline("tools/list", "mcp-4"),
    });
    await expect(invokeMcpHttp("a".repeat(32), "mcp-4", "tools/list", {}))
      .resolves.toMatchObject({ nextCursor: "opaque-cursor" });

    invokeMock.mockResolvedValueOnce({
      result: { tools: [], nextCursor: "opaque-cursor" },
      errorCode: null,
      rpcErrorCode: null,
      nextCursor: "opaque-cursor",
      timeline: invokeTimeline("tools/list", "mcp-5"),
    });
    await expect(invokeMcpHttp("a".repeat(32), "mcp-5", "tools/list", {}))
      .rejects.toThrow("mcp_message_invalid");
  });
});
