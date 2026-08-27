import { useEffect, useMemo, useRef, useState, type ChangeEvent, type FormEvent, type KeyboardEvent } from "react";
import { fetchOpenApiSource } from "./api";
import {
  displayOpenApiFileName,
  OPENAPI_LIMITS,
  parseOpenApiSource,
  selectOpenApiServer,
  type OpenApiImportPreview,
  type OpenApiOperationPreview,
} from "./lib/openapi";

interface OpenApiImportProps {
  onClose: () => void;
  onApply: (operation: OpenApiOperationPreview) => void;
  onAddToCollection: (operations: OpenApiOperationPreview[]) => Promise<void>;
}

function issueText(operation: OpenApiOperationPreview): string {
  const error = operation.errors[0] ?? operation.warnings[0];
  return error?.message ?? "";
}

export function OpenApiImport({ onClose, onApply, onAddToCollection }: OpenApiImportProps) {
  const [preview, setPreview] = useState<OpenApiImportPreview | null>(null);
  const [selected, setSelected] = useState<Record<string, boolean>>({});
  const [serverIndex, setServerIndex] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [applying, setApplying] = useState(false);
  const [urlInput, setUrlInput] = useState("");
  const requestId = useRef(0);
  const busyRef = useRef(false);
  const applyingRef = useRef(false);
  const mountedRef = useRef(true);
  const dialogRef = useRef<HTMLDivElement>(null);
  const returnFocusRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    mountedRef.current = true;
    returnFocusRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialogRef.current?.querySelector<HTMLElement>("button, input, select")?.focus();
    return () => {
      mountedRef.current = false;
      requestId.current += 1;
      if (returnFocusRef.current?.isConnected) returnFocusRef.current.focus();
    };
  }, []);

  const displayedPreview = useMemo(() => {
    if (!preview || serverIndex === null) return preview;
    return selectOpenApiServer(preview, serverIndex);
  }, [preview, serverIndex]);

  const selectedOperations = displayedPreview?.operations.filter((operation) => selected[operation.id] && operation.applyable) ?? [];
  const selectedApplyable = selectedOperations.length === 1 ? selectedOperations[0] : null;

  const loadPreview = async (
    loader: () => Promise<ReturnType<typeof parseOpenApiSource>>,
    failureMessage: string,
  ) => {
    if (busyRef.current || applyingRef.current) return;
    const currentRequest = ++requestId.current;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    setPreview(null);
    setSelected({});
    setServerIndex(null);
    try {
      const result = await loader();
      if (!mountedRef.current || currentRequest !== requestId.current) return;
      if (!result.ok) {
        setError(result.error.message);
        return;
      }
      const firstServer = result.preview.servers[0]?.index ?? null;
      setPreview(result.preview);
      setServerIndex(firstServer);
      setSelected(Object.fromEntries(
        result.preview.operations.map((operation) => [operation.id, operation.applyable]),
      ));
    } catch {
      if (mountedRef.current && currentRequest === requestId.current) setError(failureMessage);
    } finally {
      if (currentRequest === requestId.current) {
        busyRef.current = false;
        if (mountedRef.current) setBusy(false);
      }
    }
  };

  const readFile = async (event: ChangeEvent<HTMLInputElement>) => {
    if (busyRef.current || applyingRef.current) return;
    const file = event.currentTarget.files?.[0];
    event.currentTarget.value = "";
    if (!file) return;
    if (file.size > OPENAPI_LIMITS.maxBytes) {
      requestId.current += 1;
      setPreview(null);
      setSelected({});
      setServerIndex(null);
      setError("OpenAPI 파일은 4 MiB 이하만 가져올 수 있습니다.");
      return;
    }
    await loadPreview(async () => {
      const text = await file.text();
      return parseOpenApiSource({ kind: "file", name: file.name, text });
    }, "OpenAPI 파일을 안전하게 읽지 못했습니다.");
  };

  const readUrl = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const url = urlInput.trim();
    if (!url) {
      setError("가져올 OpenAPI URL을 입력하세요.");
      return;
    }
    await loadPreview(async () => {
      const source = await fetchOpenApiSource(url);
      return parseOpenApiSource({ kind: "url", format: source.format, text: source.text });
    }, "OpenAPI URL을 안전하게 가져오지 못했습니다.");
  };

  const closeOnEscape = (event: KeyboardEvent<HTMLDivElement>) => {
    if (
      event.key === "Escape"
      && !event.nativeEvent.isComposing
      && !busyRef.current
      && !applyingRef.current
    ) {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key !== "Tab") return;
    const dialog = dialogRef.current;
    if (!dialog) return;
    const focusable = [...dialog.querySelectorAll<HTMLElement>(
      "button:not([disabled]), input:not([disabled]):not([type=hidden]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex=\"-1\"])",
    )].filter((element) => element.getAttribute("aria-hidden") !== "true");
    if (focusable.length === 0) {
      event.preventDefault();
      dialog.focus();
      return;
    }
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  const applyCurrent = () => {
    if (!selectedApplyable || busyRef.current || applyingRef.current) return;
    onApply(selectedApplyable);
    onClose();
  };

  const addCollection = async () => {
    if (selectedOperations.length === 0 || busyRef.current || applyingRef.current) return;
    applyingRef.current = true;
    setApplying(true);
    setError(null);
    try {
      await onAddToCollection(selectedOperations);
      if (mountedRef.current) onClose();
    } catch {
      if (mountedRef.current) setError("선택한 OpenAPI request를 Collection에 안전하게 저장하지 못했습니다.");
    } finally {
      applyingRef.current = false;
      if (mountedRef.current) setApplying(false);
    }
  };

  return (
    <div className="openapi-overlay">
      <div
        ref={dialogRef}
        className="openapi-dialog"
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-labelledby="openapi-import-title"
        aria-describedby="openapi-import-description"
        aria-busy={busy || applying}
        onKeyDown={closeOnEscape}
      >
        <div className="openapi-dialog-head">
          <div>
            <h2 id="openapi-import-title">OpenAPI 가져오기</h2>
            <p id="openapi-import-description">
              OpenAPI 3.0/3.1 JSON 또는 YAML을 미리 확인한 뒤 request draft로 적용합니다.
            </p>
          </div>
          <button className="btn" type="button" onClick={onClose} disabled={busy || applying} aria-label="OpenAPI 가져오기 닫기">
            닫기
          </button>
        </div>

        <div className="openapi-source-row">
          <input
            id="openapi-file-input"
            className="openapi-file-input"
            type="file"
            accept=".json,.yaml,.yml,application/json,application/yaml,text/yaml"
            onChange={(event) => void readFile(event)}
            disabled={busy || applying}
          />
          <label className="btn openapi-file-label" htmlFor="openapi-file-input">
            로컬 파일 선택
          </label>
          <span className="dim">최대 4 MiB · 로컬 파일은 완전 오프라인</span>
        </div>
        <form className="openapi-url-row" onSubmit={(event) => void readUrl(event)}>
          <label htmlFor="openapi-url-input">OpenAPI URL</label>
          <input
            id="openapi-url-input"
            type="url"
            value={urlInput}
            onChange={(event) => setUrlInput(event.currentTarget.value)}
            placeholder="https://example.test/openapi.yaml"
            maxLength={2_048}
            disabled={busy || applying}
            spellCheck={false}
          />
          <button className="btn" type="submit" disabled={busy || applying || !urlInput.trim()}>
            URL 가져오기
          </button>
        </form>
        <div className="openapi-offline-note">
          URL은 native 경계에서 최대 4 MiB로만 가져옵니다. 자동 전송과 secret 값 주입은 하지 않으며 서버 주소와 인증 유형만 draft에 넣고 실제 값은 비워 둡니다.
        </div>

        {busy && <div className="openapi-status" role="status">OpenAPI 문서를 안전하게 읽고 해석하는 중…</div>}
        {error && <div className="error" role="alert">{error}</div>}
        {displayedPreview && (
          <>
            <div className="openapi-summary" role="status">
              <span>{displayedPreview.sourceName ? displayOpenApiFileName(displayedPreview.sourceName) : "OpenAPI 문서"}</span>
              <span>OpenAPI {displayedPreview.version}</span>
              <span>{displayedPreview.operations.length}개 operation</span>
            </div>
            {displayedPreview.servers.length > 0 && (
              <label className="openapi-server-row">
                <span>Server</span>
                <select
                  value={serverIndex ?? displayedPreview.servers[0].index}
                  onChange={(event) => setServerIndex(Number(event.currentTarget.value))}
                  disabled={busy || applying}
                  aria-label="OpenAPI server 선택"
                >
                  {displayedPreview.servers.map((server) => (
                    <option key={server.index} value={server.index}>{server.url}</option>
                  ))}
                </select>
              </label>
            )}
            {displayedPreview.errors.length > 0 && (
              <div className="openapi-document-warnings" role="status">
                {displayedPreview.errors.slice(0, 5).map((entry, index) => <div key={`${entry.code}-${index}`}>{entry.message}</div>)}
              </div>
            )}
            <div className="openapi-operations" aria-label="OpenAPI operation 미리보기">
              {displayedPreview.operations.map((operation) => {
                const disabled = !operation.applyable || busy || applying;
                return (
                  <label className={`openapi-operation ${operation.applyable ? "" : "is-invalid"}`} key={operation.id}>
                    <input
                      type="checkbox"
                      checked={Boolean(selected[operation.id])}
                      onChange={(event) => setSelected((current) => ({ ...current, [operation.id]: event.currentTarget.checked }))}
                      disabled={disabled}
                    />
                    <span className={`method ${operation.method.toLowerCase()}`}>{operation.method}</span>
                    <span className="openapi-operation-main">
                      <span className="openapi-operation-label">{operation.label}</span>
                      <span className="openapi-operation-meta">
                        {operation.parameters.length > 0 ? `parameters ${operation.parameters.length}` : "parameters 없음"}
                        {operation.requestBody ? ` · body ${operation.requestBody.mediaType}` : ""}
                        {operation.security ? ` · auth ${operation.security.kind}` : ""}
                      </span>
                      {operation.parameters.length > 0 && (
                        <span className="openapi-operation-detail">
                          {operation.parameters.map((parameter) => `${parameter.location}:${parameter.name}${parameter.redacted ? " (값 비공개)" : ""}`).join(" · ")}
                        </span>
                      )}
                      {operation.requestBody && (
                        <span className="openapi-operation-detail">
                          body example {operation.requestBody.exampleIncluded ? "포함" : "없음"}{operation.requestBody.redacted ? " · 민감 property 비공개" : ""}
                        </span>
                      )}
                      {operation.security && (
                        <span className="openapi-operation-detail">
                          auth {operation.security.location ?? "metadata"}:{operation.security.name} · 값 비공개
                        </span>
                      )}
                      {issueText(operation) && <span className="openapi-operation-issue">{issueText(operation)}</span>}
                    </span>
                  </label>
                );
              })}
              {displayedPreview.operations.length === 0 && <div className="dim">미리볼 수 있는 operation이 없습니다.</div>}
            </div>
          </>
        )}

        <div className="openapi-dialog-actions">
          <span className="dim">체크한 operation은 새 항목으로만 추가되며 기존 Collection을 덮어쓰지 않습니다.</span>
          <div className="openapi-action-buttons">
            <button className="btn" type="button" onClick={applyCurrent} disabled={!selectedApplyable || busy || applying}>
              현재 draft에 적용
            </button>
            <button className="btn send" type="button" onClick={() => void addCollection()} disabled={selectedOperations.length === 0 || busy || applying}>
              {applying ? "저장 중…" : `새 Collection에 추가 (${selectedOperations.length})`}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
