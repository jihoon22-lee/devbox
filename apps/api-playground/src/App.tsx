import { useCallback, useEffect, useState } from "react";
import { sendRequest } from "./api";
import type { ApiRequest, ApiResponse, HistoryItem, KeyValue } from "./types";
import "./App.css";

const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE"];
const BODY_KINDS = ["none", "json", "form", "raw"];
const AUTH_KINDS = ["none", "basic", "bearer", "apikey"];

const emptyReq = (): ApiRequest => ({
  method: "GET",
  url: "",
  headers: [],
  params: [],
  body_kind: "none",
  body: "",
  auth: { kind: "none", username: "", password: "", token: "", api_key: "", api_value: "" },
  timeout_ms: 10000,
});

function KeyValueEditor({
  rows,
  onChange,
  namePlaceholder,
}: {
  rows: KeyValue[];
  onChange: (rows: KeyValue[]) => void;
  namePlaceholder: string;
}) {
  const update = (i: number, patch: Partial<KeyValue>) => {
    onChange(rows.map((r, idx) => (idx === i ? { ...r, ...patch } : r)));
  };
  return (
    <div className="kv-editor">
      {rows.map((r, i) => (
        <div className="kv-row" key={i}>
          <input
            placeholder={namePlaceholder}
            value={r.key}
            onChange={(e) => update(i, { key: e.currentTarget.value })}
            spellCheck={false}
          />
          <input
            placeholder="Value"
            value={r.value}
            onChange={(e) => update(i, { value: e.currentTarget.value })}
            spellCheck={false}
          />
          <button className="kv-del" onClick={() => onChange(rows.filter((_, idx) => idx !== i))}>
            ✕
          </button>
        </div>
      ))}
      <button className="btn kv-add" onClick={() => onChange([...rows, { key: "", value: "" }])}>
        + Add
      </button>
    </div>
  );
}

function statusClass(status: number) {
  if (status >= 200 && status < 300) return "status-2xx";
  if (status >= 400) return "status-4xx";
  return "status-other";
}

export default function App() {
  const [req, setReq] = useState<ApiRequest>(emptyReq);
  const [resp, setResp] = useState<ApiResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [tab, setTab] = useState<"params" | "headers" | "body" | "auth">("params");
  const [showHeaders, setShowHeaders] = useState(false);
  const [pretty, setPretty] = useState(true);
  const [history, setHistory] = useState<HistoryItem[]>([]);

  const loadHistory = useCallback(() => {
    try {
      setHistory(JSON.parse(localStorage.getItem("apip-history") ?? "[]"));
    } catch {
      setHistory([]);
    }
  }, []);

  useEffect(loadHistory, [loadHistory]);

  const onSend = async () => {
    setSending(true);
    setError(null);
    try {
      const result = await sendRequest(req);
      setResp(result);
      const item: HistoryItem = { id: String(Date.now()), saved_at: Date.now(), request: req, status: result.status };
      const next = [item, ...history].slice(0, 50);
      setHistory(next);
      localStorage.setItem("apip-history", JSON.stringify(next));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setResp(null);
    } finally {
      setSending(false);
    }
  };

  const setAuth = (patch: Partial<NonNullable<ApiRequest["auth"]>>) =>
    setReq({
      ...req,
      auth: {
        kind: "none",
        username: "",
        password: "",
        token: "",
        api_key: "",
        api_value: "",
        ...req.auth,
        ...patch,
      },
    });

  const responseText = resp?.is_json && pretty ? tryPretty(resp.body) : resp?.body ?? "";

  return (
    <div className="app">
      <aside className="sidebar">
        <h1 className="app-title">API Playground</h1>
        <div className="group-name">History</div>
        {history.map((h) => (
          <button
            key={h.id}
            className="history-item"
            onClick={() => {
              setReq(h.request);
              setResp(null);
            }}
          >
            <span className={`method ${h.request.method.toLowerCase()}`}>{h.request.method}</span>
            <span className="history-url" title={h.request.url}>
              {h.request.url || "(no url)"}
            </span>
          </button>
        ))}
        {history.length === 0 && <div className="dim">No requests yet</div>}
      </aside>

      <main className="content">
        <div className="request-bar">
          <select className="method-select" value={req.method} onChange={(e) => setReq({ ...req, method: e.currentTarget.value })}>
            {METHODS.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
          <input
            className="url-input"
            placeholder="https://api.example.com/users"
            value={req.url}
            onChange={(e) => setReq({ ...req, url: e.currentTarget.value })}
            spellCheck={false}
          />
          <button className="btn send" onClick={() => void onSend()} disabled={sending || !req.url}>
            {sending ? "Sending..." : "Send"}
          </button>
        </div>

        <div className="tabs">
          {(["params", "headers", "body", "auth"] as const).map((t) => (
            <button key={t} className={`tab ${tab === t ? "active" : ""}`} onClick={() => setTab(t)}>
              {t.toUpperCase()}
            </button>
          ))}
        </div>

        <div className="tab-body">
          {tab === "params" && (
            <KeyValueEditor rows={req.params} onChange={(params) => setReq({ ...req, params })} namePlaceholder="Key" />
          )}
          {tab === "headers" && (
            <KeyValueEditor rows={req.headers} onChange={(headers) => setReq({ ...req, headers })} namePlaceholder="Header" />
          )}
          {tab === "body" && (
            <div>
              <select className="select-sm" value={req.body_kind} onChange={(e) => setReq({ ...req, body_kind: e.currentTarget.value })}>
                {BODY_KINDS.map((k) => (
                  <option key={k} value={k}>
                    {k}
                  </option>
                ))}
              </select>
              {req.body_kind !== "none" && (
                <textarea
                  className="body-input"
                  rows={8}
                  placeholder={req.body_kind === "json" ? '{ "key": "value" }' : "key=value"}
                  value={req.body}
                  onChange={(e) => setReq({ ...req, body: e.currentTarget.value })}
                  spellCheck={false}
                />
              )}
            </div>
          )}
          {tab === "auth" && (
            <div className="auth-body">
              <select className="select-sm" value={req.auth?.kind ?? "none"} onChange={(e) => setAuth({ kind: e.currentTarget.value })}>
                {AUTH_KINDS.map((k) => (
                  <option key={k} value={k}>
                    {k}
                  </option>
                ))}
              </select>
              {req.auth?.kind === "basic" && (
                <div className="kv-row">
                  <input placeholder="Username" value={req.auth.username} onChange={(e) => setAuth({ username: e.currentTarget.value })} />
                  <input placeholder="Password" type="password" value={req.auth.password} onChange={(e) => setAuth({ password: e.currentTarget.value })} />
                </div>
              )}
              {req.auth?.kind === "bearer" && (
                <div className="kv-row">
                  <input placeholder="Token" value={req.auth.token} onChange={(e) => setAuth({ token: e.currentTarget.value })} />
                </div>
              )}
              {req.auth?.kind === "apikey" && (
                <div className="kv-row">
                  <input placeholder="Header name (e.g. X-API-Key)" value={req.auth.api_key} onChange={(e) => setAuth({ api_key: e.currentTarget.value })} />
                  <input placeholder="Value" value={req.auth.api_value} onChange={(e) => setAuth({ api_value: e.currentTarget.value })} />
                </div>
              )}
            </div>
          )}
        </div>

        {error && <div className="error">{error}</div>}

        <div className="response">
          <div className="response-head">
            {resp ? (
              <>
                <span className={`status-badge ${statusClass(resp.status)}`}>
                  {resp.status} {resp.status_text}
                </span>
                <span className="dim">{resp.duration_ms}ms</span>
                <span className="dim">{(resp.size_bytes / 1024).toFixed(2)} KB</span>
                <span className="spacer" />
                {resp.is_json && (
                  <label className="toggle">
                    <input type="checkbox" checked={pretty} onChange={(e) => setPretty(e.currentTarget.checked)} />
                    pretty
                  </label>
                )}
                <button className="btn" onClick={() => void navigator.clipboard.writeText(responseText)}>
                  Copy
                </button>
                <button className="btn" onClick={() => setShowHeaders((v) => !v)}>
                  {showHeaders ? "Hide" : "Show"} headers
                </button>
              </>
            ) : (
              <span className="dim">Send a request to see the response</span>
            )}
          </div>
          {showHeaders && resp && (
            <pre className="resp-headers">
              {resp.headers.map((h) => `${h.key}: ${h.value}`).join("\n")}
            </pre>
          )}
          <pre className="resp-body">{responseText || " "}</pre>
        </div>
      </main>
    </div>
  );
}

function tryPretty(json: string): string {
  try {
    return JSON.stringify(JSON.parse(json), null, 2);
  } catch {
    return json;
  }
}
