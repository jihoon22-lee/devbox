import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ProtocolLab } from "./ProtocolLab";

const mocks = vi.hoisted(() => ({
  connect: vi.fn(),
  invoke: vi.fn(),
  cancel: vi.fn(),
  disconnect: vi.fn(),
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

beforeEach(() => {
  mocks.connect.mockReset().mockResolvedValue(connection);
  mocks.invoke.mockReset();
  mocks.cancel.mockReset().mockResolvedValue(true);
  mocks.disconnect.mockReset().mockResolvedValue(undefined);
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
    let resolveSecond: ((value: ReturnType<typeof invokeResult>) => void) | undefined;
    mocks.invoke
      .mockResolvedValueOnce(invokeResult({
        resultType: "complete",
        tools: [{ name: "one", inputSchema: { type: "object", properties: {} } }],
      }, "cursor-1"))
      .mockImplementationOnce(() => new Promise((resolve) => { resolveSecond = resolve; }));
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
    resolveSecond?.(invokeResult({ resultType: "complete", tools: [] }));
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
});
