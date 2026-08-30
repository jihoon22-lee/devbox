import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ProtocolLab } from "./ProtocolLab";

const mocks = vi.hoisted(() => ({
  connect: vi.fn(),
  connectStdio: vi.fn(),
  invoke: vi.fn(),
  invokeStdio: vi.fn(),
  cancel: vi.fn(),
  cancelStdio: vi.fn(),
  disconnect: vi.fn(),
  disconnectStdio: vi.fn(),
  pickExecutable: vi.fn(),
  pickCwd: vi.fn(),
  authorize: vi.fn(),
  cancelOAuth: vi.fn(),
  listGrants: vi.fn(),
  revokeGrant: vi.fn(),
  nextId: 0,
}));

vi.mock("./api", () => ({
  connectMcpHttp: mocks.connect,
  invokeMcpHttp: mocks.invoke,
  cancelMcpHttp: mocks.cancel,
  disconnectMcpHttp: mocks.disconnect,
  nextMcpRequestId: () => `mcp-${++mocks.nextId}`,
  safeMcpErrorCode: (cause: unknown) => cause instanceof Error ? cause.message : String(cause),
}));

vi.mock("./mcpApi", () => ({
  connectMcpStdio: mocks.connectStdio,
  invokeMcpStdio: mocks.invokeStdio,
  cancelMcpStdio: mocks.cancelStdio,
  disconnectMcpStdio: mocks.disconnectStdio,
  pickMcpStdioExecutable: mocks.pickExecutable,
  pickMcpStdioCwd: mocks.pickCwd,
  authorizeMcpHttp: mocks.authorize,
  cancelMcpOAuth: mocks.cancelOAuth,
  listMcpOAuthGrants: mocks.listGrants,
  revokeMcpOAuthGrant: mocks.revokeGrant,
}));

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
  timeline: [
    {
      sequence: 1,
      offsetMs: 0,
      direction: "outgoing" as const,
      kind: "request" as const,
      method: "server/discover",
      requestId: "discover-1",
      payload: {},
    },
  ],
};

function invokeResult(result: unknown, nextCursor: string | null = null) {
  return {
    result,
    errorCode: null,
    rpcErrorCode: null,
    nextCursor,
    timeline: connection.timeline,
  };
}

async function connect(): Promise<void> {
  fireEvent.change(screen.getByLabelText("MCP endpoint"), {
    target: { value: "https://example.test/mcp" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Connect" }));
  await screen.findByText("fixture");
}

const executableSelection = {
  selectionId: "b".repeat(32),
  kind: "executable" as const,
  label: "fixture MCP.exe",
  expiresAtMs: Date.now() + 60_000,
};

const cwdSelection = {
  selectionId: "c".repeat(32),
  kind: "directory" as const,
  label: "fixture cwd",
  expiresAtMs: Date.now() + 60_000,
};

const oauthGrant = {
  grantId: "d".repeat(32),
  issuer: "https://issuer.example",
  resource: "https://example.test/mcp",
  clientId: "public-client",
  scopes: ["tools"],
  expiresAtMs: null,
  status: "active" as const,
};

async function connectStdio(): Promise<void> {
  fireEvent.change(screen.getByLabelText("MCP transport"), {
    target: { value: "stdio" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Choose executable" }));
  await screen.findByText(executableSelection.label);
  fireEvent.click(screen.getByRole("button", { name: "Connect" }));
  await screen.findByText("fixture");
}

beforeEach(() => {
  mocks.connect.mockReset().mockResolvedValue(connection);
  mocks.connectStdio.mockReset().mockResolvedValue(connection);
  mocks.invoke.mockReset();
  mocks.invokeStdio.mockReset();
  mocks.cancel.mockReset().mockResolvedValue(true);
  mocks.cancelStdio.mockReset().mockResolvedValue(true);
  mocks.disconnect.mockReset().mockResolvedValue(undefined);
  mocks.disconnectStdio.mockReset().mockResolvedValue(undefined);
  mocks.pickExecutable.mockReset().mockResolvedValue(null);
  mocks.pickCwd.mockReset().mockResolvedValue(null);
  mocks.authorize.mockReset().mockResolvedValue(oauthGrant);
  mocks.cancelOAuth.mockReset().mockResolvedValue(true);
  mocks.listGrants.mockReset().mockResolvedValue([]);
  mocks.revokeGrant.mockReset().mockResolvedValue({
    remoteRevoked: true,
    removedLocal: true,
  });
  mocks.nextId = 0;
});

afterEach(() => cleanup());

describe("Protocol Lab", () => {
  it("keeps browser preview native-only", () => {
    render(<ProtocolLab environment={[]} native={false} />);
    expect(screen.getByText(/브라우저 미리보기에서는 MCP 네트워크 요청을 보내지 않습니다/))
      .not.toBeNull();
    expect((screen.getByRole("button", { name: "Connect" }) as HTMLButtonElement).disabled)
      .toBe(true);
    expect(mocks.connect).not.toHaveBeenCalled();
  });

  it("gates capabilities and requires explicit list then schema-valid tool call", async () => {
    mocks.invoke
      .mockResolvedValueOnce(invokeResult({
        resultType: "complete",
        tools: [{
          name: "echo",
          description: "untrusted description",
          inputSchema: {
            type: "object",
            additionalProperties: false,
            required: ["message"],
            properties: { message: { type: "string", minLength: 1 } },
          },
        }],
      }))
      .mockResolvedValueOnce(invokeResult({ resultType: "complete", content: [] }));
    render(<ProtocolLab environment={[]} native />);
    await connect();
    expect(screen.getAllByText(
      "서버가 이 capability를 제공하지 않습니다.",
      { selector: "p" },
    )).toHaveLength(2);
    expect(mocks.invoke).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "목록 조회" }));
    await screen.findByRole("combobox", { name: "MCP tool" });
    const call = screen.getByRole("button", { name: "선택 tool 호출" }) as HTMLButtonElement;
    expect(call.disabled).toBe(true);
    fireEvent.change(screen.getByLabelText("message string"), { target: { value: "hello" } });
    expect(call.disabled).toBe(false);
    fireEvent.click(call);
    await waitFor(() => expect(mocks.invoke).toHaveBeenLastCalledWith(
      connection.connectionId,
      "mcp-2",
      "tools/call",
      { name: "echo", arguments: { message: "hello" } },
    ));
  });

  it("loads pagination only when the user asks and cancels the owned request", async () => {
    let rejectSecond: ((reason: Error) => void) | undefined;
    mocks.invoke
      .mockResolvedValueOnce(invokeResult({
        resultType: "complete",
        tools: [{ name: "one", inputSchema: { type: "object", properties: {} } }],
      }, "cursor-1"))
      .mockImplementationOnce(() => new Promise((_, reject) => { rejectSecond = reject; }));
    render(<ProtocolLab environment={[]} native />);
    await connect();
    fireEvent.click(screen.getByRole("button", { name: "목록 조회" }));
    await screen.findByRole("button", { name: "목록 다음 페이지" });
    expect(mocks.invoke).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "목록 다음 페이지" }));
    await screen.findByText(/tools\/list 실행 중/);
    expect(mocks.invoke).toHaveBeenLastCalledWith(
      connection.connectionId,
      "mcp-2",
      "tools/list",
      { cursor: "cursor-1" },
    );
    fireEvent.click(screen.getByRole("button", { name: "취소" }));
    await waitFor(() => expect(mocks.cancel).toHaveBeenCalledWith(
      connection.connectionId,
      "mcp-2",
    ));
    rejectSecond?.(new Error("mcp_stdio_request_cancelled"));
  });

  it("calls a tool whose valid root object schema omits properties", async () => {
    mocks.invoke
      .mockResolvedValueOnce(invokeResult({
        resultType: "complete",
        tools: [{ name: "ping", inputSchema: { type: "object" } }],
      }))
      .mockResolvedValueOnce(invokeResult({ resultType: "complete", content: [] }));
    render(<ProtocolLab environment={[]} native />);
    await connect();
    fireEvent.click(screen.getByRole("button", { name: "목록 조회" }));
    await screen.findByText("이 tool은 arguments가 없습니다.");
    const call = screen.getByRole("button", { name: "선택 tool 호출" }) as HTMLButtonElement;
    expect(call.disabled).toBe(false);
    fireEvent.click(call);
    await waitFor(() => expect(mocks.invoke).toHaveBeenLastCalledWith(
      connection.connectionId,
      "mcp-2",
      "tools/call",
      { name: "ping", arguments: {} },
    ));
  });

  it("closes a stale native connection without replaying the request", async () => {
    mocks.invoke.mockRejectedValueOnce(new Error("mcp_connection_stale"));
    render(<ProtocolLab environment={[]} native />);
    await connect();
    fireEvent.click(screen.getByRole("button", { name: "목록 조회" }));
    await screen.findByText(/연결이 닫혔거나 오래되었습니다/);
    expect(screen.getByRole("button", { name: "Connect" })).not.toBeNull();
    expect(screen.queryByText("fixture")).toBeNull();
    expect(mocks.invoke).toHaveBeenCalledTimes(1);
  });

  it("reads resources and gets prompts only after separate explicit actions", async () => {
    mocks.connect.mockResolvedValueOnce({
      ...connection,
      server: {
        ...connection.server,
        capabilities: { resources: {}, prompts: {} },
      },
    });
    mocks.invoke
      .mockResolvedValueOnce(invokeResult({
        resultType: "complete",
        resources: [{ uri: "fixture://resource", name: "Fixture resource" }],
      }))
      .mockResolvedValueOnce(invokeResult({
        resultType: "complete",
        contents: [{ uri: "fixture://resource", text: "body" }],
      }))
      .mockResolvedValueOnce(invokeResult({
        resultType: "complete",
        prompts: [{ name: "draft", arguments: [{ name: "topic", required: true }] }],
      }))
      .mockResolvedValueOnce(invokeResult({
        resultType: "complete",
        description: "drafted",
        messages: [],
      }));

    render(<ProtocolLab environment={[]} native />);
    await connect();
    const resources = screen.getByRole("heading", { name: "Resources" }).closest("section");
    const prompts = screen.getByRole("heading", { name: "Prompts" }).closest("section");
    if (!resources || !prompts) throw new Error("explorer section missing");

    fireEvent.click(within(resources).getByRole("button", { name: "Resource 조회" }));
    await within(resources).findByRole("option", { name: "Fixture resource" });
    expect(mocks.invoke).toHaveBeenLastCalledWith(
      connection.connectionId,
      "mcp-1",
      "resources/list",
      {},
    );
    fireEvent.click(within(resources).getByRole("button", { name: "Resource 읽기" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenLastCalledWith(
      connection.connectionId,
      "mcp-2",
      "resources/read",
      { uri: "fixture://resource" },
    ));

    fireEvent.click(within(prompts).getByRole("button", { name: "목록 조회" }));
    const topic = await within(prompts).findByLabelText(/topic/);
    fireEvent.change(topic, { target: { value: "release" } });
    fireEvent.click(within(prompts).getByRole("button", { name: "Prompt 가져오기" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenLastCalledWith(
      connection.connectionId,
      "mcp-4",
      "prompts/get",
      { name: "draft", arguments: { topic: "release" } },
    ));
  });

  it("shows only safe native stdio selection labels and handles picker cancellation", async () => {
    render(<ProtocolLab environment={[]} native />);
    fireEvent.change(screen.getByLabelText("MCP transport"), {
      target: { value: "stdio" },
    });
    expect(screen.getByText(/WSL stdio와 shell command string은 지원하지 않습니다/)).not.toBeNull();
    expect(screen.queryByLabelText("MCP endpoint")).toBeNull();
    expect(screen.queryByLabelText(/path/i)).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Choose executable" }));
    fireEvent.click(screen.getByRole("button", { name: "Choose cwd" }));
    await waitFor(() => {
      expect(mocks.pickExecutable).toHaveBeenCalledTimes(1);
      expect(mocks.pickCwd).toHaveBeenCalledTimes(1);
    });
    expect(screen.getAllByText("선택하지 않음")).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Choose executable" }).getAttribute("aria-label"))
      .toBe("Choose executable");
    expect(screen.getByRole("button", { name: "Choose cwd" }).getAttribute("aria-label"))
      .toBe("Choose cwd");
  });

  it("builds a structured stdio profile without accepting raw paths", async () => {
    const environment = [{ key: "API_TOKEN", value: "sealed", secret: true }];
    mocks.pickExecutable.mockResolvedValueOnce(executableSelection);
    mocks.pickCwd.mockResolvedValueOnce(cwdSelection);
    render(<ProtocolLab environment={environment} native />);

    fireEvent.change(screen.getByLabelText("MCP transport"), {
      target: { value: "stdio" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Choose executable" }));
    await screen.findByText(executableSelection.label);
    fireEvent.click(screen.getByRole("button", { name: "Choose cwd" }));
    await screen.findByText(cwdSelection.label);

    fireEvent.click(screen.getByRole("button", { name: "Add argument" }));
    fireEvent.change(screen.getByLabelText("stdio argument 1"), {
      target: { value: "--verbose" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add environment binding" }));
    fireEvent.change(screen.getByLabelText("stdio child name 1"), {
      target: { value: "MCP_TOKEN" },
    });
    fireEvent.change(screen.getByLabelText("stdio source name 1"), {
      target: { value: "API_TOKEN" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await screen.findByText("fixture");

    expect(mocks.connectStdio).toHaveBeenCalledWith(
      {
        executableSelectionId: executableSelection.selectionId,
        cwdSelectionId: cwdSelection.selectionId,
        era: "auto",
        args: ["--verbose"],
        environment: [{ childName: "MCP_TOKEN", sourceName: "API_TOKEN" }],
        timeoutMs: 10_000,
      },
      environment,
    );
    expect(screen.queryByDisplayValue(/fixture MCP\.exe/)).toBeNull();
  });

  it("routes stdio explorer requests, cancellation, and disconnect to native commands", async () => {
    let rejectSecond: ((reason: Error) => void) | undefined;
    mocks.pickExecutable.mockResolvedValueOnce(executableSelection);
    mocks.invokeStdio
      .mockResolvedValueOnce(invokeResult({
        resultType: "complete",
        tools: [{ name: "one", inputSchema: { type: "object", properties: {} } }],
      }, "cursor-1"))
      .mockImplementationOnce(() => new Promise((_, reject) => { rejectSecond = reject; }));
    render(<ProtocolLab environment={[]} native />);
    await connectStdio();

    fireEvent.click(screen.getByRole("button", { name: "목록 조회" }));
    await screen.findByRole("button", { name: "목록 다음 페이지" });
    expect(mocks.invokeStdio).toHaveBeenLastCalledWith(
      connection.connectionId,
      "mcp-1",
      "tools/list",
      {},
    );
    fireEvent.click(screen.getByRole("button", { name: "목록 다음 페이지" }));
    await screen.findByText(/tools\/list 실행 중/);
    fireEvent.click(screen.getByRole("button", { name: "취소" }));
    await waitFor(() => expect(mocks.cancelStdio).toHaveBeenCalledWith(
      connection.connectionId,
      "mcp-2",
    ));
    rejectSecond?.(new Error("mcp_stdio_request_cancelled"));
    await waitFor(() => expect(screen.queryByText("fixture")).toBeNull());
    mocks.connectStdio.mockResolvedValueOnce(connection);
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await screen.findByText("fixture");
    fireEvent.click(screen.getByRole("button", { name: "연결 해제" }));
    await waitFor(() => expect(mocks.disconnectStdio).toHaveBeenCalledWith(connection.connectionId));
  });

  it("disconnects and resets the explorer before switching transport", async () => {
    render(<ProtocolLab environment={[]} native />);
    await connect();
    expect(screen.getByText("fixture")).not.toBeNull();
    fireEvent.change(screen.getByLabelText("MCP transport"), {
      target: { value: "stdio" },
    });
    await waitFor(() => expect(mocks.disconnect).toHaveBeenCalledWith(connection.connectionId));
    expect(screen.queryByText("fixture")).toBeNull();
    expect((screen.getByRole("button", { name: "Choose executable" }) as HTMLButtonElement).disabled)
      .toBe(false);
  });

  it("treats stdio stale-after-cancel as successful transport-switch cleanup", async () => {
    mocks.pickExecutable.mockResolvedValueOnce(executableSelection);
    mocks.invokeStdio.mockImplementationOnce(() => new Promise(() => undefined));
    mocks.disconnectStdio.mockRejectedValueOnce(new Error("mcp_stdio_connection_stale"));
    render(<ProtocolLab environment={[]} native />);
    await connectStdio();
    fireEvent.click(screen.getByRole("button", { name: "목록 조회" }));
    await screen.findByText(/tools\/list 실행 중/);

    fireEvent.change(screen.getByLabelText("MCP transport"), {
      target: { value: "http" },
    });
    await screen.findByLabelText("MCP endpoint");
    expect(mocks.cancelStdio).toHaveBeenCalledWith(connection.connectionId, "mcp-1");
    expect(screen.queryByText(/native stdio 연결이 닫혔거나 오래되었습니다/)).toBeNull();
  });

  it("authorizes HTTP in the system browser and blocks an Authorization header conflict", async () => {
    render(<ProtocolLab environment={[]} native />);
    fireEvent.change(screen.getByLabelText("MCP endpoint"), {
      target: { value: "https://example.test/mcp" },
    });
    fireEvent.change(screen.getByLabelText("OAuth public client ID"), {
      target: { value: "public-client" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add OAuth scope" }));
    fireEvent.change(screen.getByLabelText("OAuth scope 1"), {
      target: { value: "tools" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Authorize in system browser" }));

    await screen.findByText("Selected OAuth grant");
    expect(mocks.authorize).toHaveBeenCalledWith(
      "mcp-1",
      "https://example.test/mcp",
      null,
      "public-client",
      ["tools"],
    );
    expect(screen.getByText(/Windows DPAPI/)).not.toBeNull();
    expect(screen.queryByText(/accessToken|callbackCode|must-not-survive/)).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "+ Header 추가" }));
    fireEvent.change(screen.getByLabelText("1번 header 이름"), {
      target: { value: "Authorization" },
    });
    expect(screen.getByRole("alert").textContent).toContain("함께 사용할 수 없습니다");
    expect((screen.getByRole("button", { name: "Connect" }) as HTMLButtonElement).disabled)
      .toBe(true);
  });

  it("cancels the exact active OAuth authorization request", async () => {
    let rejectAuthorization: ((reason: Error) => void) | undefined;
    mocks.authorize.mockImplementationOnce(() => new Promise((_, reject) => {
      rejectAuthorization = reject;
    }));
    render(<ProtocolLab environment={[]} native />);
    fireEvent.change(screen.getByLabelText("MCP endpoint"), {
      target: { value: "https://example.test/mcp" },
    });
    fireEvent.change(screen.getByLabelText("OAuth public client ID"), {
      target: { value: "public-client" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Authorize in system browser" }));
    fireEvent.click(await screen.findByRole("button", { name: "Cancel OAuth authorization" }));
    await waitFor(() => expect(mocks.cancelOAuth).toHaveBeenCalledWith("mcp-1"));
    rejectAuthorization?.(new Error("mcp_oauth_cancelled"));
    await screen.findByText(/OAuth authorization을 취소했습니다/);
  });

  it("requires an explicit local-only fallback after remote revoke failure", async () => {
    mocks.listGrants.mockResolvedValueOnce([oauthGrant]);
    mocks.revokeGrant
      .mockRejectedValueOnce(new Error("mcp_oauth_revoke_failed"))
      .mockResolvedValueOnce({ remoteRevoked: false, removedLocal: true });
    render(<ProtocolLab environment={[]} native />);
    fireEvent.click(screen.getByRole("button", { name: "Refresh OAuth grants" }));
    await screen.findByText("Selected OAuth grant");

    fireEvent.click(screen.getByRole("button", { name: "Revoke OAuth grant" }));
    const fallback = await screen.findByRole("button", { name: "Remove OAuth grant locally" });
    expect(mocks.revokeGrant).toHaveBeenLastCalledWith(oauthGrant.grantId, false);
    expect(screen.getByText(/원격에서 revoke하지 못했습니다/)).not.toBeNull();

    fireEvent.click(fallback);
    await waitFor(() => expect(mocks.revokeGrant).toHaveBeenLastCalledWith(
      oauthGrant.grantId,
      true,
    ));
    expect(await screen.findByText(/로컬에서 제거했습니다/)).not.toBeNull();
    expect(screen.queryByText("Selected OAuth grant")).toBeNull();
  });
});
