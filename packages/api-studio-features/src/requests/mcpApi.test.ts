import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, isTauriMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  isTauriMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("./lib/isTauri", () => ({ isTauri: isTauriMock }));

import {
  authorizeMcpHttp,
  cancelMcpOAuth,
  cancelMcpStdio,
  connectMcpStdio,
  connectMcpHttp,
  disconnectMcpStdio,
  invokeMcpStdio,
  listMcpOAuthGrants,
  pickMcpStdioCwd,
  pickMcpStdioExecutable,
  invokeMcpHttp,
  revokeMcpOAuthGrant,
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

  it("wraps native selection commands, including a cancelled picker, without raw paths", async () => {
    invokeMock.mockResolvedValueOnce({
      selectionId: "b".repeat(32),
      kind: "executable",
      label: "fixture MCP.exe",
      expiresAtMs: 1_000,
      path: "C:\\Users\\secret\\fixture MCP.exe",
    });
    await expect(pickMcpStdioExecutable()).resolves.toEqual({
      selectionId: "b".repeat(32),
      kind: "executable",
      label: "fixture MCP.exe",
      expiresAtMs: 1_000,
    });
    expect(invokeMock).toHaveBeenCalledWith("pick_mcp_stdio_executable");

    invokeMock.mockResolvedValueOnce(null);
    await expect(pickMcpStdioCwd()).resolves.toBeNull();
    expect(invokeMock).toHaveBeenLastCalledWith("pick_mcp_stdio_cwd");

    invokeMock.mockResolvedValueOnce({
      selectionId: "b".repeat(32),
      kind: "executable",
      label: "C:\\Users\\secret\\fixture.exe",
      expiresAtMs: 1_000,
    });
    await expect(pickMcpStdioExecutable()).rejects.toThrow("mcp_stdio_selection_invalid");
  });

  it("sends the structured stdio profile and routes invoke/cancel/disconnect payloads", async () => {
    const profile = {
      executableSelectionId: "b".repeat(32),
      cwdSelectionId: "c".repeat(32),
      era: "legacy" as const,
      args: ["--fixture", "two words"],
      environment: [{ childName: "MCP_TOKEN", sourceName: "API_TOKEN" }],
      timeoutMs: 20_000,
    };
    const environment = [{ key: "API_TOKEN", value: "sealed", secret: true }];
    const connection = {
      connectionId: "a".repeat(32),
      sessionManaged: false,
      server: {
        era: "modern" as const,
        protocolVersion: "2026-07-28",
        serverName: "fixture",
        serverVersion: "1",
        capabilities: { tools: {} },
        supportedVersions: ["2026-07-28"],
      },
      timeline,
    };
    invokeMock.mockResolvedValueOnce(connection);
    await expect(connectMcpStdio(profile, environment)).resolves.toEqual(connection);
    expect(invokeMock).toHaveBeenCalledWith("connect_mcp_stdio", { profile, environment });

    invokeMock.mockResolvedValueOnce({
      result: { resultType: "complete" },
      errorCode: null,
      rpcErrorCode: null,
      nextCursor: null,
      timeline: invokeTimeline("tools/call", "mcp-stdio-1"),
    });
    await expect(invokeMcpStdio(
      connection.connectionId,
      "mcp-stdio-1",
      "tools/call",
      { name: "fixture", arguments: {} },
    )).resolves.toMatchObject({ result: { resultType: "complete" } });
    expect(invokeMock).toHaveBeenLastCalledWith("invoke_mcp_stdio", {
      connectionId: connection.connectionId,
      requestId: "mcp-stdio-1",
      method: "tools/call",
      params: { name: "fixture", arguments: {} },
    });

    invokeMock.mockResolvedValueOnce(true);
    await expect(cancelMcpStdio(connection.connectionId, "mcp-stdio-1")).resolves.toBe(true);
    expect(invokeMock).toHaveBeenLastCalledWith("cancel_mcp_stdio", {
      connectionId: connection.connectionId,
      requestId: "mcp-stdio-1",
    });

    invokeMock.mockResolvedValueOnce(undefined);
    await expect(disconnectMcpStdio(connection.connectionId)).resolves.toBeUndefined();
    expect(invokeMock).toHaveBeenLastCalledWith("disconnect_mcp_stdio", {
      connectionId: connection.connectionId,
    });
  });

  it("keeps all stdio commands native-only", async () => {
    isTauriMock.mockReturnValue(false);
    await expect(pickMcpStdioExecutable()).rejects.toThrow("native_required");
    await expect(connectMcpStdio({
      executableSelectionId: "b".repeat(32),
      era: "auto",
      args: [],
      environment: [],
      timeoutMs: 10_000,
    }, [])).rejects.toThrow("native_required");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("owns the OAuth command payloads and returns only bounded grant projections", async () => {
    const grant = {
      grantId: "d".repeat(32),
      issuer: "https://issuer.example",
      resource: "https://example.test/mcp",
      clientId: "public-client",
      scopes: ["tools"],
      expiresAtMs: null,
      status: "active",
      accessToken: "must-not-survive-projection",
      callbackCode: "must-not-survive-projection",
    };
    invokeMock.mockResolvedValueOnce(grant);
    const authorized = await authorizeMcpHttp(
      "oauth-request-1",
      "https://example.test/mcp",
      null,
      "public-client",
      ["tools"],
    );
    expect(authorized).toEqual({
      grantId: "d".repeat(32),
      issuer: "https://issuer.example",
      resource: "https://example.test/mcp",
      clientId: "public-client",
      scopes: ["tools"],
      expiresAtMs: null,
      status: "active",
    });
    expect(authorized).not.toHaveProperty("accessToken");
    expect(authorized).not.toHaveProperty("callbackCode");
    expect(invokeMock).toHaveBeenLastCalledWith("authorize_mcp_http", {
      requestId: "oauth-request-1",
      endpoint: "https://example.test/mcp",
      issuer: null,
      clientId: "public-client",
      scopes: ["tools"],
    });

    invokeMock.mockResolvedValueOnce(true);
    await expect(cancelMcpOAuth("oauth-request-1")).resolves.toBe(true);
    expect(invokeMock).toHaveBeenLastCalledWith("cancel_mcp_oauth", {
      requestId: "oauth-request-1",
    });

    invokeMock.mockResolvedValueOnce([grant]);
    await expect(listMcpOAuthGrants()).resolves.toEqual([authorized]);
    expect(invokeMock).toHaveBeenLastCalledWith("list_mcp_oauth_grants");

    invokeMock.mockResolvedValueOnce({ remoteRevoked: false, removedLocal: true });
    await expect(revokeMcpOAuthGrant("d".repeat(32), true)).resolves.toEqual({
      remoteRevoked: false,
      removedLocal: true,
    });
    expect(invokeMock).toHaveBeenLastCalledWith("revoke_mcp_oauth_grant", {
      grantId: "d".repeat(32),
      removeLocalOnRemoteFailure: true,
    });
  });

  it("rejects malformed OAuth inputs and duplicate native projections", async () => {
    await expect(authorizeMcpHttp(
      "bad request",
      "https://example.test/mcp",
      null,
      "public-client",
      [],
    )).rejects.toThrow("mcp_oauth_request_invalid");
    expect(invokeMock).not.toHaveBeenCalled();

    const malformed = {
      grantId: "d".repeat(32),
      issuer: "https://issuer.example",
      resource: "https://example.test/mcp",
      clientId: "public-client",
      scopes: ["tools"],
      expiresAtMs: null,
      status: "active",
    };
    invokeMock.mockResolvedValueOnce([malformed, malformed]);
    await expect(listMcpOAuthGrants()).rejects.toThrow("mcp_oauth_storage_failed");

    invokeMock.mockResolvedValueOnce({
      ...malformed,
      scopes: ["tools", "tools"],
      token: "must-not-be-accepted",
    });
    await expect(authorizeMcpHttp(
      "oauth-request-2",
      "https://example.test/mcp",
      undefined,
      "public-client",
      [],
    )).rejects.toThrow("mcp_oauth_request_invalid");
  });
});
