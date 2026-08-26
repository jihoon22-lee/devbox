import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  clearHistory,
  copyHistoryHeaders,
  copyMaskedHistory,
  copyRawHistory,
  deleteHistory,
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
import { buildHistoryContextMenu, buildRuleContextMenu } from "./lib/contextMenus";
import { buildExampleCurl, type CurlShell } from "./lib/exampleCurl";
import "./App.css";

const DEFAULT_PORT = 9000;
const GENERIC_ERROR_MESSAGE = "요청을 처리하지 못했습니다. 입력과 서버 상태를 확인하세요.";
const SAFE_ERROR_MESSAGES = new Set([
  "요청 기록을 찾을 수 없습니다",
  "규칙을 찾을 수 없습니다",
  "원본 요청 복사는 데스크톱 앱에서만 사용할 수 있습니다",
]);

function emptyRule(): ResponseRule {
  return {
    id: "",
    method: "POST",
    path: "/hook",
    status: 200,
    headers: [],
    body: "",
    delayMs: 0,
  };
}

function safeMessage(error: unknown): string {
  const message = error instanceof Error
    ? error.message
    : typeof error === "string" ? error : "";
  return SAFE_ERROR_MESSAGES.has(message) ? message : GENERIC_ERROR_MESSAGE;
}

export default function App() {
  const [status, setStatus] = useState<ServerStatus>({ running: false, address: null });
  const [port, setPort] = useState(DEFAULT_PORT);
  const [lanBind, setLanBind] = useState(false);
  const [history, setHistory] = useState<RequestRecord[]>([]);
  const [rules, setRules] = useState<ResponseRule[]>([]);
  const [rule, setRuleDraft] = useState<ResponseRule>(emptyRule);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [selectedHistoryId, setSelectedHistoryId] = useState<number | null>(null);
  const [selectedRuleId, setSelectedRuleId] = useState<string | null>(null);
  const [contextHistory, setContextHistory] = useState<RequestRecord | null>(null);
  const [contextRule, setContextRule] = useState<ResponseRule | null>(null);
  const operationInFlight = useRef(false);

  const beginBusy = () => {
    if (operationInFlight.current) return false;
    operationInFlight.current = true;
    setBusy(true);
    return true;
  };

  const endBusy = () => {
    operationInFlight.current = false;
    setBusy(false);
  };

  const prepareHistoryContext = useCallback((target: HTMLElement) => {
    const id = Number(target.dataset.historyId);
    const request = history.find((candidate) => candidate.id === id);
    if (!request) return;
    setSelectedHistoryId(request.id);
    setContextHistory(request);
  }, [history]);
  const historyContextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => prepareHistoryContext(target),
  });

  const prepareRuleContext = useCallback((target: HTMLElement) => {
    const id = target.dataset.ruleId;
    const targetRule = rules.find((candidate) => candidate.id === id);
    if (!targetRule) return;
    setSelectedRuleId(targetRule.id);
    setContextRule(targetRule);
  }, [rules]);
  const ruleContextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => prepareRuleContext(target),
  });

  const refresh = useCallback(async () => {
    try {
      const [st, h, r] = await Promise.all([serverStatus(), listHistory(), listRules()]);
      setStatus(st);
      setHistory(h);
      setRules(r);
    } catch (e) {
      setError(safeMessage(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    const id = contextHistory?.id;
    if (id === undefined) return;
    const current = history.find((request) => request.id === id) ?? null;
    if (current) setContextHistory(current);
    else {
      historyContextMenu.close();
      setContextHistory(null);
      setSelectedHistoryId((selected) => selected === id ? null : selected);
    }
  }, [contextHistory?.id, history, historyContextMenu.close]);

  useEffect(() => {
    const id = contextRule?.id;
    if (!id) return;
    const current = rules.find((candidate) => candidate.id === id) ?? null;
    if (current) setContextRule(current);
    else {
      ruleContextMenu.close();
      setContextRule(null);
      setSelectedRuleId((selected) => selected === id ? null : selected);
    }
  }, [contextRule?.id, ruleContextMenu.close, rules]);

  const onStart = async () => {
    if (!beginBusy()) return;
    setError(null);
    try {
      const bind = lanBind ? "0.0.0.0" : "127.0.0.1";
      setStatus(await startServer(bind, port));
      await refresh();
    } catch (e) {
      setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onStop = async () => {
    if (!beginBusy()) return;
    setError(null);
    try {
      setStatus(await stopServer());
    } catch (e) {
      setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onSaveRule = async () => {
    if (!beginBusy()) return;
    setError(null);
    try {
      await setRule(rule);
      setRuleDraft(emptyRule());
      await refresh();
    } catch (e) {
      setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const copyHistoryText = async (
    load: () => Promise<string>,
    failureMessage: string,
  ) => {
    if (!beginBusy()) return;
    setError(null);
    try {
      const text = await load();
      await navigator.clipboard.writeText(text);
    } catch {
      setError(failureMessage);
    } finally {
      endBusy();
    }
  };

  const copyExampleCurl = async (ruleId: string, shell: CurlShell) => {
    if (!beginBusy()) return;
    setError(null);
    try {
      // A menu can remain open while another actor stops the server or removes the
      // rule. Re-read both sources before generating or copying anything.
      const [freshStatus, freshRules] = await Promise.all([serverStatus(), listRules()]);
      setStatus(freshStatus);
      setRules(freshRules);

      if (!freshStatus.running || !freshStatus.address) {
        setError("현재 서버가 실행 중이 아니거나 주소가 유효하지 않아 예시 curl을 만들지 못했습니다.");
        return;
      }

      const freshRule = freshRules.find((candidate) => candidate.id === ruleId);
      if (!freshRule) {
        setContextRule(null);
        setSelectedRuleId((selected) => selected === ruleId ? null : selected);
        setError("선택한 규칙이 더 이상 존재하지 않습니다.");
        return;
      }

      const curl = buildExampleCurl(freshRule, freshStatus.address, shell);
      if (!curl) {
        setError("현재 서버 주소 또는 규칙 입력을 확인할 수 없어 예시 curl을 만들지 못했습니다.");
        return;
      }
      try {
        await navigator.clipboard.writeText(curl);
      } catch {
        setError("예시 curl을 복사하지 못했습니다.");
      }
    } catch (e) {
      setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onDeleteHistory = async (request: RequestRecord) => {
    if (!window.confirm(`'${request.method} ${request.url}' 요청 기록을 삭제할까요?`)) return;
    if (!beginBusy()) return;
    setError(null);
    try {
      await deleteHistory(request.id);
      setSelectedHistoryId(null);
      await refresh();
    } catch (e) {
      setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onClearHistory = async () => {
    if (!window.confirm("수신 요청 기록을 모두 삭제할까요? 이 작업은 되돌릴 수 없습니다.")) return;
    if (!beginBusy()) return;
    setError(null);
    try {
      await clearHistory();
      setSelectedHistoryId(null);
      await refresh();
    } catch (e) {
      setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onDeleteRule = async (targetRule: ResponseRule) => {
    if (!window.confirm(`'${targetRule.method ?? "*"} ${targetRule.path}' 규칙을 삭제할까요?`)) return;
    if (!beginBusy()) return;
    setError(null);
    try {
      await deleteRule(targetRule.id);
      setSelectedRuleId(null);
      if (rule.id === targetRule.id) setRuleDraft(emptyRule());
      await refresh();
    } catch (e) {
      setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onDuplicateRule = async (targetRule: ResponseRule) => {
    if (!beginBusy()) return;
    setError(null);
    try {
      await setRule({ ...targetRule, id: "" });
      await refresh();
    } catch (e) {
      setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const historyContextItems = useMemo<readonly ContextMenuEntry[]>(
    () => buildHistoryContextMenu(busy),
    [busy],
  );
  const ruleContextItems = useMemo<readonly ContextMenuEntry[]>(
    () => buildRuleContextMenu(busy, status.running && Boolean(status.address)),
    [busy, status.address, status.running],
  );

  const onHistoryContextSelect = (id: string) => {
    const request = contextHistory;
    if (!request) return;
    if (id === "copy-masked") {
      void copyHistoryText(
        () => copyMaskedHistory(request.id),
        "마스킹된 요청을 복사하지 못했습니다.",
      );
    } else if (id === "copy-raw") {
      const confirmed = window.confirm(
        "원본 요청에는 Authorization, Cookie, API key 같은 민감정보가 포함될 수 있습니다. 클립보드에 한 번 복사할까요?",
      );
      if (confirmed) {
        void copyHistoryText(
          () => copyRawHistory(request.id),
          "원본 요청을 안전하게 만들거나 복사하지 못했습니다.",
        );
      }
    } else if (id === "copy-headers") {
      void copyHistoryText(
        () => copyHistoryHeaders(request.id),
        "마스킹된 헤더를 복사하지 못했습니다.",
      );
    } else if (id === "delete") {
      void onDeleteHistory(request);
    }
  };

  const onRuleContextSelect = (id: string) => {
    const targetRule = contextRule;
    if (!targetRule) return;
    if (id === "copy-example-curl-powershell") {
      void copyExampleCurl(targetRule.id, "powershell");
      return;
    }
    if (id === "copy-example-curl-posix") {
      void copyExampleCurl(targetRule.id, "posix");
      return;
    }

    const currentRule = rules.find((candidate) => candidate.id === targetRule.id);
    if (!currentRule) {
      setContextRule(null);
      setSelectedRuleId((selected) => selected === targetRule.id ? null : selected);
      setError("선택한 규칙이 더 이상 존재하지 않습니다.");
      return;
    }
    if (id === "edit") setRuleDraft({ ...currentRule, headers: [...currentRule.headers] });
    else if (id === "duplicate") void onDuplicateRule(currentRule);
    else if (id === "delete") void onDeleteRule(currentRule);
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
              <input type="number" value={port} min={1} max={65535} disabled={busy} onChange={(e) => setPort(Number(e.currentTarget.value))} />
            </label>
            <label className="toggle">
              <input type="checkbox" checked={lanBind} disabled={busy} onChange={(e) => setLanBind(e.currentTarget.checked)} />
              LAN 공개 (위험)
            </label>
            <button className="btn primary" disabled={busy} onClick={() => void onStart()}>시작</button>
          </>
        ) : (
          <button className="btn danger" disabled={busy} onClick={() => void onStop()}>중지</button>
        )}
      </header>

      {lanBind && <div className="warn">LAN 공개는 명시적 설정입니다. 외부에서 접근 가능합니다.</div>}
      {error && <div className="error" role="alert" aria-live="assertive">{error}</div>}

      <div className="main">
        <section className="panel">
          <h2>Rules</h2>
          <div className="rule-editor">
            <input placeholder="method (없으면 전체)" value={rule.method ?? ""} onChange={(e) => setRuleDraft({ ...rule, method: e.currentTarget.value || null })} />
            <input placeholder="path (예: /hook 또는 /events/*)" value={rule.path} onChange={(e) => setRuleDraft({ ...rule, path: e.currentTarget.value })} />
            <input type="number" placeholder="status" value={rule.status} onChange={(e) => setRuleDraft({ ...rule, status: Number(e.currentTarget.value) })} />
            <input type="number" placeholder="delay ms" value={rule.delayMs} onChange={(e) => setRuleDraft({ ...rule, delayMs: Number(e.currentTarget.value) })} />
            <textarea placeholder="응답 body" value={rule.body} onChange={(e) => setRuleDraft({ ...rule, body: e.currentTarget.value })} />
            <button className="btn primary" disabled={busy} onClick={() => void onSaveRule()}>{rule.id ? "규칙 저장" : "규칙 추가"}</button>
          </div>
          {rules.map((targetRule) => (
            <div
              key={targetRule.id}
              className={`rule-row ${selectedRuleId === targetRule.id ? "selected" : ""}`}
              tabIndex={0}
              aria-current={selectedRuleId === targetRule.id ? "true" : undefined}
              aria-label={`${targetRule.method ?? "*"} ${targetRule.path} 규칙`}
              data-rule-id={targetRule.id}
              onClick={() => setSelectedRuleId(targetRule.id)}
              {...ruleContextMenu.triggerProps}
            >
              <span className="mono">{targetRule.method ?? "*"} {targetRule.path} → {targetRule.status}{targetRule.delayMs ? ` (+${targetRule.delayMs}ms)` : ""}</span>
              <button className="mini" disabled={busy} onClick={() => void onDeleteRule(targetRule)}>✕</button>
            </div>
          ))}
          {rules.length === 0 && <div className="dim">규칙 없음 — 매치 없으면 404.</div>}
        </section>

        <section className="panel">
          <h2>History ({history.length})</h2>
          <div className="history-head">
            <button className="mini" disabled={busy || history.length === 0} onClick={() => void onClearHistory()}>비우기</button>
          </div>
          {history.map((request) => (
            <div
              key={request.id}
              className={`request-row ${selectedHistoryId === request.id ? "selected" : ""}`}
              tabIndex={0}
              aria-current={selectedHistoryId === request.id ? "true" : undefined}
              aria-label={`${request.method} ${request.url} 요청`}
              data-history-id={request.id}
              onClick={() => setSelectedHistoryId(request.id)}
              {...historyContextMenu.triggerProps}
            >
              <span className={`method ${request.method.toLowerCase()}`}>{request.method}</span>
              <span className="url">{request.url}</span>
              <span className="dim">{new Date(request.receivedAtMs).toLocaleTimeString()}</span>
              {request.body && <pre className="body">{request.body.slice(0, 200)}</pre>}
              {request.headers.some(([, value]) => value === "•••••") && (
                <span className="masked">민감 헤더 마스킹됨</span>
              )}
            </div>
          ))}
          {history.length === 0 && <div className="dim">수신 요청이 없습니다.</div>}
        </section>
      </div>

      <ContextMenu
        open={historyContextMenu.open}
        anchor={historyContextMenu.anchor}
        restoreFocusTo={historyContextMenu.restoreFocusTo}
        items={historyContextItems}
        onSelect={onHistoryContextSelect}
        onClose={historyContextMenu.close}
        ariaLabel="수신 요청 메뉴"
      />
      <ContextMenu
        open={ruleContextMenu.open}
        anchor={ruleContextMenu.anchor}
        restoreFocusTo={ruleContextMenu.restoreFocusTo}
        items={ruleContextItems}
        onSelect={onRuleContextSelect}
        onClose={ruleContextMenu.close}
        ariaLabel="응답 규칙 메뉴"
      />
    </div>
  );
}
