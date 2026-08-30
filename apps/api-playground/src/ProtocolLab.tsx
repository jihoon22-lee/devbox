import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  cancelMcpHttp,
  connectMcpHttp,
  disconnectMcpHttp,
  invokeMcpHttp,
  nextMcpRequestId,
  safeMcpErrorCode,
} from "./api";
import {
  authorizeMcpHttp,
  cancelMcpStdio,
  cancelMcpOAuth,
  connectMcpStdio,
  disconnectMcpStdio,
  invokeMcpStdio,
  listMcpOAuthGrants,
  pickMcpStdioCwd,
  pickMcpStdioExecutable,
  revokeMcpOAuthGrant,
} from "./mcpApi";
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
  McpNativeSelection,
  McpOAuthGrantProjection,
  McpStdioEnvironmentBinding,
  McpStdioProfile,
  McpTransport,
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
  mcp_stdio_selection_invalid: "선택한 native executable 또는 cwd를 다시 선택하세요.",
  mcp_stdio_profile_invalid: "stdio profile의 executable, 인자, environment 또는 timeout을 확인하세요.",
  mcp_stdio_environment_invalid: "stdio environment binding을 확인하세요.",
  mcp_stdio_spawn_failed: "native executable을 시작하지 못했습니다.",
  mcp_stdio_transport_failed: "native stdio transport 요청에 실패했습니다.",
  mcp_stdio_protocol_invalid: "native stdio MCP message가 올바르지 않습니다.",
  mcp_stdio_message_too_large: "native stdio MCP message가 허용된 크기를 초과했습니다.",
  mcp_stdio_request_timeout: "native stdio MCP 요청 시간이 초과되었습니다.",
  mcp_stdio_request_cancelled: "native stdio MCP 요청을 취소했습니다.",
  mcp_stdio_connection_stale: "native stdio 연결이 닫혔거나 오래되었습니다. 다시 연결하세요.",
  mcp_stdio_cleanup_failed: "native stdio process 정리를 완료하지 못했습니다.",
  mcp_stdio_connection_limit: "열 수 있는 native stdio 연결 수를 초과했습니다.",
  mcp_stdio_request_limit: "동시에 실행할 수 있는 native stdio 요청 수를 초과했습니다.",
  mcp_oauth_required: "선택한 OAuth grant를 다시 인증하세요.",
  mcp_oauth_request_invalid: "OAuth 요청 구성을 확인하세요.",
  mcp_oauth_discovery_failed: "OAuth 보호 resource 또는 authorization server를 확인하지 못했습니다.",
  mcp_oauth_resource_mismatch: "OAuth resource binding이 MCP endpoint와 일치하지 않습니다.",
  mcp_oauth_issuer_mismatch: "OAuth issuer binding이 선택한 issuer와 일치하지 않습니다.",
  mcp_oauth_pkce_required: "OAuth server가 필요한 PKCE S256을 지원하지 않습니다.",
  mcp_oauth_client_unsupported: "OAuth public client 구성을 지원하지 않습니다.",
  mcp_oauth_callback_failed: "OAuth browser callback을 확인하지 못했습니다.",
  mcp_oauth_token_failed: "OAuth token 교환에 실패했습니다.",
  mcp_oauth_storage_failed: "OAuth grant를 안전하게 저장하거나 읽지 못했습니다.",
  mcp_oauth_reauthorization_required: "OAuth grant가 만료되어 다시 인증해야 합니다.",
  mcp_oauth_cancelled: "OAuth authorization을 취소했습니다.",
  mcp_oauth_revoke_failed: "OAuth grant를 원격에서 revoke하지 못했습니다. 원하면 로컬에서 제거할 수 있습니다.",
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

type OAuthPhase = "idle" | "authorizing" | "loading" | "revoking";
type OAuthNotice = "authorized" | "remote-revoked" | "local-only";

const emptyList = (): ListState => ({ items: [], nextCursor: null, loaded: false });
const OAUTH_NOTICE_LABELS: Record<OAuthNotice, string> = {
  authorized: "OAuth authorization이 완료되었습니다.",
  "remote-revoked": "OAuth grant를 원격과 로컬에서 제거했습니다.",
  "local-only": "원격 revoke를 확인하지 못했지만 OAuth grant를 로컬에서 제거했습니다.",
};

function cancelMcpRequest(
  transport: McpTransport,
  connectionId: string,
  requestId: string,
): Promise<boolean> {
  return transport === "stdio"
    ? cancelMcpStdio(connectionId, requestId)
    : cancelMcpHttp(connectionId, requestId);
}

function disconnectMcpConnection(
  transport: McpTransport,
  connectionId: string,
): Promise<void> {
  return transport === "stdio"
    ? disconnectMcpStdio(connectionId)
    : disconnectMcpHttp(connectionId);
}

function invokeMcpRequest(
  transport: McpTransport,
  connectionId: string,
  requestId: string,
  method: string,
  params: Record<string, unknown>,
): Promise<McpInvokeResult> {
  return transport === "stdio"
    ? invokeMcpStdio(connectionId, requestId, method, params)
    : invokeMcpHttp(connectionId, requestId, method, params);
}

function isExpectedPostCancelStale(
  transport: McpTransport,
  hadActiveRequest: boolean,
  code: string,
): boolean {
  return transport === "stdio"
    && hadActiveRequest
    && code === "mcp_stdio_connection_stale";
}

export function ProtocolLab({ environment, native }: ProtocolLabProps) {
  const [transport, setTransport] = useState<McpTransport>("http");
  const [endpoint, setEndpoint] = useState("");
  const [era, setEra] = useState<McpEraPreference>("auto");
  const [timeoutMs, setTimeoutMs] = useState(10_000);
  const [headers, setHeaders] = useState<RequestHeader[]>([]);
  const [stdioExecutable, setStdioExecutable] = useState<McpNativeSelection | null>(null);
  const [stdioCwd, setStdioCwd] = useState<McpNativeSelection | null>(null);
  const [stdioArgs, setStdioArgs] = useState<string[]>([]);
  const [stdioEnvironment, setStdioEnvironment] = useState<McpStdioEnvironmentBinding[]>([]);
  const [oauthClientId, setOAuthClientId] = useState("");
  const [oauthIssuer, setOAuthIssuer] = useState("");
  const [oauthScopes, setOAuthScopes] = useState<string[]>([]);
  const [oauthGrants, setOAuthGrants] = useState<McpOAuthGrantProjection[]>([]);
  const [selectedOAuthGrantId, setSelectedOAuthGrantId] = useState("");
  const [oauthPhase, setOAuthPhase] = useState<OAuthPhase>("idle");
  const [oauthNotice, setOAuthNotice] = useState<OAuthNotice | null>(null);
  const [oauthFallbackGrantId, setOAuthFallbackGrantId] = useState<string | null>(null);
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
  const connectionTransportRef = useRef<McpTransport | null>(null);
  const activeRequestRef = useRef<{ connectionId: string; requestId: string } | null>(null);
  const oauthRequestRef = useRef<string | null>(null);

  connectionRef.current = connection;

  const selectedOAuthGrant = useMemo(
    () => oauthGrants.find((grant) => grant.grantId === selectedOAuthGrantId) ?? null,
    [oauthGrants, selectedOAuthGrantId],
  );
  const oauthBusy = oauthPhase !== "idle";
  const authorizationHeaderConflict = Boolean(selectedOAuthGrantId)
    && headers.some((header) => header.enabled !== false
      && header.key.trim().toLowerCase() === "authorization");

  useEffect(() => () => {
    generationRef.current += 1;
    const active = activeRequestRef.current;
    const current = connectionRef.current;
    const currentTransport = connectionTransportRef.current;
    const oauthRequestId = oauthRequestRef.current;
    activeRequestRef.current = null;
    oauthRequestRef.current = null;
    connectionRef.current = null;
    connectionTransportRef.current = null;
    if (active && currentTransport) {
      void cancelMcpRequest(currentTransport, active.connectionId, active.requestId).catch(() => undefined);
    }
    if (current && currentTransport) {
      void disconnectMcpConnection(currentTransport, current.connectionId).catch(() => undefined);
    }
    if (oauthRequestId && native) {
      void cancelMcpOAuth(oauthRequestId).catch(() => undefined);
    }
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

  const resetConnection = () => {
    connectionTransportRef.current = null;
    connectionRef.current = null;
    setActiveRequest(null);
    setConnection(null);
    resetExplorer();
    setPhase("idle");
  };

  const onConnect = async () => {
    if (
      !native
      || phase !== "idle"
      || oauthBusy
      || (transport === "http" && selectedOAuthGrantId && authorizationHeaderConflict)
    ) return;
    const generation = ++generationRef.current;
    const requestedTransport = transport;
    setPhase("connecting");
    setErrorCode(null);
    resetExplorer();
    const profile: McpHttpProfile | McpStdioProfile = requestedTransport === "stdio"
      ? {
        executableSelectionId: stdioExecutable?.selectionId ?? "",
        cwdSelectionId: stdioCwd?.selectionId,
        era,
        args: stdioArgs,
        environment: stdioEnvironment,
        timeoutMs,
      }
      : {
        endpoint,
        era,
        headers,
        timeoutMs,
        ...(selectedOAuthGrantId ? { oauthGrantId: selectedOAuthGrantId } : {}),
      };
    try {
      const connected = requestedTransport === "stdio"
        ? await connectMcpStdio(profile as McpStdioProfile, environment)
        : await connectMcpHttp(profile as McpHttpProfile, environment);
      if (generation !== generationRef.current) {
        await disconnectMcpConnection(requestedTransport, connected.connectionId).catch(() => undefined);
        return;
      }
      connectionTransportRef.current = requestedTransport;
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

  const onTransportChange = async (next: McpTransport) => {
    if (
      next === transport
      || phase === "connecting"
      || phase === "disconnecting"
      || oauthBusy
    ) return;
    const current = connectionRef.current;
    if (!current) {
      setTransport(next);
      setErrorCode(native ? null : "native_required");
      return;
    }

    const currentTransport = connectionTransportRef.current ?? transport;
    const generation = ++generationRef.current;
    setPhase("disconnecting");
    const active = activeRequestRef.current;
    activeRequestRef.current = null;
    setActiveRequest(null);
    if (active) {
      await cancelMcpRequest(currentTransport, active.connectionId, active.requestId).catch(() => undefined);
    }
    let disconnectError: string | null = null;
    try {
      await disconnectMcpConnection(currentTransport, current.connectionId);
    } catch (cause) {
      const code = safeMcpErrorCode(cause);
      if (!isExpectedPostCancelStale(currentTransport, Boolean(active), code)) {
        disconnectError = code;
      }
    } finally {
      if (generation === generationRef.current) {
        connectionTransportRef.current = null;
        connectionRef.current = null;
        setConnection(null);
        resetExplorer();
        setTransport(next);
        setPhase("idle");
        setErrorCode(disconnectError ?? (native ? null : "native_required"));
      }
    }
  };

  const onDisconnect = async () => {
    const current = connectionRef.current;
    if (!current || phase === "disconnecting") return;
    const currentTransport = connectionTransportRef.current ?? transport;
    const generation = ++generationRef.current;
    setPhase("disconnecting");
    const active = activeRequestRef.current;
    activeRequestRef.current = null;
    setActiveRequest(null);
    if (active) {
      await cancelMcpRequest(currentTransport, active.connectionId, active.requestId).catch(() => undefined);
    }
    try {
      await disconnectMcpConnection(currentTransport, current.connectionId);
    } catch (cause) {
      const code = safeMcpErrorCode(cause);
      if (
        generation === generationRef.current
        && !isExpectedPostCancelStale(currentTransport, Boolean(active), code)
      ) {
        setErrorCode(code);
      }
    } finally {
      if (generation === generationRef.current) {
        connectionTransportRef.current = null;
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
    const currentTransport = connectionTransportRef.current ?? transport;
    const generation = generationRef.current;
    const requestId = nextMcpRequestId();
    const ownership = { connectionId: current.connectionId, requestId };
    activeRequestRef.current = ownership;
    setActiveRequest({ id: requestId, label });
    setErrorCode(null);
    try {
      const response = await invokeMcpRequest(
        currentTransport,
        current.connectionId,
        requestId,
        method,
        params,
      );
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
        if (
          code === "mcp_connection_stale"
          || code === "mcp_stdio_connection_stale"
          || (currentTransport === "stdio" && [
            "mcp_stdio_transport_failed",
            "mcp_stdio_protocol_invalid",
            "mcp_stdio_message_too_large",
            "mcp_stdio_request_timeout",
            "mcp_stdio_request_cancelled",
            "mcp_stdio_cleanup_failed",
          ].includes(code))
        ) {
          generationRef.current += 1;
          activeRequestRef.current = null;
          resetConnection();
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
    const currentTransport = connectionTransportRef.current ?? transport;
    try {
      await cancelMcpRequest(currentTransport, active.connectionId, active.requestId);
    } catch (cause) {
      setErrorCode(safeMcpErrorCode(cause));
    }
  };

  const onAuthorize = async () => {
    if (
      !native
      || transport !== "http"
      || phase === "connecting"
      || phase === "disconnecting"
      || oauthBusy
      || !endpoint.trim()
      || !oauthClientId.trim()
    ) return;
    const scopes = oauthScopes.map((scope) => scope.trim()).filter(Boolean);
    if (scopes.length > 32 || new Set(scopes).size !== scopes.length) {
      setErrorCode("mcp_oauth_request_invalid");
      return;
    }
    const requestId = nextMcpRequestId();
    oauthRequestRef.current = requestId;
    setOAuthPhase("authorizing");
    setOAuthNotice(null);
    setErrorCode(null);
    try {
      const grant = await authorizeMcpHttp(
        requestId,
        endpoint.trim(),
        oauthIssuer.trim() || null,
        oauthClientId.trim(),
        scopes,
      );
      if (oauthRequestRef.current !== requestId) return;
      setOAuthGrants((current) => {
        const existing = current.findIndex((item) => item.grantId === grant.grantId);
        if (existing < 0) return [...current, grant];
        const next = [...current];
        next[existing] = grant;
        return next;
      });
      setSelectedOAuthGrantId(grant.grantId);
      setOAuthFallbackGrantId(null);
      setOAuthNotice("authorized");
    } catch (cause) {
      if (oauthRequestRef.current === requestId) setErrorCode(safeMcpErrorCode(cause));
    } finally {
      if (oauthRequestRef.current === requestId) {
        oauthRequestRef.current = null;
        setOAuthPhase("idle");
      }
    }
  };

  const onCancelOAuth = async () => {
    const requestId = oauthRequestRef.current;
    if (!requestId || oauthPhase !== "authorizing") return;
    try {
      await cancelMcpOAuth(requestId);
    } catch (cause) {
      setErrorCode(safeMcpErrorCode(cause));
    }
  };

  const onRefreshOAuthGrants = async () => {
    if (
      !native
      || transport !== "http"
      || oauthBusy
      || phase === "connecting"
      || phase === "disconnecting"
      || busy
    ) return;
    setOAuthPhase("loading");
    setOAuthNotice(null);
    setErrorCode(null);
    try {
      const grants = await listMcpOAuthGrants();
      setOAuthGrants(grants);
      setSelectedOAuthGrantId((current) => (
        grants.some((grant) => grant.grantId === current)
          ? current
          : grants[0]?.grantId ?? ""
      ));
      setOAuthFallbackGrantId((current) => (
        current && grants.some((grant) => grant.grantId === current) ? current : null
      ));
    } catch (cause) {
      setErrorCode(safeMcpErrorCode(cause));
    } finally {
      setOAuthPhase("idle");
    }
  };

  const onRevokeOAuthGrant = async (removeLocalOnRemoteFailure: boolean) => {
    const grant = selectedOAuthGrant;
    if (
      !native
      || !grant
      || oauthBusy
      || phase === "connecting"
      || phase === "disconnecting"
      || busy
    ) return;
    const grantId = grant.grantId;
    setOAuthPhase("revoking");
    setOAuthNotice(null);
    setErrorCode(null);
    try {
      const revoked = await revokeMcpOAuthGrant(grantId, removeLocalOnRemoteFailure);
      if (revoked.removedLocal) {
        setOAuthGrants((current) => current.filter((item) => item.grantId !== grantId));
        setSelectedOAuthGrantId((current) => (current === grantId ? "" : current));
        setOAuthFallbackGrantId(null);
        setOAuthNotice(revoked.remoteRevoked ? "remote-revoked" : "local-only");
      }
    } catch (cause) {
      const code = safeMcpErrorCode(cause);
      setErrorCode(code);
      if (!removeLocalOnRemoteFailure && code === "mcp_oauth_revoke_failed") {
        setOAuthFallbackGrantId(grantId);
      }
    } finally {
      setOAuthPhase("idle");
    }
  };

  const onPickExecutable = async () => {
    if (!native || phase !== "idle") return;
    try {
      const selection = await pickMcpStdioExecutable();
      if (selection) setStdioExecutable(selection);
    } catch (cause) {
      setErrorCode(safeMcpErrorCode(cause));
    }
  };

  const onPickCwd = async () => {
    if (!native || phase !== "idle") return;
    try {
      const selection = await pickMcpStdioCwd();
      if (selection) setStdioCwd(selection);
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
  const normalizedOAuthScopes = oauthScopes.map((scope) => scope.trim()).filter(Boolean);
  const oauthScopesHaveDuplicates = new Set(normalizedOAuthScopes).size !== normalizedOAuthScopes.length;

  return (
    <section className="protocol-lab" aria-labelledby="protocol-lab-heading">
      <div className="protocol-lab-head">
        <div>
          <h2 id="protocol-lab-heading">Protocol Lab · MCP</h2>
          <p className="dim">
            {transport === "http"
              ? "Streamable HTTP를 modern 2026-07-28 또는 legacy 2025-11-25로 검사합니다."
              : "native executable의 stdio를 modern 2026-07-28 또는 legacy 2025-11-25로 검사합니다."}
            연결과 목록 조회만으로 tool·resource·prompt를 실행하지 않습니다.
          </p>
        </div>
        <span className="mcp-memory-badge">Protocol timeline/result · 메모리 전용</span>
      </div>

      <p className="dim mcp-storage-disclosure">
        Protocol timeline/result는 메모리에만 유지됩니다. OAuth token은 Windows DPAPI로 암호화되어
        revoke 또는 local removal 전까지 app-local grant store에 보관됩니다.
      </p>

      {!native && (
        <div className="mcp-notice" role="note">
          브라우저 미리보기에서는 MCP 네트워크 요청을 보내지 않습니다. 데스크톱 앱에서 연결하세요.
        </div>
      )}

      <div className="mcp-profile">
        <label>
          Transport
          <select
            aria-label="MCP transport"
            value={transport}
            disabled={phase === "connecting" || phase === "disconnecting" || oauthBusy}
            onChange={(event) => void onTransportChange(event.currentTarget.value as McpTransport)}
          >
            <option value="http">HTTP</option>
            <option value="stdio">stdio · native executable</option>
          </select>
        </label>
        {transport === "http" ? (
          <label>
            Endpoint
            <input
              aria-label="MCP endpoint"
              type="url"
              value={endpoint}
              maxLength={8 * 1024}
              disabled={connected || busy || oauthBusy}
              placeholder="https://server.example/mcp"
              onChange={(event) => setEndpoint(event.currentTarget.value)}
              spellCheck={false}
            />
          </label>
        ) : (
          <div className="mcp-stdio-profile">
            <div className="mcp-stdio-warning mcp-notice" role="note">
              stdio는 native executable만 실행합니다. WSL stdio와 shell command string은 지원하지 않습니다.
            </div>
            <div className="mcp-stdio-selection-row">
              <span>Executable</span>
              <button
                className="btn"
                type="button"
                aria-label="Choose executable"
                disabled={!native || connected || busy}
                onClick={() => void onPickExecutable()}
              >
                Choose executable
              </button>
              <span role="status">{stdioExecutable ? stdioExecutable.label : "선택하지 않음"}</span>
            </div>
            <div className="mcp-stdio-selection-row">
              <span>Working directory</span>
              <button
                className="btn"
                type="button"
                aria-label="Choose cwd"
                disabled={!native || connected || busy}
                onClick={() => void onPickCwd()}
              >
                Choose cwd
              </button>
              <span role="status">{stdioCwd ? stdioCwd.label : "선택하지 않음"}</span>
              {stdioCwd && (
                <button
                  className="btn"
                  type="button"
                  aria-label="Clear cwd"
                  disabled={connected || busy}
                  onClick={() => setStdioCwd(null)}
                >
                  Clear
                </button>
              )}
            </div>
            <fieldset disabled={connected || busy}>
              <legend>Arguments</legend>
              {stdioArgs.map((value, index) => (
                <div className="mcp-stdio-row" key={`arg-${index}`}>
                  <label>
                    Argument {index + 1}
                    <input
                      aria-label={`stdio argument ${index + 1}`}
                      value={value}
                      maxLength={8 * 1024}
                      onChange={(event) => {
                        const next = [...stdioArgs];
                        next[index] = event.currentTarget.value;
                        setStdioArgs(next);
                      }}
                      spellCheck={false}
                    />
                  </label>
                  <button
                    className="btn"
                    type="button"
                    aria-label={`Remove argument ${index + 1}`}
                    onClick={() => setStdioArgs(stdioArgs.filter((_, itemIndex) => itemIndex !== index))}
                  >
                    Remove
                  </button>
                </div>
              ))}
              <button
                className="btn"
                type="button"
                aria-label="Add argument"
                onClick={() => setStdioArgs((current) => [...current, ""])}
              >
                Add argument
              </button>
            </fieldset>
            <fieldset disabled={connected || busy}>
              <legend>Environment bindings</legend>
              {stdioEnvironment.map((binding, index) => (
                <div className="mcp-stdio-row" key={`environment-${index}`}>
                  <label>
                    Child name {index + 1}
                    <input
                      aria-label={`stdio child name ${index + 1}`}
                      value={binding.childName}
                      maxLength={256}
                      onChange={(event) => {
                        const next = [...stdioEnvironment];
                        next[index] = { ...binding, childName: event.currentTarget.value };
                        setStdioEnvironment(next);
                      }}
                      spellCheck={false}
                    />
                  </label>
                  <label>
                    Source name {index + 1}
                    <input
                      aria-label={`stdio source name ${index + 1}`}
                      list="mcp-stdio-environment-names"
                      value={binding.sourceName}
                      maxLength={256}
                      onChange={(event) => {
                        const next = [...stdioEnvironment];
                        next[index] = { ...binding, sourceName: event.currentTarget.value };
                        setStdioEnvironment(next);
                      }}
                      spellCheck={false}
                    />
                  </label>
                  <button
                    className="btn"
                    type="button"
                    aria-label={`Remove environment binding ${index + 1}`}
                    onClick={() => setStdioEnvironment(
                      stdioEnvironment.filter((_, itemIndex) => itemIndex !== index),
                    )}
                  >
                    Remove
                  </button>
                </div>
              ))}
              <button
                className="btn"
                type="button"
                aria-label="Add environment binding"
                onClick={() => setStdioEnvironment((current) => [
                  ...current,
                  { childName: "", sourceName: "" },
                ])}
              >
                Add environment binding
              </button>
              <datalist id="mcp-stdio-environment-names">
                {environment.map((variable) => <option key={variable.key} value={variable.key} />)}
              </datalist>
              <p className="dim">Environment 값은 native process 시작 시에만 해석되며 화면이나 timeline에 표시되지 않습니다.</p>
            </fieldset>
          </div>
        )}
        <label>
          Era
            <select
            aria-label="MCP era"
            value={era}
            disabled={connected || busy || oauthBusy}
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
            disabled={connected || busy || oauthBusy}
            onChange={(event) => setTimeoutMs(Number(event.currentTarget.value))}
          />
        </label>
        <div className="mcp-profile-actions">
          {connected ? (
            <button className="btn" type="button" disabled={busy || oauthBusy} onClick={() => void onDisconnect()}>
              연결 해제
            </button>
          ) : (
            <button
              className="btn send"
              type="button"
              disabled={!native || busy || oauthBusy || timeoutMs < 100 || timeoutMs > 120_000
                || (transport === "http"
                  ? !endpoint.trim() || Boolean(selectedOAuthGrantId && authorizationHeaderConflict)
                  : !stdioExecutable)}
              onClick={() => void onConnect()}
            >
              {phase === "connecting" ? "연결 중..." : "Connect"}
            </button>
          )}
        </div>
      </div>

      {transport === "http" && (
        <details className="mcp-custom-headers" open={headers.length > 0}>
          <summary>Custom headers · Environment secret 참조 가능</summary>
          <fieldset disabled={connected || busy || oauthBusy}>
            <HeaderTable rows={headers} secretNames={secretNames} onChange={setHeaders} />
          </fieldset>
        </details>
      )}

      {transport === "http" && (
        <section className="mcp-oauth-panel" aria-labelledby="mcp-oauth-heading">
          <h3 id="mcp-oauth-heading">HTTP OAuth 2.1</h3>
          <p className="dim">
            HTTP MCP server의 public client authorization만 지원합니다. 토큰과 callback 값은
            Protocol Lab 화면이나 timeline에 표시하지 않습니다.
          </p>
          <div className="mcp-oauth-inputs">
            <label>
              Public client ID
              <input
                aria-label="OAuth public client ID"
                value={oauthClientId}
                maxLength={8 * 1024}
                disabled={oauthBusy || busy}
                onChange={(event) => setOAuthClientId(event.currentTarget.value)}
                spellCheck={false}
              />
            </label>
            <label>
              Issuer (optional)
              <input
                aria-label="OAuth issuer (optional)"
                value={oauthIssuer}
                maxLength={8 * 1024}
                disabled={oauthBusy || busy}
                onChange={(event) => setOAuthIssuer(event.currentTarget.value)}
                spellCheck={false}
              />
            </label>
          </div>
          <fieldset disabled={oauthBusy || busy}>
            <legend>OAuth scopes</legend>
            {oauthScopes.map((scope, index) => (
              <div className="mcp-oauth-scope-row" key={`oauth-scope-${index}`}>
                <label>
                  Scope {index + 1}
                  <input
                    aria-label={`OAuth scope ${index + 1}`}
                    value={scope}
                    maxLength={256}
                    onChange={(event) => {
                      const next = [...oauthScopes];
                      next[index] = event.currentTarget.value;
                      setOAuthScopes(next);
                    }}
                    spellCheck={false}
                  />
                </label>
                <button
                  className="btn"
                  type="button"
                  aria-label={`Remove OAuth scope ${index + 1}`}
                  onClick={() => setOAuthScopes(
                    oauthScopes.filter((_, itemIndex) => itemIndex !== index),
                  )}
                >
                  Remove
                </button>
              </div>
            ))}
            <button
              className="btn"
              type="button"
              aria-label="Add OAuth scope"
              disabled={oauthScopes.length >= 32}
              onClick={() => setOAuthScopes((current) => [...current, ""])}
            >
              Add scope
            </button>
            {oauthScopesHaveDuplicates && (
              <p className="dim" role="alert">OAuth scopes must be unique.</p>
            )}
          </fieldset>
          <div className="mcp-inline-actions">
            {oauthPhase === "authorizing" ? (
              <button
                className="btn"
                type="button"
                aria-label="Cancel OAuth authorization"
                disabled={!native}
                onClick={() => void onCancelOAuth()}
              >
                Cancel authorization
              </button>
            ) : (
              <button
                className="btn send"
                type="button"
                aria-label="Authorize in system browser"
                disabled={!native || oauthBusy || busy || !endpoint.trim() || !oauthClientId.trim()
                  || oauthScopesHaveDuplicates}
                onClick={() => void onAuthorize()}
              >
                Authorize in system browser
              </button>
            )}
            <button
              className="btn"
              type="button"
              aria-label="Refresh OAuth grants"
              disabled={!native || oauthBusy || busy}
              onClick={() => void onRefreshOAuthGrants()}
            >
              Refresh grants
            </button>
          </div>
          <label>
            Stored OAuth grant
            <select
              aria-label="OAuth grant"
              value={selectedOAuthGrantId}
              disabled={oauthBusy || busy}
              onChange={(event) => {
                setSelectedOAuthGrantId(event.currentTarget.value);
                setOAuthFallbackGrantId(null);
                setOAuthNotice(null);
              }}
            >
              <option value="">No grant selected</option>
              {oauthGrants.map((grant) => (
                <option key={grant.grantId} value={grant.grantId}>
                  {boundedText(grant.clientId, 160)} · {grant.status}
                </option>
              ))}
            </select>
          </label>
          {selectedOAuthGrant && (
            <div className="mcp-oauth-grant" role="status">
              <strong>Selected OAuth grant</strong>
              <span>Issuer: {boundedText(selectedOAuthGrant.issuer, 300)}</span>
              <span>Resource: {boundedText(selectedOAuthGrant.resource, 300)}</span>
              <span>Client ID: {boundedText(selectedOAuthGrant.clientId, 300)}</span>
              <span>Status: {selectedOAuthGrant.status}</span>
              <span>
                Scopes: {selectedOAuthGrant.scopes.map((scope) => boundedText(scope, 256)).join(", ") || "none"}
              </span>
              <span>Expires: {formatOAuthExpiry(selectedOAuthGrant.expiresAtMs)}</span>
              <div className="mcp-inline-actions">
                <button
                  className="btn"
                  type="button"
                  aria-label="Revoke OAuth grant"
                  disabled={!native || oauthBusy || busy}
                  onClick={() => void onRevokeOAuthGrant(false)}
                >
                  Revoke grant
                </button>
                {oauthFallbackGrantId === selectedOAuthGrant.grantId && (
                  <button
                    className="btn"
                    type="button"
                    aria-label="Remove OAuth grant locally"
                    disabled={!native || oauthBusy || busy}
                    onClick={() => void onRevokeOAuthGrant(true)}
                  >
                    Remove locally
                  </button>
                )}
              </div>
            </div>
          )}
          {selectedOAuthGrantId && authorizationHeaderConflict && (
            <div className="mcp-notice" role="alert">
              OAuth grant와 활성화된 Authorization custom header를 함께 사용할 수 없습니다. Header를
              끄거나 OAuth grant 선택을 해제하세요.
            </div>
          )}
          {oauthNotice && (
            <p className="dim" role="status">{OAUTH_NOTICE_LABELS[oauthNotice]}</p>
          )}
        </section>
      )}

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

function formatOAuthExpiry(value: number | null): string {
  if (value === null) return "not provided";
  const formatted = new Date(value).toLocaleString();
  return formatted === "Invalid Date" ? "not provided" : formatted;
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
