import { useCallback, useEffect, useState } from "react";
import {
  clearHistory,
  deleteRule,
  listHistory,
  listRules,
  serverStatus,
  setRule,
  startServer,
  stopServer,
  type RequestRecord,
  type ResponseRule,
  type ServerStatus,
} from "./api";
import "./App.css";

const DEFAULT_PORT = 9000;

export default function App() {
  const [status, setStatus] = useState<ServerStatus>({ running: false, address: null });
  const [port, setPort] = useState(DEFAULT_PORT);
  const [lanBind, setLanBind] = useState(false);
  const [history, setHistory] = useState<RequestRecord[]>([]);
  const [rules, setRules] = useState<ResponseRule[]>([]);
  const [rule, setRuleDraft] = useState<ResponseRule>({ id: "", method: "POST", path: "/hook", status: 200, headers: [], body: "", delayMs: 0 });
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [st, h, r] = await Promise.all([serverStatus(), listHistory(), listRules()]);
      setStatus(st);
      setHistory(h);
      setRules(r);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const onStart = async () => {
    setError(null);
    try {
      const bind = lanBind ? "0.0.0.0" : "127.0.0.1";
      setStatus(await startServer(bind, port));
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onStop = async () => {
    setError(null);
    try {
      setStatus(await stopServer());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onSaveRule = async () => {
    setError(null);
    try {
      await setRule(rule);
      setRuleDraft({ id: "", method: "POST", path: "/hook", status: 200, headers: [], body: "", delayMs: 0 });
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">Webhook Lab</h1>
        <span className={`status ${status.running ? "ok" : "off"}`}>
          ● {status.running ? `듣는 중 ${status.address}` : "중지"}
        </span>
        <span className="spacer" />
        {!status.running ? (
          <>
            <label className="field-inline">
              포트
              <input type="number" value={port} min={1} max={65535} onChange={(e) => setPort(Number(e.currentTarget.value))} />
            </label>
            <label className="toggle">
              <input type="checkbox" checked={lanBind} onChange={(e) => setLanBind(e.currentTarget.checked)} />
              LAN 공개 (위험)
            </label>
            <button className="btn primary" onClick={() => void onStart()}>시작</button>
          </>
        ) : (
          <button className="btn danger" onClick={() => void onStop()}>중지</button>
        )}
      </header>

      {lanBind && <div className="warn">LAN 공개는 명시적 설정입니다. 외부에서 접근 가능합니다.</div>}
      {error && <div className="error">{error}</div>}

      <div className="main">
        <section className="panel">
          <h2>Rules</h2>
          <div className="rule-editor">
            <input placeholder="method (없으면 전체)" value={rule.method ?? ""} onChange={(e) => setRuleDraft({ ...rule, method: e.currentTarget.value || null })} />
            <input placeholder="path (예: /hook 또는 /events/*)" value={rule.path} onChange={(e) => setRuleDraft({ ...rule, path: e.currentTarget.value })} />
            <input type="number" placeholder="status" value={rule.status} onChange={(e) => setRuleDraft({ ...rule, status: Number(e.currentTarget.value) })} />
            <input type="number" placeholder="delay ms" value={rule.delayMs} onChange={(e) => setRuleDraft({ ...rule, delayMs: Number(e.currentTarget.value) })} />
            <textarea placeholder="응답 body" value={rule.body} onChange={(e) => setRuleDraft({ ...rule, body: e.currentTarget.value })} />
            <button className="btn primary" onClick={() => void onSaveRule()}>규칙 추가</button>
          </div>
          {rules.map((r) => (
            <div key={r.id} className="rule-row">
              <span className="mono">{r.method ?? "*"} {r.path} → {r.status}{r.delayMs ? ` (+${r.delayMs}ms)` : ""}</span>
              <button className="mini" onClick={() => void deleteRule(r.id).then(refresh)}>✕</button>
            </div>
          ))}
          {rules.length === 0 && <div className="dim">규칙 없음 — 매치 없으면 404.</div>}
        </section>

        <section className="panel">
          <h2>History ({history.length})</h2>
          <div className="history-head">
            <button className="mini" onClick={() => void clearHistory().then(refresh)}>비우기</button>
          </div>
          {history.map((h) => (
            <div key={h.id} className="request-row">
              <span className={`method ${h.method.toLowerCase()}`}>{h.method}</span>
              <span className="url">{h.url}</span>
              <span className="dim">{new Date(h.receivedAtMs).toLocaleTimeString()}</span>
              {h.body && <pre className="body">{h.body.slice(0, 200)}</pre>}
              {h.headers.filter(([, v]) => v === "•••••").length > 0 && (
                <span className="masked">민감 헤더 마스킹됨</span>
              )}
            </div>
          ))}
          {history.length === 0 && <div className="dim">수신 요청이 없습니다.</div>}
        </section>
      </div>
    </div>
  );
}
