import { useEffect, useMemo, useRef, useState } from "react";
import {
  cancelGrpc,
  connectGrpc,
  deleteGrpcTlsCredential,
  disconnectGrpc,
  exportGrpcSummary,
  importGrpcTlsCredential,
  invokeGrpc,
  listGrpcTlsCredentials,
  nextGrpcRequestId,
  pickGrpcCa,
  pickGrpcClientCertificate,
  pickGrpcClientKey,
  pickGrpcImportRoot,
  pickGrpcProto,
  safeGrpcErrorCode,
  type GrpcConnectResult,
  type GrpcCredentialProjection,
  type GrpcExchangeSummary,
  type GrpcInvokeResult,
  type GrpcMethodProjection,
  type GrpcNativeSelection,
  type GrpcRootMode,
} from "./grpcApi";
import {
  appendGrpcHistory,
  clearGrpcHistory,
  loadGrpcHistory,
  saveGrpcHistory,
  splitGrpcRequestMessages,
  type GrpcHistoryStore,
} from "./lib/grpc";

const ERROR_LABELS: Record<string, string> = {
  grpc_native_required: "gRPC 연결과 native 파일 선택은 데스크톱 앱에서만 사용할 수 있습니다.",
  grpc_invalid_profile: "endpoint, timeout 또는 TLS 구성을 확인하세요.",
  grpc_source_selection_invalid: "선택한 proto/import root가 만료되었거나 변경되었습니다. 다시 선택하세요.",
  grpc_source_invalid: "선택한 proto와 import를 안전하게 컴파일하지 못했습니다.",
  grpc_source_too_large: "proto source의 파일 수 또는 크기 제한을 초과했습니다.",
  grpc_descriptor_invalid: "gRPC descriptor가 올바르지 않거나 허용 범위를 초과했습니다.",
  grpc_reflection_unavailable: "서버 reflection을 사용할 수 없습니다. 서버 설정 또는 local proto를 확인하세요.",
  grpc_connection_limit: "열 수 있는 gRPC 연결 수를 초과했습니다.",
  grpc_connect_timeout: "gRPC 연결 또는 reflection 시간이 초과되었습니다.",
  grpc_tls_failed: "TLS 인증서, server name, native root 또는 연결 상태를 확인하세요.",
  grpc_credential_storage_unavailable: "TLS credential 저장은 패키징된 Windows 앱에서만 사용할 수 있습니다.",
  grpc_credential_storage_failed: "TLS credential을 안전하게 저장하거나 읽지 못했습니다.",
  grpc_credential_invalid: "CA와 client certificate/private key 구성을 확인하세요.",
  grpc_connection_stale: "gRPC 연결이 닫혔거나 오래되었습니다. 다시 연결하세요.",
  grpc_method_unavailable: "연결된 descriptor에 선택한 method가 없습니다.",
  grpc_request_invalid: "ProtoJSON 입력 형식 또는 message 수를 확인하세요.",
  grpc_request_too_large: "gRPC 요청이 허용된 크기를 초과했습니다.",
  grpc_request_limit: "이 연결에서 동시에 실행할 수 있는 요청 수를 초과했습니다.",
  grpc_request_timeout: "gRPC deadline이 초과되었습니다. 요청은 재시도되지 않았습니다.",
  grpc_request_cancelled: "gRPC 요청을 취소했습니다. 요청은 재시도되지 않았습니다.",
  grpc_response_too_large: "gRPC 응답이 허용된 message 수 또는 크기를 초과했습니다.",
  grpc_protocol_failed: "gRPC transport 또는 응답을 안전하게 처리하지 못했습니다.",
  grpc_export_failed: "gRPC summary를 안전하게 저장하지 못했습니다.",
  grpc_history_failed: "gRPC summary history를 브라우저 저장소에 기록하지 못했습니다.",
};

type SourceMode = "local-proto" | "reflection";
type ConnectionPhase = "idle" | "connecting" | "connected" | "disconnecting";

interface GrpcLabProps {
  native: boolean;
}

export function GrpcLab({ native }: GrpcLabProps) {
  const [sourceMode, setSourceMode] = useState<SourceMode>("local-proto");
  const [protoSelection, setProtoSelection] = useState<GrpcNativeSelection | null>(null);
  const [importRootSelection, setImportRootSelection] = useState<GrpcNativeSelection | null>(null);
  const [endpoint, setEndpoint] = useState("http://127.0.0.1:50051");
  const [rootMode, setRootMode] = useState<GrpcRootMode>("native");
  const [serverName, setServerName] = useState("");
  const [connectTimeoutMs, setConnectTimeoutMs] = useState(10_000);
  const [rpcTimeoutMs, setRpcTimeoutMs] = useState(30_000);
  const [credentials, setCredentials] = useState<GrpcCredentialProjection[]>([]);
  const [credentialId, setCredentialId] = useState("");
  const [credentialLabel, setCredentialLabel] = useState("");
  const [caSelection, setCaSelection] = useState<GrpcNativeSelection | null>(null);
  const [certificateSelection, setCertificateSelection] = useState<GrpcNativeSelection | null>(null);
  const [keySelection, setKeySelection] = useState<GrpcNativeSelection | null>(null);
  const [credentialBusy, setCredentialBusy] = useState(false);
  const [connection, setConnection] = useState<GrpcConnectResult | null>(null);
  const [phase, setPhase] = useState<ConnectionPhase>("idle");
  const [methodQuery, setMethodQuery] = useState("");
  const [methodName, setMethodName] = useState("");
  const [requestText, setRequestText] = useState("{}");
  const [activeRequest, setActiveRequest] = useState<{ connectionId: string; requestId: string } | null>(null);
  const [result, setResult] = useState<GrpcInvokeResult | null>(null);
  const [errorCode, setErrorCode] = useState<string | null>(native ? null : "grpc_native_required");
  const [notice, setNotice] = useState<string | null>(null);
  const [history, setHistory] = useState<GrpcHistoryStore>(() => {
    try {
      return loadGrpcHistory();
    } catch {
      return { schema: "devbox.api-playground.grpc-history/v1", entries: [] };
    }
  });
  const generationRef = useRef(0);
  const connectionRef = useRef<GrpcConnectResult | null>(null);
  const activeRef = useRef<{ connectionId: string; requestId: string } | null>(null);
  const historyRef = useRef(history);

  connectionRef.current = connection;
  activeRef.current = activeRequest;
  historyRef.current = history;

  const selectedCredential = useMemo(
    () => credentials.find((credential) => credential.credentialId === credentialId) ?? null,
    [credentialId, credentials],
  );
  const selectedMethod = useMemo(
    () => connection?.methods.find((method) => method.fullName === methodName) ?? null,
    [connection, methodName],
  );
  const filteredMethods = useMemo(() => {
    const query = methodQuery.trim().toLowerCase();
    if (!connection || !query) return connection?.methods ?? [];
    return connection.methods.filter((method) => [
      method.fullName,
      method.inputType,
      method.outputType,
      method.rpcKind,
    ].some((value) => value.toLowerCase().includes(query)));
  }, [connection, methodQuery]);
  const inputIssue = useMemo(() => {
    if (!selectedMethod) return "method를 선택하세요.";
    try {
      splitGrpcRequestMessages(requestText, selectedMethod.rpcKind);
      return null;
    } catch (cause) {
      return ERROR_LABELS[localErrorCode(cause)] ?? ERROR_LABELS.grpc_request_invalid;
    }
  }, [requestText, selectedMethod]);
  const https = endpoint.trim().toLowerCase().startsWith("https://");
  const profileIssue = useMemo(() => {
    if (!endpoint.trim()) return "endpoint를 입력하세요.";
    if (sourceMode === "local-proto" && !protoSelection) return "root proto를 선택하세요.";
    if (connectTimeoutMs < 100 || connectTimeoutMs > 30_000) return "connect timeout은 100–30000ms입니다.";
    if (rpcTimeoutMs < 100 || rpcTimeoutMs > 300_000) return "RPC deadline은 100–300000ms입니다.";
    if (https && (rootMode === "custom" || rootMode === "native+custom")
      && !selectedCredential?.hasCustomCa) {
      return "custom root mode에는 CA가 포함된 credential이 필요합니다.";
    }
    if (https && rootMode === "native" && selectedCredential && !selectedCredential.hasClientIdentity) {
      return "native root mode에서 credential을 선택하려면 client identity가 필요합니다.";
    }
    return null;
  }, [connectTimeoutMs, credentialId, endpoint, https, protoSelection, rootMode, rpcTimeoutMs, selectedCredential, serverName, sourceMode]);

  useEffect(() => {
    if (!connection) return;
    if (filteredMethods.some((method) => method.fullName === methodName)) return;
    setMethodName(filteredMethods[0]?.fullName ?? "");
  }, [connection, filteredMethods, methodName]);

  useEffect(() => {
    if (!selectedMethod) return;
    const template = selectedMethod.rpcKind === "client-streaming"
      || selectedMethod.rpcKind === "bidirectional-streaming"
      ? [selectedMethod.inputTemplate]
      : selectedMethod.inputTemplate;
    setRequestText(JSON.stringify(template, null, 2));
    setResult(null);
  }, [selectedMethod]);

  useEffect(() => {
    if (!native) return undefined;
    let active = true;
    void listGrpcTlsCredentials()
      .then((values) => {
        if (active) setCredentials(values);
      })
      .catch((cause) => {
        if (active) setErrorCode(localErrorCode(cause));
      });
    return () => {
      active = false;
    };
  }, [native]);

  useEffect(() => () => {
    generationRef.current += 1;
    const active = activeRef.current;
    const current = connectionRef.current;
    activeRef.current = null;
    connectionRef.current = null;
    if (active) void cancelGrpc(active.connectionId, active.requestId).catch(() => undefined);
    if (current) void disconnectGrpc(current.connectionId).catch(() => undefined);
  }, []);

  const pickSource = async (kind: "proto" | "import-root") => {
    if (!native || phase !== "idle") return;
    try {
      const selection = kind === "proto" ? await pickGrpcProto() : await pickGrpcImportRoot();
      if (!selection) return;
      if (kind === "proto") setProtoSelection(selection);
      else setImportRootSelection(selection);
      setErrorCode(null);
    } catch (cause) {
      setErrorCode(localErrorCode(cause));
    }
  };

  const pickCredentialFile = async (kind: "ca" | "certificate" | "key") => {
    if (!native || credentialBusy) return;
    try {
      const selection = kind === "ca"
        ? await pickGrpcCa()
        : kind === "certificate"
          ? await pickGrpcClientCertificate()
          : await pickGrpcClientKey();
      if (!selection) return;
      if (kind === "ca") setCaSelection(selection);
      else if (kind === "certificate") setCertificateSelection(selection);
      else setKeySelection(selection);
      setErrorCode(null);
    } catch (cause) {
      setErrorCode(localErrorCode(cause));
    }
  };

  const refreshCredentials = async () => {
    if (!native || credentialBusy) return;
    setCredentialBusy(true);
    try {
      const values = await listGrpcTlsCredentials();
      setCredentials(values);
      if (credentialId && !values.some((credential) => credential.credentialId === credentialId)) {
        setCredentialId("");
      }
      setErrorCode(null);
    } catch (cause) {
      setErrorCode(localErrorCode(cause));
    } finally {
      setCredentialBusy(false);
    }
  };

  const importCredential = async () => {
    if (!native || credentialBusy || !credentialLabel.trim()) return;
    setCredentialBusy(true);
    try {
      const imported = await importGrpcTlsCredential({
        label: credentialLabel.trim(),
        ...(caSelection ? { caSelectionId: caSelection.selectionId } : {}),
        ...(certificateSelection ? { clientCertificateSelectionId: certificateSelection.selectionId } : {}),
        ...(keySelection ? { clientKeySelectionId: keySelection.selectionId } : {}),
      });
      const values = [imported, ...credentials.filter((value) => value.credentialId !== imported.credentialId)];
      setCredentials(values);
      setCredentialId(imported.credentialId);
      setCredentialLabel("");
      setCaSelection(null);
      setCertificateSelection(null);
      setKeySelection(null);
      setNotice("TLS credential을 DPAPI 저장소에 추가했습니다.");
      setErrorCode(null);
    } catch (cause) {
      setErrorCode(localErrorCode(cause));
    } finally {
      setCredentialBusy(false);
    }
  };

  const removeCredential = async (id: string) => {
    if (!native || credentialBusy) return;
    setCredentialBusy(true);
    try {
      const removed = await deleteGrpcTlsCredential(id);
      if (removed) {
        setCredentials((values) => values.filter((value) => value.credentialId !== id));
        if (credentialId === id) setCredentialId("");
        setNotice(connection
          ? "저장 credential을 삭제했습니다. 이미 연결된 channel은 연결 해제 전까지 기존 TLS material을 사용합니다."
          : "저장 credential을 삭제했습니다.");
      }
      setErrorCode(null);
    } catch (cause) {
      setErrorCode(localErrorCode(cause));
    } finally {
      setCredentialBusy(false);
    }
  };

  const onConnect = async () => {
    if (!native || phase !== "idle" || profileIssue) return;
    const generation = ++generationRef.current;
    setPhase("connecting");
    setErrorCode(null);
    setNotice(null);
    setResult(null);
    try {
      const connected = await connectGrpc({
        endpoint: endpoint.trim(),
        source: sourceMode === "reflection"
          ? { kind: "reflection" }
          : {
              kind: "local-proto",
              protoSelectionId: protoSelection?.selectionId ?? "",
              ...(importRootSelection
                ? { importRootSelectionId: importRootSelection.selectionId }
                : {}),
            },
        tls: https
          ? {
              rootMode,
              ...(serverName.trim() ? { serverName: serverName.trim() } : {}),
              ...(credentialId ? { credentialId } : {}),
            }
          : { rootMode: "native" },
        connectTimeoutMs,
        rpcTimeoutMs,
      });
      if (generation !== generationRef.current) {
        await disconnectGrpc(connected.connectionId).catch(() => undefined);
        return;
      }
      connectionRef.current = connected;
      setConnection(connected);
      setMethodName(connected.methods[0]?.fullName ?? "");
      setProtoSelection(null);
      setImportRootSelection(null);
      setPhase("connected");
    } catch (cause) {
      if (generation === generationRef.current) {
        setErrorCode(localErrorCode(cause));
        setPhase("idle");
      }
    }
  };

  const onDisconnect = async () => {
    const current = connectionRef.current;
    if (!current || phase === "disconnecting") return;
    const generation = ++generationRef.current;
    setPhase("disconnecting");
    const active = activeRef.current;
    activeRef.current = null;
    setActiveRequest(null);
    if (active) await cancelGrpc(active.connectionId, active.requestId).catch(() => undefined);
    try {
      await disconnectGrpc(current.connectionId);
    } catch (cause) {
      if (generation === generationRef.current) setErrorCode(localErrorCode(cause));
    } finally {
      if (generation === generationRef.current) resetConnection();
    }
  };

  const resetConnection = () => {
    connectionRef.current = null;
    setConnection(null);
    setMethodName("");
    setMethodQuery("");
    setResult(null);
    setPhase("idle");
  };

  const rememberSummary = (summary: GrpcExchangeSummary) => {
    try {
      const saved = saveGrpcHistory(appendGrpcHistory(historyRef.current, summary));
      historyRef.current = saved;
      setHistory(saved);
    } catch {
      setErrorCode("grpc_history_failed");
    }
  };

  const onInvoke = async () => {
    const current = connectionRef.current;
    if (!current || !selectedMethod || activeRef.current || inputIssue) return;
    let messages: string[];
    try {
      messages = splitGrpcRequestMessages(requestText, selectedMethod.rpcKind);
    } catch (cause) {
      setErrorCode(localErrorCode(cause));
      return;
    }
    const generation = generationRef.current;
    const requestId = nextGrpcRequestId();
    const ownership = { connectionId: current.connectionId, requestId };
    const localStartedAt = Date.now();
    const monotonicStarted = performance.now();
    activeRef.current = ownership;
    setActiveRequest(ownership);
    setResult(null);
    setErrorCode(null);
    setNotice(null);
    try {
      const response = await invokeGrpc(current.connectionId, requestId, selectedMethod.fullName, messages);
      if (generation !== generationRef.current || connectionRef.current?.connectionId !== current.connectionId) return;
      setResult(response);
      rememberSummary(toSummary(current, selectedMethod, response));
    } catch (cause) {
      if (generation !== generationRef.current) return;
      const code = localErrorCode(cause);
      setErrorCode(code);
      if (code === "grpc_request_timeout" || code === "grpc_request_cancelled") {
        rememberSummary({
          sourceKind: current.source.kind,
          service: selectedMethod.service,
          method: selectedMethod.method,
          rpcKind: selectedMethod.rpcKind,
          requestMessageCount: messages.length,
          responseMessageCount: 0,
          startedAtMs: localStartedAt,
          elapsedMs: Math.max(0, Math.round(performance.now() - monotonicStarted)),
          status: code === "grpc_request_timeout" ? "DEADLINE_EXCEEDED" : "CANCELLED",
          tlsMode: current.tls.mode,
          credentialUsed: current.tls.credentialUsed,
        });
      }
      if (code === "grpc_connection_stale") {
        generationRef.current += 1;
        resetConnection();
      }
    } finally {
      if (activeRef.current?.requestId === requestId) {
        activeRef.current = null;
        setActiveRequest(null);
      }
    }
  };

  const onCancel = async () => {
    const active = activeRef.current;
    if (!active) return;
    try {
      await cancelGrpc(active.connectionId, active.requestId);
    } catch (cause) {
      setErrorCode(localErrorCode(cause));
    }
  };

  const onExport = async (summary: GrpcExchangeSummary) => {
    try {
      const saved = await exportGrpcSummary(summary);
      if (saved) setNotice("gRPC summary를 저장했습니다. message body와 credential 정보는 포함하지 않았습니다.");
      setErrorCode(null);
    } catch (cause) {
      setErrorCode(localErrorCode(cause));
    }
  };

  const onClearHistory = () => {
    try {
      const cleared = clearGrpcHistory();
      historyRef.current = cleared;
      setHistory(cleared);
      setNotice("gRPC 요약 기록을 비웠습니다.");
      setErrorCode(null);
    } catch {
      setErrorCode("grpc_history_failed");
    }
  };

  const connected = phase === "connected" && connection !== null;
  const busy = phase === "connecting" || phase === "disconnecting";
  const importReady = credentialLabel.trim().length > 0
    && Boolean(caSelection || (certificateSelection && keySelection))
    && Boolean(certificateSelection) === Boolean(keySelection);

  return (
    <section
      id="protocol-panel-grpc"
      className="protocol-lab grpc-lab"
      role="tabpanel"
      aria-labelledby="protocol-tab-grpc grpc-lab-heading"
    >
      <div className="protocol-lab-head">
        <div>
          <h2 id="grpc-lab-heading">Protocol Lab · gRPC</h2>
          <p className="dim">
            로컬 proto 또는 서버 reflection으로 descriptor를 확인하고 네 가지 RPC kind를 명시적으로 호출합니다.
          </p>
        </div>
        <span className="mcp-memory-badge">응답 본문 · 메모리 전용</span>
      </div>

      <p className="dim mcp-storage-disclosure">
        기록/내보내기에는 method, count, status, 시간, TLS mode만 저장합니다. ProtoJSON 본문, 엔드포인트,
        metadata, descriptor, 네이티브 경로, 자격 증명 ID와 PEM은 저장하지 않습니다.
      </p>

      {!native && (
        <div className="mcp-notice" role="note">
          브라우저 미리보기에서는 gRPC 연결이나 native 파일 선택을 실행하지 않습니다. 데스크톱 앱에서 사용하세요.
        </div>
      )}

      <section className="grpc-panel" aria-labelledby="grpc-profile-heading">
        <h3 id="grpc-profile-heading">연결 프로필</h3>
        <div className="grpc-profile-grid">
          <label>
            스키마 소스
            <select
              aria-label="gRPC 스키마 소스"
              value={sourceMode}
              disabled={connected || busy}
              onChange={(event) => setSourceMode(event.currentTarget.value as SourceMode)}
            >
              <option value="local-proto">로컬 proto</option>
              <option value="reflection">서버 reflection · v1 우선</option>
            </select>
          </label>
          <label>
            엔드포인트
            <input
              aria-label="gRPC 엔드포인트"
              type="url"
              value={endpoint}
              maxLength={8 * 1024}
              disabled={connected || busy}
              spellCheck={false}
              onChange={(event) => setEndpoint(event.currentTarget.value)}
            />
          </label>
          <label>
            연결 제한 시간(ms)
            <input
              aria-label="gRPC 연결 제한 시간"
              type="number"
              min={100}
              max={30_000}
              value={connectTimeoutMs}
              disabled={connected || busy}
              onChange={(event) => setConnectTimeoutMs(Number(event.currentTarget.value))}
            />
          </label>
          <label>
            RPC 기한(ms)
            <input
              aria-label="gRPC RPC 기한"
              type="number"
              min={100}
              max={300_000}
              value={rpcTimeoutMs}
              disabled={connected || busy}
              onChange={(event) => setRpcTimeoutMs(Number(event.currentTarget.value))}
            />
          </label>
        </div>

        {sourceMode === "local-proto" ? (
          <div className="grpc-selection-grid">
            <NativeSelectionRow
              title="루트 proto"
              action="proto 선택"
              selection={protoSelection}
              disabled={!native || connected || busy}
              onPick={() => void pickSource("proto")}
              onClear={() => setProtoSelection(null)}
            />
            <NativeSelectionRow
              title="가져오기 루트 · 선택"
              action="가져오기 루트 선택"
              selection={importRootSelection}
              disabled={!native || connected || busy}
              onPick={() => void pickSource("import-root")}
              onClear={() => setImportRootSelection(null)}
            />
          </div>
        ) : (
          <p className="dim grpc-inline-note">
            reflection v1만 먼저 시도하며, 그 경계가 UNIMPLEMENTED일 때만 v1alpha를 시도합니다.
          </p>
        )}

        <details className="grpc-tls-panel" open={https}>
          <summary>TLS / mTLS</summary>
          <fieldset disabled={connected || busy || !https}>
            <div className="grpc-profile-grid">
              <label>
                루트 방식
                <select
                  aria-label="gRPC TLS 루트 방식"
                  value={rootMode}
                  onChange={(event) => setRootMode(event.currentTarget.value as GrpcRootMode)}
                >
                  <option value="native">네이티브 루트</option>
                  <option value="custom">사용자 지정 CA만</option>
                  <option value="native+custom">네이티브 + 사용자 지정 CA</option>
                </select>
              </label>
              <label>
                서버 이름 재정의 · 선택
                <input
                  aria-label="gRPC 서버 이름 재정의"
                  value={serverName}
                  maxLength={253}
                  spellCheck={false}
                  onChange={(event) => setServerName(event.currentTarget.value)}
                />
              </label>
              <label>
                저장된 TLS 자격 증명
                <select
                  aria-label="gRPC TLS 자격 증명"
                  value={credentialId}
                  onChange={(event) => setCredentialId(event.currentTarget.value)}
                >
                  <option value="">자격 증명 없음</option>
                  {credentials.map((credential) => (
                    <option key={credential.credentialId} value={credential.credentialId}>
                      {credential.label} · {credential.hasCustomCa ? "CA" : "no CA"}
                      {credential.hasClientIdentity ? " + mTLS" : ""}
                    </option>
                  ))}
                </select>
              </label>
            </div>
          </fieldset>
          {!https && <p className="dim">현재 endpoint는 plaintext입니다. TLS material은 전송되지 않습니다.</p>}
        </details>

        {profileIssue && <p className="grpc-validation" role="status">{profileIssue}</p>}
        <div className="mcp-inline-actions">
          {connected ? (
            <button className="btn" type="button" disabled={busy || Boolean(activeRequest)} onClick={() => void onDisconnect()}>
              연결 해제
            </button>
          ) : (
            <button
              className="btn send"
              type="button"
              disabled={!native || busy || Boolean(profileIssue)}
              onClick={() => void onConnect()}
            >
              {phase === "connecting" ? "연결 중..." : "gRPC 연결"}
            </button>
          )}
        </div>
      </section>

      <details className="grpc-panel grpc-credential-panel">
        <summary>TLS 자격 증명 관리자 · Windows DPAPI</summary>
        <p className="dim">
          CA는 선택 사항입니다. client certificate와 암호화되지 않은 private key는 반드시 한 쌍으로 가져옵니다.
          PEM 내용과 native 경로는 화면에 표시되지 않습니다.
        </p>
        <div className="grpc-credential-import">
          <label>
            자격 증명 이름
            <input
              aria-label="gRPC 자격 증명 이름"
              value={credentialLabel}
              maxLength={256}
              disabled={!native || credentialBusy}
              onChange={(event) => setCredentialLabel(event.currentTarget.value)}
            />
          </label>
          <NativeSelectionRow
            title="CA 번들 · 선택"
            action="CA 선택"
            selection={caSelection}
            disabled={!native || credentialBusy}
            onPick={() => void pickCredentialFile("ca")}
            onClear={() => setCaSelection(null)}
          />
          <NativeSelectionRow
            title="클라이언트 인증서"
            action="클라이언트 인증서 선택"
            selection={certificateSelection}
            disabled={!native || credentialBusy}
            onPick={() => void pickCredentialFile("certificate")}
            onClear={() => setCertificateSelection(null)}
          />
          <NativeSelectionRow
            title="Private key"
            action="private key 선택"
            selection={keySelection}
            disabled={!native || credentialBusy}
            onPick={() => void pickCredentialFile("key")}
            onClear={() => setKeySelection(null)}
          />
          <div className="mcp-inline-actions">
            <button
              className="btn send"
              type="button"
              disabled={!native || credentialBusy || !importReady}
              onClick={() => void importCredential()}
            >
              암호화된 자격 증명 가져오기
            </button>
            <button className="btn" type="button" disabled={!native || credentialBusy} onClick={() => void refreshCredentials()}>
              자격 증명 새로 고침
            </button>
          </div>
        </div>
        <ul className="grpc-credential-list">
          {credentials.map((credential) => (
            <li key={credential.credentialId}>
              <strong>{credential.label}</strong>
              <span>{credential.hasCustomCa ? "사용자 지정 CA" : "네이티브 루트만"}</span>
              <span>{credential.hasClientIdentity ? "클라이언트 ID 포함" : "클라이언트 ID 없음"}</span>
              <time dateTime={new Date(credential.createdAtMs).toISOString()}>
                {new Date(credential.createdAtMs).toLocaleString()}
              </time>
              <button
                className="btn"
                type="button"
                aria-label={`${credential.label} TLS credential 삭제`}
                disabled={!native || credentialBusy}
                onClick={() => void removeCredential(credential.credentialId)}
              >
                삭제
              </button>
            </li>
          ))}
          {credentials.length === 0 && <li className="dim">저장된 TLS 자격 증명이 없습니다.</li>}
        </ul>
      </details>

      {connection && (
        <section className="grpc-panel" aria-labelledby="grpc-method-heading">
          <div className="grpc-connection-card" role="status">
            <strong>{connection.authority}</strong>
            <span>{connection.source.kind}{connection.source.label ? ` · ${connection.source.label}` : ""}</span>
            <span>서비스 {connection.source.serviceCount}개 · 메서드 {connection.methods.length}개</span>
            <span>descriptor 파일 {connection.source.descriptorFileCount}개</span>
            <span>{connection.tls.mode}{connection.tls.credentialUsed ? " · 자격 증명 사용" : ""}</span>
          </div>
          <h3 id="grpc-method-heading">메서드 탐색기</h3>
          <div className="grpc-method-controls">
            <label>
              메서드 필터
              <input
                aria-label="gRPC 메서드 필터"
                value={methodQuery}
                maxLength={1024}
                onChange={(event) => setMethodQuery(event.currentTarget.value)}
              />
            </label>
            <label>
              메서드
              <select
                aria-label="gRPC method"
                value={methodName}
                onChange={(event) => setMethodName(event.currentTarget.value)}
              >
                {filteredMethods.map((method) => (
                  <option key={method.fullName} value={method.fullName}>
                    {method.fullName} · {method.rpcKind}
                  </option>
                ))}
              </select>
            </label>
          </div>
          {selectedMethod && (
            <div className="grpc-method-card">
              <code>{selectedMethod.service}/{selectedMethod.method}</code>
              <span>{selectedMethod.rpcKind}</span>
              <span>{selectedMethod.inputType} → {selectedMethod.outputType}</span>
            </div>
          )}
          <label className="grpc-editor-label">
            {selectedMethod && (selectedMethod.rpcKind === "client-streaming"
              || selectedMethod.rpcKind === "bidirectional-streaming")
              ? "ProtoJSON 메시지 배열"
              : "ProtoJSON 메시지"}
            <textarea
              aria-label="gRPC ProtoJSON request"
              className="grpc-editor"
              value={requestText}
              maxLength={4 * 1024 * 1024}
              spellCheck={false}
              onChange={(event) => setRequestText(event.currentTarget.value)}
            />
          </label>
          {inputIssue && <p className="grpc-validation" role="status">{inputIssue}</p>}
          <div className="mcp-inline-actions">
            <button
              className="btn send"
              type="button"
              disabled={!selectedMethod || Boolean(inputIssue) || Boolean(activeRequest)}
              onClick={() => void onInvoke()}
            >
              {activeRequest ? "RPC 실행 중..." : "RPC 호출"}
            </button>
            {activeRequest && (
              <button className="btn" type="button" onClick={() => void onCancel()}>
                취소
              </button>
            )}
          </div>
          {result && (
            <section className="grpc-result" aria-labelledby="grpc-result-heading">
              <div>
                <h3 id="grpc-result-heading">결과</h3>
                <span className={result.ok ? "grpc-status-ok" : "grpc-status-error"}>{result.status}</span>
                <span>메시지 {result.responseMessageCount}개 · {result.elapsedMs}ms</span>
              </div>
              <pre>{boundedJson(result.responses, 1024 * 1024)}</pre>
            </section>
          )}
        </section>
      )}

      <section className="grpc-panel" aria-labelledby="grpc-history-heading">
        <div className="grpc-history-head">
          <div>
            <h3 id="grpc-history-heading">요약 기록</h3>
            <p className="dim">최대 50개 · 본문/엔드포인트/경로/자격 증명 ID 미저장</p>
          </div>
          <button className="btn" type="button" disabled={history.entries.length === 0} onClick={onClearHistory}>
            기록 지우기
          </button>
        </div>
        <ol className="grpc-history-list">
          {history.entries.map((entry, index) => (
            <li key={`${entry.startedAtMs}-${entry.service}-${entry.method}-${index}`}>
              <div>
                <strong>{entry.service}/{entry.method}</strong>
                <code>{entry.rpcKind} · {entry.status}</code>
                <span>{entry.requestMessageCount} → {entry.responseMessageCount}개 메시지 · {entry.elapsedMs}ms</span>
                <span>{entry.sourceKind} · {entry.tlsMode}{entry.credentialUsed ? " · 자격 증명 사용" : ""}</span>
                <time dateTime={new Date(entry.startedAtMs).toISOString()}>
                  {new Date(entry.startedAtMs).toLocaleString()}
                </time>
              </div>
              <button
                className="btn"
                type="button"
                disabled={!native}
                onClick={() => void onExport(entry)}
              >
                요약 내보내기
              </button>
            </li>
          ))}
          {history.entries.length === 0 && <li className="dim">아직 저장된 gRPC 요약이 없습니다.</li>}
        </ol>
      </section>

      {errorCode && (
        <div className="mcp-error" role="alert">
          {ERROR_LABELS[errorCode] ?? ERROR_LABELS.grpc_protocol_failed}
        </div>
      )}
      {notice && <p className="dim grpc-notice" role="status">{notice}</p>}
    </section>
  );
}

function NativeSelectionRow({
  title,
  action,
  selection,
  disabled,
  onPick,
  onClear,
}: {
  title: string;
  action: string;
  selection: GrpcNativeSelection | null;
  disabled: boolean;
  onPick: () => void;
  onClear: () => void;
}) {
  return (
    <div className="grpc-selection-row">
      <span>{title}</span>
      <button className="btn" type="button" disabled={disabled} onClick={onPick}>{action}</button>
      <span role="status">{selection?.label ?? "선택하지 않음"}</span>
      {selection && <button className="btn" type="button" disabled={disabled} onClick={onClear}>지우기</button>}
    </div>
  );
}

function toSummary(
  connection: GrpcConnectResult,
  method: GrpcMethodProjection,
  result: GrpcInvokeResult,
): GrpcExchangeSummary {
  return {
    sourceKind: connection.source.kind,
    service: method.service,
    method: method.method,
    rpcKind: method.rpcKind,
    requestMessageCount: result.requestMessageCount,
    responseMessageCount: result.responseMessageCount,
    startedAtMs: result.startedAtMs,
    elapsedMs: result.elapsedMs,
    status: result.status,
    tlsMode: connection.tls.mode,
    credentialUsed: connection.tls.credentialUsed,
  };
}

function localErrorCode(cause: unknown): string {
  if (cause instanceof Error && cause.message === "grpc_history_failed") return cause.message;
  return safeGrpcErrorCode(cause);
}

function boundedJson(value: unknown, maximum: number): string {
  try {
    const serialized = JSON.stringify(value, null, 2);
    return serialized.length <= maximum
      ? serialized
      : `${serialized.slice(0, maximum)}\n… UI preview truncated; body remains memory-only …`;
  } catch {
    return "[표시할 수 없는 response]";
  }
}
