import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { isTauri } from "./lib/isTauri";
import type {
  ApiResponse,
  BinaryResponse,
  GraphqlResponse,
  ResponseCookie,
  ToolboxDispatch,
} from "./types";

export type RawResponseCopyKind = "headers" | "cookies";
type ResponseTab = "body" | "headers" | "cookies";

const RESPONSE_TABS: readonly ResponseTab[] = ["body", "headers", "cookies"];

export const TOOLBOX_SELECTION_MESSAGES = {
  empty: "선택 영역이 비어 있습니다. 현재 응답 본문에서 텍스트를 선택하세요.",
  outside: "선택 영역은 현재 응답 본문 안에 있어야 합니다.",
  stale: "응답이 변경되어 선택 영역을 보낼 수 없습니다. 현재 응답에서 다시 선택하세요.",
  nativeOnly: "Developer Toolbox로 보내기는 데스크톱 앱에서만 사용할 수 있습니다. 클립보드로 자동 전환하지 않습니다",
  success: "선택 영역을 Developer Toolbox로 보냈습니다.",
  redacted: "민감한 값이 마스킹된 선택 영역을 Developer Toolbox로 보냈습니다.",
  error: "Developer Toolbox로 선택 영역을 전달하지 못했습니다. 클립보드로 자동 전환하지 않습니다",
} as const;

export type ResponseSelectionCheck =
  | { kind: "valid"; text: string }
  | { kind: "empty" }
  | { kind: "outside" };

/**
 * Inspect only the Selection whose boundary points are inside the rendered
 * response body. The caller supplies the body element so controls, headers,
 * cookies, and any other page text can never become handoff input.
 */
export function inspectResponseSelection(
  body: HTMLElement | null,
  selection: Selection | null,
): ResponseSelectionCheck {
  if (!body || !selection || selection.rangeCount !== 1) return { kind: "empty" };

  const range = selection.getRangeAt(0);
  const nodes = [
    range.startContainer,
    range.endContainer,
    range.commonAncestorContainer,
    selection.anchorNode,
    selection.focusNode,
  ];
  if (nodes.some((node) => node !== null && !body.contains(node))) {
    return { kind: "outside" };
  }
  if (range.collapsed) return { kind: "empty" };

  const text = selection.toString();
  return text.trim() ? { kind: "valid", text } : { kind: "empty" };
}

interface ResponseViewerProps {
  response: ApiResponse | null;
  responseText: string;
  pretty: boolean;
  onPrettyChange: (pretty: boolean) => void;
  onRawCopy: (kind: RawResponseCopyKind, responseId: string) => Promise<string>;
  onBinarySave: (responseId: string) => Promise<boolean>;
  onSendSelection?: (text: string) => Promise<ToolboxDispatch>;
  native?: boolean;
  onError: (message: string) => void;
}

export function formatResponseHeaders(response: ApiResponse): string {
  return response.headers.map((header) => `${header.key}: ${header.value}`).join("\n");
}

export function formatResponseCookies(cookies: readonly ResponseCookie[]): string {
  return cookies.map((cookie) => {
    const attributes = cookie.attributes
      .map((attribute) => attribute.value ? `${attribute.key}=${attribute.value}` : attribute.key)
      .join("; ");
    return `${cookie.name}=${cookie.value}${attributes ? `; ${attributes}` : ""}`;
  }).join("\n");
}

export function formatGraphqlData(data: unknown): string {
  try {
    return JSON.stringify(data, null, 2) ?? "";
  } catch {
    return "";
  }
}

function GraphqlResponseSummary({ response, graphql }: { response: ApiResponse; graphql: GraphqlResponse }) {
  const httpError = response.status >= 400;
  return (
    <section className="graphql-response-summary" aria-label="GraphQL response summary">
      <div className="graphql-response-state">
        <span className={httpError ? "graphql-http-error" : "graphql-http-ok"}>
          HTTP {httpError ? "error" : "success"} ({response.status})
        </span>
        <span className="dim">GraphQL envelope: {graphql.envelope}</span>
      </div>
      {graphql.errors.length > 0 && (
        <div className="graphql-errors" role="alert" aria-label="GraphQL errors">
          <div className="graphql-section-label">GraphQL errors ({graphql.errors.length})</div>
          {graphql.errors.map((error, index) => (
            <div className="graphql-error-item" key={`${index}-${error.message}`}>
              <div>{error.message}</div>
              {(error.path.length > 0 || error.locations.length > 0) && (
                <div className="dim graphql-error-meta">
                  {error.path.length > 0 && `path: ${error.path.join(".")}`}
                  {error.path.length > 0 && error.locations.length > 0 && " · "}
                  {error.locations.length > 0 && `location: ${error.locations.map((location) => `${location.line}:${location.column}`).join(", ")}`}
                </div>
              )}
            </div>
          ))}
          {graphql.errors_truncated && <div className="dim">Additional GraphQL errors were omitted at the display limit.</div>}
        </div>
      )}
      {graphql.data !== null && graphql.data !== undefined && (
        <div className="graphql-data">
          <div className="graphql-section-label">GraphQL data</div>
          <pre className="resp-body graphql-data-body">{formatGraphqlData(graphql.data) || " "}</pre>
        </div>
      )}
      {graphql.envelope !== "valid" && (
        <div className="dim">The response is retained in the Body tab, but its GraphQL envelope could not be safely projected.</div>
      )}
    </section>
  );
}

function BinaryResponseSummary({
  response,
  binary,
  saving,
  onSave,
}: {
  response: ApiResponse;
  binary: BinaryResponse;
  saving: boolean;
  onSave: () => void;
}) {
  const canSave = binary.save_available && Boolean(response.response_id);
  return (
    <section className="binary-response-summary" aria-label="Binary response preview">
      <div className="binary-response-meta">
        <span><strong>Type</strong> {binary.media_type}</span>
        <span><strong>Size</strong> {binary.size_bytes.toLocaleString()} bytes</span>
        <span className="spacer" />
        <button
          type="button"
          className="btn"
          disabled={!canSave || saving}
          title={canSave ? "현재 응답 binary를 native dialog에서 한 번 저장" : "현재 응답을 native 앱에서만 저장할 수 있습니다"}
          onClick={onSave}
        >
          {saving ? "Saving..." : "Save binary"}
        </button>
      </div>
      <div className="binary-response-field">
        <span className="binary-response-label">Hex preview</span>
        <code>{binary.hex_preview || "(empty)"}</code>
        {binary.hex_truncated && <span className="dim">preview truncated</span>}
      </div>
      {binary.text_preview !== null && binary.text_preview !== undefined && (
        <div className="binary-response-field">
          <span className="binary-response-label">UTF-8 preview</span>
          <code>{binary.text_preview || "(empty)"}</code>
          {binary.text_truncated && <span className="dim">preview truncated</span>}
        </div>
      )}
      {!canSave && <div className="dim">Binary 원문은 bounded memory에만 있으며 데스크톱 native save에서만 내보낼 수 있습니다.</div>}
    </section>
  );
}

export function ResponseViewer({
  response,
  responseText,
  pretty,
  onPrettyChange,
  onRawCopy,
  onBinarySave,
  onSendSelection,
  native = isTauri(),
  onError,
}: ResponseViewerProps) {
  const [tab, setTab] = useState<ResponseTab>("body");
  const [copyingRaw, setCopyingRaw] = useState<RawResponseCopyKind | null>(null);
  const [savingBinary, setSavingBinary] = useState(false);
  const [sendingSelection, setSendingSelection] = useState(false);
  const [toolboxFeedback, setToolboxFeedback] = useState<{
    kind: "success" | "error";
    message: string;
  } | null>(null);
  const responseBodyRef = useRef<HTMLPreElement | null>(null);
  const renderRevisionRef = useRef(0);
  const selectionRef = useRef<{
    body: HTMLElement;
    text: string;
    revision: number;
  } | null>(null);
  const lastSelectionRevisionRef = useRef<number | null>(null);

  useEffect(() => {
    setTab("body");
    setCopyingRaw(null);
    setSavingBinary(false);
  }, [response]);

  // A response or its rendered form (for example pretty JSON) starts a new
  // revision. A previously captured DOM Range must not cross that boundary.
  useLayoutEffect(() => {
    renderRevisionRef.current += 1;
    if (selectionRef.current) {
      lastSelectionRevisionRef.current = selectionRef.current.revision;
    }
    selectionRef.current = null;
    setSendingSelection(false);
    setToolboxFeedback(null);
  }, [response, responseText]);

  // Keep the latest browser Selection in renderer memory only. No clipboard
  // read/write or native call occurs from this listener.
  useEffect(() => {
    const onSelectionChange = () => {
      const check = inspectResponseSelection(responseBodyRef.current, window.getSelection());
      if (check.kind !== "valid" || !responseBodyRef.current) {
        selectionRef.current = null;
        return;
      }
      selectionRef.current = {
        body: responseBodyRef.current,
        text: check.text,
        revision: renderRevisionRef.current,
      };
      lastSelectionRevisionRef.current = renderRevisionRef.current;
    };
    document.addEventListener("selectionchange", onSelectionChange);
    return () => document.removeEventListener("selectionchange", onSelectionChange);
  }, []);

  const copyMasked = async (text: string, failureMessage: string) => {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      onError(failureMessage);
    }
  };

  const copyRaw = async (kind: RawResponseCopyKind) => {
    if (!response?.raw_headers_available || !response.response_id || copyingRaw) return;
    const label = kind === "headers" ? "header" : "Set-Cookie";
    const confirmed = window.confirm(
      `원문 응답 ${label}에는 session, token, Cookie 같은 민감정보가 포함될 수 있습니다. 클립보드에 한 번 복사할까요?`,
    );
    if (!confirmed) return;
    setCopyingRaw(kind);
    try {
      const raw = await onRawCopy(kind, response.response_id);
      await navigator.clipboard.writeText(raw);
    } catch {
      onError(`원문 응답 ${label}를 안전하게 복사하지 못했습니다.`);
    } finally {
      setCopyingRaw(null);
    }
  };

  const saveBinary = async () => {
    if (!response?.binary?.save_available || !response.response_id || savingBinary) return;
    setSavingBinary(true);
    try {
      await onBinarySave(response.response_id);
    } catch {
      onError("binary 응답을 안전하게 저장하지 못했습니다.");
    } finally {
      setSavingBinary(false);
    }
  };

  const sendSelection = async () => {
    if (!response || response.binary || sendingSelection) return;

    const body = responseBodyRef.current;
    const check = inspectResponseSelection(body, window.getSelection());
    const revision = renderRevisionRef.current;
    const previous = selectionRef.current;
    const staleRevision = lastSelectionRevisionRef.current !== null
      && lastSelectionRevisionRef.current !== revision;

    if (staleRevision) {
      setToolboxFeedback({ kind: "error", message: TOOLBOX_SELECTION_MESSAGES.stale });
      return;
    }
    if (check.kind === "empty") {
      setToolboxFeedback({ kind: "error", message: TOOLBOX_SELECTION_MESSAGES.empty });
      return;
    }
    if (check.kind === "outside") {
      setToolboxFeedback({ kind: "error", message: TOOLBOX_SELECTION_MESSAGES.outside });
      return;
    }
    if (!body) {
      setToolboxFeedback({ kind: "error", message: TOOLBOX_SELECTION_MESSAGES.empty });
      return;
    }

    // A selection event normally populates this snapshot. The fallback makes
    // programmatic/native test selections safe on the first revision while a
    // prior revision is still rejected by the guard above.
    if (!previous) {
      selectionRef.current = { body, text: check.text, revision };
      lastSelectionRevisionRef.current = revision;
    } else if (
      previous.body !== body
      || previous.revision !== revision
      || previous.text !== check.text
    ) {
      setToolboxFeedback({ kind: "error", message: TOOLBOX_SELECTION_MESSAGES.stale });
      return;
    }

    if (!native) {
      setToolboxFeedback({ kind: "error", message: TOOLBOX_SELECTION_MESSAGES.nativeOnly });
      return;
    }
    if (!onSendSelection) {
      setToolboxFeedback({ kind: "error", message: TOOLBOX_SELECTION_MESSAGES.error });
      return;
    }

    const actionRevision = revision;
    setSendingSelection(true);
    try {
      const result = await onSendSelection(check.text);
      if (renderRevisionRef.current !== actionRevision) return;
      setToolboxFeedback({
        kind: "success",
        message: result.redacted
          ? TOOLBOX_SELECTION_MESSAGES.redacted
          : TOOLBOX_SELECTION_MESSAGES.success,
      });
    } catch {
      if (renderRevisionRef.current === actionRevision) {
        setToolboxFeedback({ kind: "error", message: TOOLBOX_SELECTION_MESSAGES.error });
      }
    } finally {
      if (renderRevisionRef.current === actionRevision) setSendingSelection(false);
    }
  };

  const onTabKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    let nextIndex: number | null = null;
    if (event.key === "ArrowRight") nextIndex = (index + 1) % RESPONSE_TABS.length;
    if (event.key === "ArrowLeft") nextIndex = (index - 1 + RESPONSE_TABS.length) % RESPONSE_TABS.length;
    if (event.key === "Home") nextIndex = 0;
    if (event.key === "End") nextIndex = RESPONSE_TABS.length - 1;
    if (nextIndex === null) return;
    event.preventDefault();
    const nextTab = RESPONSE_TABS[nextIndex];
    setTab(nextTab);
    document.getElementById(`response-tab-${nextTab}`)?.focus();
  };

  if (!response) {
    return (
      <div className="response">
        <div className="response-head">
          <span className="dim">Send a request to see the response</span>
        </div>
        <pre className="resp-body"> </pre>
      </div>
    );
  }

  const rawUnavailableTitle = response.headers_truncated
    ? "응답 header 상한을 넘어 원문 복사를 사용할 수 없습니다"
    : "원문 복사는 데스크톱 앱에서만 사용할 수 있습니다";
  const canCopyRawHeaders = response.raw_headers_available
    && Boolean(response.response_id)
    && response.headers.length > 0;
  const canCopyRawCookies = response.raw_headers_available
    && Boolean(response.response_id)
    && response.cookies.length > 0;

  return (
    <div className="response">
      <div className="response-head">
        <span className={`status-badge ${statusClass(response.status)}`}>
          {response.status} {response.status_text}
        </span>
        <span className="dim">{response.duration_ms}ms</span>
        <span className="dim">{(response.size_bytes / 1024).toFixed(2)} KB</span>
      </div>
      <div className="response-tabs" role="tablist" aria-label="Response view">
        {RESPONSE_TABS.map((candidate, index) => {
          const label = candidate === "body"
            ? "Body"
            : candidate === "headers"
              ? `Headers (${response.headers.length})`
              : `Cookies (${response.cookies.length})`;
          return (
            <button
              key={candidate}
              type="button"
              role="tab"
              id={`response-tab-${candidate}`}
              aria-selected={tab === candidate}
              aria-controls={`response-panel-${candidate}`}
              tabIndex={tab === candidate ? 0 : -1}
              className={`response-tab ${tab === candidate ? "active" : ""}`}
              onClick={() => setTab(candidate)}
              onKeyDown={(event) => onTabKeyDown(event, index)}
            >
              {label}
            </button>
          );
        })}
      </div>

      {tab === "body" && (
        <section
          id="response-panel-body"
          role="tabpanel"
          aria-labelledby="response-tab-body"
          className="response-panel body-panel"
        >
          <div className="response-actions">
            {response.is_json && !response.binary && (
              <label className="toggle">
                <input
                  type="checkbox"
                  checked={pretty}
                  onChange={(event) => onPrettyChange(event.currentTarget.checked)}
                />
                pretty
              </label>
            )}
            <span className="spacer" />
            <button
              type="button"
              className="btn"
              disabled={Boolean(response.binary) || sendingSelection}
              title={response.binary
                ? "Binary 응답은 선택 영역을 Developer Toolbox로 보낼 수 없습니다"
                : "현재 마스킹된 응답 본문에서 선택한 텍스트를 Developer Toolbox로 보내기"}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => void sendSelection()}
            >
              {sendingSelection ? "Sending..." : "Send selection to Developer Toolbox"}
            </button>
            <button
              type="button"
              className="btn"
              disabled={Boolean(response.binary)}
              title={response.binary ? "Binary 응답은 bounded preview와 명시적 저장만 지원합니다" : "마스킹된 응답 본문 복사"}
              onClick={() => void copyMasked(responseText, "마스킹된 응답 본문을 복사하지 못했습니다.")}
            >
              Copy body
            </button>
          </div>
          {toolboxFeedback && (
            <div
              className={`response-feedback ${toolboxFeedback.kind}`}
              role="status"
              aria-live={toolboxFeedback.kind === "error" ? "assertive" : "polite"}
              aria-atomic="true"
            >
              {toolboxFeedback.message}
            </div>
          )}
          {response.binary && (
            <BinaryResponseSummary
              response={response}
              binary={response.binary}
              saving={savingBinary}
              onSave={() => void saveBinary()}
            />
          )}
          {response.graphql && !response.binary && <GraphqlResponseSummary response={response} graphql={response.graphql} />}
          {!response.binary && (
            <pre
              ref={responseBodyRef}
              className="resp-body"
              data-testid="response-body"
              onMouseUp={() => {
                const check = inspectResponseSelection(responseBodyRef.current, window.getSelection());
                if (check.kind === "valid" && responseBodyRef.current) {
                  selectionRef.current = {
                    body: responseBodyRef.current,
                    text: check.text,
                    revision: renderRevisionRef.current,
                  };
                  lastSelectionRevisionRef.current = renderRevisionRef.current;
                } else {
                  selectionRef.current = null;
                }
              }}
              onKeyUp={() => {
                const check = inspectResponseSelection(responseBodyRef.current, window.getSelection());
                if (check.kind === "valid" && responseBodyRef.current) {
                  selectionRef.current = {
                    body: responseBodyRef.current,
                    text: check.text,
                    revision: renderRevisionRef.current,
                  };
                  lastSelectionRevisionRef.current = renderRevisionRef.current;
                } else {
                  selectionRef.current = null;
                }
              }}
            >
              {responseText || " "}
            </pre>
          )}
        </section>
      )}

      {tab === "headers" && (
        <section
          id="response-panel-headers"
          role="tabpanel"
          aria-labelledby="response-tab-headers"
          className="response-panel"
        >
          <div className="response-actions">
            <span className="dim">Sensitive values are masked by default.</span>
            <span className="spacer" />
            <button
              type="button"
              className="btn"
              disabled={response.headers.length === 0}
              onClick={() => void copyMasked(
                formatResponseHeaders(response),
                "마스킹된 응답 header를 복사하지 못했습니다.",
              )}
            >
              Copy masked headers
            </button>
            <button
              type="button"
              className="btn danger-outline"
              disabled={!canCopyRawHeaders || copyingRaw !== null}
              title={canCopyRawHeaders ? "확인 후 현재 응답 원문을 한 번 복사" : rawUnavailableTitle}
              onClick={() => void copyRaw("headers")}
            >
              {copyingRaw === "headers" ? "Copying..." : "Copy original headers"}
            </button>
          </div>
          {response.headers_truncated && (
            <div className="response-warning">
              Header display was bounded to 100 rows / 64 KiB. Original copy is disabled.
            </div>
          )}
          {response.headers.length > 0 ? (
            <div className="response-table-wrap">
              <table className="response-table">
                <thead><tr><th>NAME</th><th>VALUE</th></tr></thead>
                <tbody>
                  {response.headers.map((header, index) => (
                    <tr key={`${header.key}-${index}`}>
                      <td>{header.key}</td>
                      <td className="mono">{header.value}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <div className="response-empty">No response headers.</div>
          )}
        </section>
      )}

      {tab === "cookies" && (
        <section
          id="response-panel-cookies"
          role="tabpanel"
          aria-labelledby="response-tab-cookies"
          className="response-panel"
        >
          <div className="response-actions">
            <span className="dim">Set-Cookie values are always masked in the viewer.</span>
            <span className="spacer" />
            <button
              type="button"
              className="btn"
              disabled={response.cookies.length === 0}
              onClick={() => void copyMasked(
                formatResponseCookies(response.cookies),
                "마스킹된 응답 Cookie를 복사하지 못했습니다.",
              )}
            >
              Copy masked cookies
            </button>
            <button
              type="button"
              className="btn danger-outline"
              disabled={!canCopyRawCookies || copyingRaw !== null}
              title={canCopyRawCookies ? "확인 후 현재 Set-Cookie 원문을 한 번 복사" : rawUnavailableTitle}
              onClick={() => void copyRaw("cookies")}
            >
              {copyingRaw === "cookies" ? "Copying..." : "Copy original cookies"}
            </button>
          </div>
          {response.cookies.length > 0 ? (
            <div className="response-table-wrap">
              <table className="response-table">
                <thead><tr><th>NAME</th><th>VALUE</th><th>ATTRIBUTES</th></tr></thead>
                <tbody>
                  {response.cookies.map((cookie, index) => (
                    <tr key={`${cookie.name}-${index}`}>
                      <td>{cookie.name}</td>
                      <td className="mono">{cookie.value}</td>
                      <td className="mono">
                        {cookie.attributes
                          .map((attribute) => attribute.value
                            ? `${attribute.key}=${attribute.value}`
                            : attribute.key)
                          .join("; ") || "—"}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <div className="response-empty">
              No visible Set-Cookie headers. Browser preview cannot expose Set-Cookie values.
            </div>
          )}
        </section>
      )}
    </div>
  );
}

export function statusClass(status: number) {
  if (status >= 200 && status < 300) return "status-2xx";
  if (status >= 400) return "status-4xx";
  return "status-other";
}
