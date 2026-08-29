import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  cancelMcpHttp,
  connectMcpHttp,
  disconnectMcpHttp,
  invokeMcpHttp,
  nextMcpRequestId,
  safeMcpErrorCode,
} from "./api";
import { HeaderTable } from "./HeaderTable";
import { McpSchemaEditor } from "./McpSchemaEditor";
import type { EnvVariable } from "./lib/environments";
import {
  analyzeMcpToolSchema,
  appendMcpListPage,
  hasMcpCapability,
  initialMcpArguments,
  projectMcpListPage,
  validateMcpArguments,
  type McpListKind,
} from "./lib/mcp";
import type {
  McpConnectResult,
  McpEraPreference,
  McpHttpProfile,
  McpInvokeResult,
  McpTimelineEntry,
  RequestHeader,
} from "./types";

const ERROR_LABELS: Record<string, string> = {
  native_required: "Protocol Lab 네트워크 연결은 데스크톱 앱에서만 사용할 수 있습니다.",
  mcp_invalid_profile: "endpoint, timeout 또는 custom header 구성을 확인하세요.",
  mcp_secret_unavailable: "현재 Environment의 secret을 안전하게 해제할 수 없습니다.",
  mcp_connection_limit: "열 수 있는 MCP 연결 수를 초과했습니다.",
  mcp_connect_timeout: "MCP 연결 시간이 초과되었습니다.",
  mcp_transport_failed: "MCP transport 요청에 실패했습니다.",
  mcp_redirect_blocked: "credential 보호를 위해 redirect를 차단했습니다.",
  mcp_response_type_invalid: "서버가 JSON 또는 SSE가 아닌 응답을 반환했습니다.",
  mcp_request_too_large: "MCP 요청이 허용된 크기를 초과했습니다.",
  mcp_response_too_large: "MCP 응답이 허용된 크기를 초과했습니다.",
  mcp_message_invalid: "MCP message 또는 응답 구조가 올바르지 않습니다.",
  mcp_version_unsupported: "지원하는 MCP protocol version을 협상하지 못했습니다.",
  mcp_capability_unavailable: "서버가 이 capability를 제공하지 않습니다.",
  mcp_request_limit: "동시에 실행할 수 있는 MCP 요청 수를 초과했습니다.",
  mcp_request_timeout: "MCP 요청 시간이 초과되었습니다.",
  mcp_request_cancelled: "MCP 요청을 취소했습니다.",
  mcp_cursor_invalid: "pagination cursor가 올바르지 않습니다.",
  mcp_schema_unsupported: "이 tool schema는 안전한 호출 형식으로 해석할 수 없습니다.",
  mcp_connection_stale: "연결이 닫혔거나 오래되었습니다. 다시 연결하세요.",
  mcp_server_error: "MCP 서버가 JSON-RPC 오류를 반환했습니다.",
};

interface ListState {
  items: Record<string, unknown>[];
  nextCursor: string | null;
  loaded: boolean;
}

interface ProtocolLabProps {
  environment: readonly EnvVariable[];
  native: boolean;
}

const emptyList = (): ListState => ({ items: [], nextCursor: null, loaded: false });

export function ProtocolLab({ environment, native }: ProtocolLabProps) {
  const [endpoint, setEndpoint] = useState("");
  const [era, setEra] = useState<McpEraPreference>("auto");
  const [timeoutMs, setTimeoutMs] = useState(10_000);
  const [headers, setHeaders] = useState<RequestHeader[]>([]);
  const [connection, setConnection] = useState<McpConnectResult | null>(null);
  const [phase, setPhase] = useState<"idle" | "connecting" | "connected" | "disconnecting">("idle");
  const [activeRequest, setActiveRequest] = useState<{ id: string; label: string } | null>(null);
  const [errorCode, setErrorCode] = useState<string | null>(native ? null : "native_required");
  const [timeline, setTimeline] = useState<McpTimelineEntry[]>([]);
  const [result, setResult] = useState<unknown | null>(null);
  const [tools, setTools] = useState<ListState>(emptyList);
  const [resources, setResources] = useState<ListState>(emptyList);
  const [resourceTemplates, setResourceTemplates] = useState<ListState>(emptyList);
  const [prompts, setPrompts] = useState<ListState>(emptyList);
  const [selectedToolName, setSelectedToolName] = useState("");
  const [toolArguments, setToolArguments] = useState<Record<string, unknown>>({});
  const [selectedResourceUri, setSelectedResourceUri] = useState("");
  const [selectedPromptName, setSelectedPromptName] = useState("");
  const [promptArguments, setPromptArguments] = useState<Record<string, string>>({});
  const generationRef = useRef(0);
  const connectionRef = useRef<McpConnectResult | null>(null);
  const activeRequestRef = useRef<{ connectionId: string; requestId: string } | null>(null);

  connectionRef.current = connection;

  useEffect(() => () => {
    generationRef.current += 1;
    const active = activeRequestRef.current;
    const current = connectionRef.current;
    activeRequestRef.current = null;
    connectionRef.current = null;
    if (active) void cancelMcpHttp(active.connectionId, active.requestId).catch(() => undefined);
    if (current) void disconnectMcpHttp(current.connectionId).catch(() => undefined);
  }, []);

  const selectedTool = useMemo(() => tools.items.find(
    (item) => item.name === selectedToolName,
  ) ?? null, [selectedToolName, tools.items]);
  const toolSchema = selectedTool?.inputSchema;
  const schemaAnalysis = useMemo(
    () => analyzeMcpToolSchema(toolSchema),
    [toolSchema],
  );
  const argumentIssues = useMemo(
    () => schemaAnalysis.mode === "form" && schemaAnalysis.schema
      ? validateMcpArguments(schemaAnalysis.schema, toolArguments)
      : [schemaAnalysis.reason ?? "지원하지 않는 schema입니다."],
    [schemaAnalysis, toolArguments],
  );
  const selectedPrompt = useMemo(() => prompts.items.find(
    (item) => item.name === selectedPromptName,
  ) ?? null, [prompts.items, selectedPromptName]);
  const promptFields = Array.isArray(selectedPrompt?.arguments)
    ? selectedPrompt.arguments.filter(isPromptArgument)
    : [];

  useEffect(() => {
    if (!selectedTool || schemaAnalysis.mode !== "form" || !schemaAnalysis.schema) {
      setToolArguments({});
      return;
    }
    setToolArguments(initialMcpArguments(schemaAnalysis.schema));
  }, [schemaAnalysis, selectedTool]);

  useEffect(() => {
    const next: Record<string, string> = {};
    for (const field of promptFields) next[field.name] = "";
    setPromptArguments(next);
  }, [selectedPromptName]);

  const resetExplorer = () => {
    setTools(emptyList());
    setResources(emptyList());
    setResourceTemplates(emptyList());
    setPrompts(emptyList());
    setSelectedToolName("");
    setToolArguments({});
    setSelectedResourceUri("");
    setSelectedPromptName("");
    setPromptArguments({});
    setResult(null);
    setTimeline([]);
  };

  const onConnect = async () => {
    if (!native || phase !== "idle") return;
    const generation = ++generationRef.current;
    setPhase("connecting");
    setErrorCode(null);
    resetExplorer();
    const profile: McpHttpProfile = { endpoint, era, headers, timeoutMs };
    try {
      const connected = await connectMcpHttp(profile, environment);
      if (generation !== generationRef.current) {
        await disconnectMcpHttp(connected.connectionId).catch(() => undefined);
        return;
      }
      connectionRef.current = connected;
      setConnection(connected);
      setTimeline(connected.timeline);
      setPhase("connected");
    } catch (cause) {
      if (generation === generationRef.current) {
        setErrorCode(safeMcpErrorCode(cause));
        setPhase("idle");
      }
    }
  };

  const onDisconnect = async () => {
    const current = connectionRef.current;
    if (!current || phase === "disconnecting") return;
    const generation = ++generationRef.current;
    setPhase("disconnecting");
    const active = activeRequestRef.current;
    activeRequestRef.current = null;
    setActiveRequest(null);
    if (active) await cancelMcpHttp(active.connectionId, active.requestId).catch(() => undefined);
    try {
      await disconnectMcpHttp(current.connectionId);
    } catch (cause) {
      if (generation === generationRef.current) setErrorCode(safeMcpErrorCode(cause));
    } finally {
      if (generation === generationRef.current) {
        connectionRef.current = null;
        setConnection(null);
        resetExplorer();
        setPhase("idle");
      }
    }
  };

  const invoke = async (
    method: string,
    params: Record<string, unknown>,
    label: string,
  ): Promise<McpInvokeResult | null> => {
    const current = connectionRef.current;
    if (!current || activeRequestRef.current) return null;
    const generation = generationRef.current;
    const requestId = nextMcpRequestId();
    const ownership = { connectionId: current.connectionId, requestId };
    activeRequestRef.current = ownership;
    setActiveRequest({ id: requestId, label });
    setErrorCode(null);
    try {
      const response = await invokeMcpHttp(current.connectionId, requestId, method, params);
      if (generation !== generationRef.current || connectionRef.current?.connectionId !== current.connectionId) {
        return null;
      }
      setTimeline(response.timeline);
      setResult(response.result);
      if (response.errorCode) setErrorCode(response.errorCode);
      return response;
    } catch (cause) {
      if (generation === generationRef.current) {
        const code = safeMcpErrorCode(cause);
        if (code === "mcp_connection_stale") {
          generationRef.current += 1;
          activeRequestRef.current = null;
          connectionRef.current = null;
          setActiveRequest(null);
          setConnection(null);
          resetExplorer();
          setPhase("idle");
        }
        setErrorCode(code);
      }
      return null;
    } finally {
      if (activeRequestRef.current?.requestId === requestId) {
        activeRequestRef.current = null;
        if (generation === generationRef.current) setActiveRequest(null);
      }
    }
  };

  const onCancel = async () => {
    const active = activeRequestRef.current;
    if (!active) return;
    try {
      await cancelMcpHttp(active.connectionId, active.requestId);
    } catch (cause) {
      setErrorCode(safeMcpErrorCode(cause));
    }
  };

  const loadList = async (
    kind: McpListKind,
    method: string,
    state: ListState,
    update: (state: ListState) => void,
  ) => {
    const params = state.loaded && state.nextCursor ? { cursor: state.nextCursor } : {};
    const response = await invoke(method, params, method);
    if (!response?.result) return;
    try {
      const page = projectMcpListPage(response.result, kind);
      const items = appendMcpListPage(state.items, page, kind);
      update({ items, nextCursor: response.nextCursor, loaded: true });
      if (kind === "tools" && !selectedToolName && typeof items[0]?.name === "string") {
        setSelectedToolName(items[0].name);
      }
      if (kind === "resources" && !selectedResourceUri && typeof items[0]?.uri === "string") {
        setSelectedResourceUri(items[0].uri);
      }
      if (kind === "prompts" && !selectedPromptName && typeof items[0]?.name === "string") {
        setSelectedPromptName(items[0].name);
      }
    } catch (cause) {
      setErrorCode(safeMcpErrorCode(cause));
    }
  };

  const connected = phase === "connected" && connection !== null;
  const busy = activeRequest !== null || phase === "connecting" || phase === "disconnecting";
  const capabilities = connection?.server.capabilities ?? {};
  const secretNames = environment.filter((item) => item.secret).map((item) => item.key);

  return (
    <section className="protocol-lab" aria-labelledby="protocol-lab-heading">
      <div className="protocol-lab-head">
        <div>
          <h2 id="protocol-lab-heading">Protocol Lab · MCP</h2>
          <p className="dim">
            Streamable HTTP를 modern 2026-07-28 또는 legacy 2025-11-25로 검사합니다.
            연결과 목록 조회만으로 tool·resource·prompt를 실행하지 않습니다.
          </p>
        </div>
        <span className="mcp-memory-badge">메모리 전용 · 저장 안 함</span>
      </div>

      {!native && (
        <div className="mcp-notice" role="note">
          브라우저 미리보기에서는 MCP 네트워크 요청을 보내지 않습니다. 데스크톱 앱에서 연결하세요.
        </div>
      )}

      <div className="mcp-profile">
        <label>
          Endpoint
          <input
            aria-label="MCP endpoint"
            type="url"
            value={endpoint}
            maxLength={8 * 1024}
            disabled={connected || busy}
            placeholder="https://server.example/mcp"
            onChange={(event) => setEndpoint(event.currentTarget.value)}
            spellCheck={false}
          />
        </label>
        <label>
          Era
          <select
            aria-label="MCP era"
            value={era}
            disabled={connected || busy}
            onChange={(event) => setEra(event.currentTarget.value as McpEraPreference)}
          >
            <option value="auto">auto · modern 우선</option>
            <option value="modern">modern · 2026-07-28</option>
            <option value="legacy">legacy · 2025-11-25</option>
          </select>
        </label>
        <label>
          Timeout (ms)
          <input
            aria-label="MCP timeout"
            type="number"
            min={100}
            max={120_000}
            value={timeoutMs}
            disabled={connected || busy}
            onChange={(event) => setTimeoutMs(Number(event.currentTarget.value))}
          />
        </label>
        <div className="mcp-profile-actions">
          {connected ? (
            <button className="btn" type="button" disabled={busy} onClick={() => void onDisconnect()}>
              연결 해제
            </button>
          ) : (
            <button
              className="btn send"
              type="button"
              disabled={!native || busy || !endpoint.trim() || timeoutMs < 100 || timeoutMs > 120_000}
              onClick={() => void onConnect()}
            >
              {phase === "connecting" ? "연결 중..." : "Connect"}
            </button>
          )}
        </div>
      </div>

      <details className="mcp-custom-headers" open={headers.length > 0}>
        <summary>Custom headers · Environment secret 참조 가능</summary>
        <fieldset disabled={connected || busy}>
          <HeaderTable rows={headers} secretNames={secretNames} onChange={setHeaders} />
        </fieldset>
      </details>

      {connection && (
        <div className="mcp-server-card" role="status">
          <strong>{boundedText(connection.server.serverName, 200) || "이름 없는 MCP server"}</strong>
          <span>{boundedText(connection.server.serverVersion, 100) || "version 미제공"}</span>
          <code>{connection.server.era} · {connection.server.protocolVersion}</code>
          <span>legacy session: {connection.sessionManaged ? "사용" : "미사용"}</span>
          <span>tools {hasMcpCapability(capabilities, "tools") ? "✓" : "—"}</span>
          <span>resources {hasMcpCapability(capabilities, "resources") ? "✓" : "—"}</span>
          <span>prompts {hasMcpCapability(capabilities, "prompts") ? "✓" : "—"}</span>
        </div>
      )}

      {connected && (
        <div className="mcp-explorer-grid">
          <ExplorerSection title="Tools" enabled={hasMcpCapability(capabilities, "tools")}>
            <ListButton
              state={tools}
              busy={busy}
              onClick={() => void loadList("tools", "tools/list", tools, setTools)}
            />
            {tools.items.length > 0 && (
              <select
                aria-label="MCP tool"
                value={selectedToolName}
                disabled={busy}
                onChange={(event) => setSelectedToolName(event.currentTarget.value)}
              >
                {tools.items.map((tool) => (
                  <option key={String(tool.name)} value={String(tool.name)}>{String(tool.name)}</option>
                ))}
              </select>
            )}
            {selectedTool && (
              <>
                {typeof selectedTool.description === "string" && (
                  <p className="dim mcp-untrusted-text">{boundedText(selectedTool.description, 2_000)}</p>
                )}
                {schemaAnalysis.mode === "form" && schemaAnalysis.schema ? (
                  <McpSchemaEditor
                    schema={schemaAnalysis.schema}
                    value={toolArguments}
                    disabled={busy}
                    onChange={setToolArguments}
                  />
                ) : (
                  <div className="mcp-schema-fallback">
                    <p className="dim">{schemaAnalysis.reason}</p>
                    <pre>{boundedJson(toolSchema, 16_000)}</pre>
                  </div>
                )}
                {argumentIssues.length > 0 && schemaAnalysis.mode === "form" && (
                  <ul className="mcp-validation-list">
                    {argumentIssues.slice(0, 5).map((issue) => <li key={issue}>{issue}</li>)}
                  </ul>
                )}
                <button
                  className="btn send"
                  type="button"
                  disabled={busy || schemaAnalysis.mode !== "form" || argumentIssues.length > 0}
                  onClick={() => void invoke(
                    "tools/call",
                    { name: selectedToolName, arguments: toolArguments },
                    `tools/call · ${selectedToolName}`,
                  )}
                >
                  선택 tool 호출
                </button>
              </>
            )}
          </ExplorerSection>

          <ExplorerSection title="Resources" enabled={hasMcpCapability(capabilities, "resources")}>
            <div className="mcp-inline-actions">
              <ListButton
                state={resources}
                busy={busy}
                label="Resource"
                onClick={() => void loadList("resources", "resources/list", resources, setResources)}
              />
              <ListButton
                state={resourceTemplates}
                busy={busy}
                label="Template"
                onClick={() => void loadList(
                  "resourceTemplates",
                  "resources/templates/list",
                  resourceTemplates,
                  setResourceTemplates,
                )}
              />
            </div>
            {resources.items.length > 0 && (
              <select
                aria-label="MCP resource"
                value={selectedResourceUri}
                disabled={busy}
                onChange={(event) => setSelectedResourceUri(event.currentTarget.value)}
              >
                {resources.items.map((resource) => (
                  <option key={String(resource.uri)} value={String(resource.uri)}>
                    {boundedText(typeof resource.name === "string" ? resource.name : String(resource.uri), 160)}
                  </option>
                ))}
              </select>
            )}
            <input
              aria-label="MCP resource URI"
              value={selectedResourceUri}
              maxLength={8 * 1024}
              disabled={busy}
              placeholder="Resource URI"
              onChange={(event) => setSelectedResourceUri(event.currentTarget.value)}
              spellCheck={false}
            />
            <button
              className="btn send"
              type="button"
              disabled={busy || !selectedResourceUri}
              onClick={() => void invoke(
                "resources/read",
                { uri: selectedResourceUri },
                "resources/read",
              )}
            >
              Resource 읽기
            </button>
            {resourceTemplates.items.length > 0 && (
              <details>
                <summary>Resource templates {resourceTemplates.items.length}개</summary>
                <ul className="mcp-identity-list">
                  {resourceTemplates.items.map((item) => (
                    <li key={String(item.uriTemplate)}><code>{boundedText(String(item.uriTemplate), 300)}</code></li>
                  ))}
                </ul>
              </details>
            )}
          </ExplorerSection>

          <ExplorerSection title="Prompts" enabled={hasMcpCapability(capabilities, "prompts")}>
            <ListButton
              state={prompts}
              busy={busy}
              onClick={() => void loadList("prompts", "prompts/list", prompts, setPrompts)}
            />
            {prompts.items.length > 0 && (
              <select
                aria-label="MCP prompt"
                value={selectedPromptName}
                disabled={busy}
                onChange={(event) => setSelectedPromptName(event.currentTarget.value)}
              >
                {prompts.items.map((prompt) => (
                  <option key={String(prompt.name)} value={String(prompt.name)}>{String(prompt.name)}</option>
                ))}
              </select>
            )}
            {promptFields.map((field) => (
              <label key={field.name}>
                {field.name}{field.required ? " · 필수" : ""}
                <input
                  value={promptArguments[field.name] ?? ""}
                  disabled={busy}
                  maxLength={256 * 1024}
                  onChange={(event) => {
                    const value = event.currentTarget.value;
                    setPromptArguments((current) => ({
                      ...current,
                      [field.name]: value,
                    }));
                  }}
                />
              </label>
            ))}
            <button
              className="btn send"
              type="button"
              disabled={busy || !selectedPromptName || promptFields.some(
                (field) => field.required && !(field.name in promptArguments),
              )}
              onClick={() => void invoke(
                "prompts/get",
                { name: selectedPromptName, arguments: promptArguments },
                `prompts/get · ${selectedPromptName}`,
              )}
            >
              Prompt 가져오기
            </button>
          </ExplorerSection>
        </div>
      )}

      {activeRequest && (
        <div className="mcp-active-request" role="status">
          <span>{activeRequest.label} 실행 중</span>
          <button className="btn" type="button" onClick={() => void onCancel()}>취소</button>
        </div>
      )}

      {errorCode && (
        <div className="error mcp-error" role="alert">
          {ERROR_LABELS[errorCode] ?? ERROR_LABELS.mcp_transport_failed} <code>{errorCode}</code>
        </div>
      )}

      {result !== null && (
        <section className="mcp-result" aria-labelledby="mcp-result-heading">
          <h3 id="mcp-result-heading">최근 결과</h3>
          <pre>{boundedJson(result, 200_000)}</pre>
        </section>
      )}

      {timeline.length > 0 && (
        <section className="mcp-timeline" aria-labelledby="mcp-timeline-heading">
          <h3 id="mcp-timeline-heading">최근 작업 timeline</h3>
          <ol>
            {timeline.map((entry) => (
              <li key={entry.sequence}>
                <div>
                  <span>+{entry.offsetMs}ms</span>
                  <strong>{entry.direction} · {entry.kind}</strong>
                  {entry.method && <code>{entry.method}</code>}
                  {entry.requestId && <code>{entry.requestId}</code>}
                </div>
                {entry.payload !== null && <pre>{boundedJson(entry.payload, 8_000)}</pre>}
              </li>
            ))}
          </ol>
        </section>
      )}
    </section>
  );
}

function ExplorerSection({
  title,
  enabled,
  children,
}: {
  title: string;
  enabled: boolean;
  children: ReactNode;
}) {
  return (
    <section className={`mcp-explorer-section ${enabled ? "" : "disabled"}`} aria-disabled={!enabled}>
      <h3>{title}</h3>
      {enabled ? children : <p className="dim">서버가 이 capability를 제공하지 않습니다.</p>}
    </section>
  );
}

function ListButton({
  state,
  busy,
  label = "목록",
  onClick,
}: {
  state: ListState;
  busy: boolean;
  label?: string;
  onClick: () => void;
}) {
  const exhausted = state.loaded && state.nextCursor === null;
  return (
    <button className="btn" type="button" disabled={busy || exhausted} onClick={onClick}>
      {!state.loaded ? `${label} 조회` : state.nextCursor ? `${label} 다음 페이지` : `${label} 완료`}
    </button>
  );
}

function isPromptArgument(value: unknown): value is { name: string; required?: boolean } {
  return Boolean(value)
    && typeof value === "object"
    && typeof (value as { name?: unknown }).name === "string"
    && ((value as { required?: unknown }).required === undefined
      || typeof (value as { required?: unknown }).required === "boolean");
}

function boundedText(value: string, max: number): string {
  return value.length <= max ? value : `${value.slice(0, max)}…`;
}

function boundedJson(value: unknown, max: number): string {
  try {
    const serialized = JSON.stringify(value, null, 2);
    return serialized.length <= max
      ? serialized
      : `${serialized.slice(0, max)}\n… UI preview truncated …`;
  } catch {
    return "[표시할 수 없는 JSON]";
  }
}
