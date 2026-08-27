import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  clearFixtures,
  clearHistory,
  deleteFixture,
  copyHistoryHeaders,
  copyMaskedHistory,
  copyRawHistory,
  deleteHistory,
  deleteRule,
  fixtureToRule,
  listFixtures,
  listHistory,
  listRules,
  saveFixture,
  sendFixtureToApi,
  sendHistoryToApi,
  serverStatus,
  setRule,
  startServer,
  stopServer,
  type RequestRecord,
  type ResponseRule,
  type ServerStatus,
  type CapturedFixture,
  type ApiHandoffDispatch,
} from "./api";
import { buildHistoryContextMenu, buildRuleContextMenu } from "./lib/contextMenus";
import { buildExampleCurl, type CurlShell } from "./lib/exampleCurl";
import {
  MAX_METHOD_CHARS,
  MAX_RESPONSE_DELAY_MS,
  MAX_RESPONSE_STATUS,
  MIN_RESPONSE_STATUS,
  validateRule,
  validateRuleCollection,
  type RuleValidationField,
} from "./lib/ruleValidation";
import "./App.css";

const DEFAULT_PORT = 9000;
const GENERIC_ERROR_MESSAGE = "요청을 처리하지 못했습니다. 입력과 서버 상태를 확인하세요.";
const STALE_RULE_MESSAGE = "선택한 규칙이 더 이상 존재하지 않습니다. 목록을 새로 고친 뒤 다시 시도하세요.";
const SAFE_ERROR_MESSAGES = new Set([
  "요청 기록을 찾을 수 없습니다",
  "규칙을 찾을 수 없습니다",
  "규칙 입력이 유효하지 않습니다",
  "원본 요청 복사는 데스크톱 앱에서만 사용할 수 있습니다",
  "fixture 저장소를 읽을 수 없습니다",
  "fixture 저장소를 저장할 수 없습니다",
  "fixture 저장소 크기 제한을 초과했습니다",
  "fixture 저장소가 다른 작업으로 변경되었습니다. 다시 시도하세요",
  "fixture를 찾을 수 없습니다",
  "fixture 입력이 유효하지 않습니다",
  "LAN 공개를 시작하려면 명시적인 확인이 필요합니다",
  "허용되지 않은 bind 주소입니다",
  "포트는 1~65535 범위여야 합니다",
  "서버 bind에 실패했습니다",
  "API Playground를 사용할 수 없습니다. 설치 또는 업데이트 후 다시 시도하세요. 클립보드로 자동 전환하지 않습니다",
  "API Playground를 실행하지 못했습니다. handoff는 잠시 보관되며 다시 시도할 수 있습니다. 클립보드로 자동 전환하지 않습니다",
  "API Playground handoff를 만들지 못했습니다. 클립보드로 자동 전환하지 않습니다",
  "handoff 요청에 사용할 fixture가 유효하지 않습니다",
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

function formatFixtureTime(receivedAtMs: number): string {
  const date = new Date(receivedAtMs);
  return Number.isFinite(date.getTime()) ? date.toISOString() : "시간 미상";
}

export default function App() {
  const [status, setStatus] = useState<ServerStatus>({ running: false, address: null });
  const [port, setPort] = useState(DEFAULT_PORT);
  const [lanBind, setLanBind] = useState(false);
  const [history, setHistory] = useState<RequestRecord[]>([]);
  const [fixtures, setFixtures] = useState<CapturedFixture[]>([]);
  const [rules, setRules] = useState<ResponseRule[]>([]);
  const [rule, setRuleDraft] = useState<ResponseRule>(emptyRule);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [selectedHistoryId, setSelectedHistoryId] = useState<number | null>(null);
  const [selectedRuleId, setSelectedRuleId] = useState<string | null>(null);
  const [contextHistory, setContextHistory] = useState<RequestRecord | null>(null);
  const [contextRule, setContextRule] = useState<ResponseRule | null>(null);
  const [handoffNotice, setHandoffNotice] = useState<string | null>(null);
  const operationInFlight = useRef(false);
  const refreshRequest = useRef(0);

  const beginBusy = useCallback(() => {
    if (operationInFlight.current) return false;
    operationInFlight.current = true;
    setBusy(true);
    return true;
  }, []);

  const endBusy = useCallback(() => {
    operationInFlight.current = false;
    setBusy(false);
  }, []);

  const prepareHistoryContext = useCallback((target: HTMLElement) => {
    const id = Number(target.dataset.historyId);
    const request = history.find((candidate) => candidate.id === id);
    if (!request) return;
    setSelectedHistoryId(request.id);
    setContextHistory(request);
  }, [history]);
  const historyContextMenu = useContextMenu({
    disabled: busy,
    onBeforeOpen: (_reason, target) => prepareHistoryContext(target),
  });

  const prepareRuleContext = useCallback((target: HTMLElement) => {
    const id = target.dataset.ruleId;
    const targetRule = rules.find((candidate) => candidate.id === id);
    if (!targetRule) {
      setContextRule(null);
      setSelectedRuleId(null);
      setError(STALE_RULE_MESSAGE);
      return;
    }
    setSelectedRuleId(targetRule.id);
    setContextRule(targetRule);
  }, [rules]);
  const ruleContextMenu = useContextMenu({
    disabled: busy,
    onBeforeOpen: (_reason, target) => prepareRuleContext(target),
  });

  const refresh = useCallback(async () => {
    const request = refreshRequest.current + 1;
    refreshRequest.current = request;
    const [statusResult, historyResult, rulesResult, fixtureResult] = await Promise.allSettled([
      serverStatus(),
      listHistory(),
      listRules(),
      listFixtures(),
    ]);
    if (refreshRequest.current !== request) return;
    if (statusResult.status === "fulfilled") setStatus(statusResult.value);
    if (historyResult.status === "fulfilled") setHistory(historyResult.value);
    if (rulesResult.status === "fulfilled") setRules(rulesResult.value);
    if (fixtureResult.status === "fulfilled") setFixtures(fixtureResult.value);
    else setFixtures([]);
    const failure = [statusResult, historyResult, rulesResult, fixtureResult]
      .find((result): result is PromiseRejectedResult => result.status === "rejected");
    if (failure) setError(safeMessage(failure.reason));
  }, []);

  useEffect(() => {
    void refresh();
    return () => {
      // Invalidate the mount refresh and any action-owned refresh before a
      // late promise can update a newer view or an unmounted app.
      refreshRequest.current += 1;
      operationInFlight.current = false;
    };
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
    if (lanBind && !window.confirm("LAN 공개는 외부 접근을 허용합니다. 이 컴퓨터의 다른 장치에서 요청을 받을까요?")) return;
    if (!beginBusy()) return;
    setError(null);
    try {
      const bind = lanBind ? "0.0.0.0" : "127.0.0.1";
      setStatus(await startServer(bind, port, lanBind));
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
      // Stop changes the source of truth. A refresh that started before the
      // stop must not restore the old running status or stale rule/history view.
      await refresh();
    } catch (e) {
      setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onSaveRule = async () => {
    const [validationIssue] = validateRule(rule);
    if (validationIssue) {
      setError(validationIssue.message);
      return;
    }

    if (rule.id && !rules.some((candidate) => candidate.id === rule.id)) {
      setError(STALE_RULE_MESSAGE);
      return;
    }

    const projectedRules = rule.id
      ? [...rules.filter((candidate) => candidate.id !== rule.id), rule]
      : [...rules, rule];
    const [collectionIssue] = validateRuleCollection(projectedRules);
    if (collectionIssue) {
      setError(collectionIssue.message);
      return;
    }

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
    const request = refreshRequest.current + 1;
    refreshRequest.current = request;
    try {
      // A menu can remain open while another actor stops the server or removes the
      // rule. Re-read both sources before generating or copying anything.
      const [freshStatus, freshRules] = await Promise.all([serverStatus(), listRules()]);
      if (refreshRequest.current !== request) return;
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
        setError(STALE_RULE_MESSAGE);
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

  const onSaveFixture = async (request: RequestRecord) => {
    if (!beginBusy()) return;
    setError(null);
    try {
      await saveFixture(request.id);
      await refresh();
    } catch (e) {
      setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const showHandoffSuccess = (dispatch: ApiHandoffDispatch) => {
    setHandoffNotice(
      `API Playground 미리보기로 전달했습니다. producer: ${dispatch.producerId} · consumer: ${dispatch.consumerId} · handoff: ${dispatch.handoffId}. 적용 전 내용을 확인하세요.`,
    );
  };

  const onSendHistoryToApi = async (request: RequestRecord) => {
    if (!beginBusy()) return;
    setError(null);
    setHandoffNotice(null);
    try {
      showHandoffSuccess(await sendHistoryToApi(request.id));
    } catch (e) {
      setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onSendFixtureToApi = async (fixture: CapturedFixture) => {
    if (!beginBusy()) return;
    setError(null);
    setHandoffNotice(null);
    try {
      showHandoffSuccess(await sendFixtureToApi(fixture.id));
    } catch (e) {
      setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onDraftFixture = async (fixture: CapturedFixture) => {
    if (!beginBusy()) return;
    setError(null);
    try {
      setRuleDraft(await fixtureToRule(fixture.id));
    } catch (e) {
      setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onDeleteFixture = async (fixture: CapturedFixture) => {
    if (!window.confirm("선택한 masked fixture를 삭제할까요?")) return;
    if (!beginBusy()) return;
    setError(null);
    try {
      await deleteFixture(fixture.id);
      await refresh();
    } catch (e) {
      setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onClearFixtures = async () => {
    if (!window.confirm("저장된 masked fixture를 모두 삭제할까요? 이 작업은 되돌릴 수 없습니다.")) return;
    if (!beginBusy()) return;
    setError(null);
    try {
      await clearFixtures();
      await refresh();
    } catch (e) {
      setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onDeleteRule = async (targetRule: ResponseRule) => {
    const currentRule = rules.find((candidate) => candidate.id === targetRule.id);
    if (!currentRule) {
      setError(STALE_RULE_MESSAGE);
      return;
    }
    if (!window.confirm(`'${currentRule.method ?? "*"} ${currentRule.path}' 규칙을 삭제할까요?`)) return;
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
    const currentRule = rules.find((candidate) => candidate.id === targetRule.id);
    if (!currentRule) {
      setError(STALE_RULE_MESSAGE);
      return;
    }

    const duplicate = { ...currentRule, id: "" };
    const [validationIssue] = validateRule(duplicate);
    if (validationIssue) {
      setError(validationIssue.message);
      return;
    }

    const [collectionIssue] = validateRuleCollection([...rules, duplicate]);
    if (collectionIssue) {
      setError(collectionIssue.message);
      return;
    }

    if (!beginBusy()) return;
    setError(null);
    try {
      await setRule(duplicate);
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

  const ruleIssues = validateRule(rule);
  const ruleIssueFor = (field: RuleValidationField) =>
    ruleIssues.find((issue) => issue.field === field);
  const methodIssue = ruleIssueFor("method");
  const pathIssue = ruleIssueFor("path");
  const statusIssue = ruleIssueFor("status");
  const headersIssue = ruleIssueFor("headers");
  const bodyIssue = ruleIssueFor("body");
  const delayIssue = ruleIssueFor("delayMs");

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
    } else if (id === "save-fixture") {
      void onSaveFixture(request);
    } else if (id === "convert-api-playground") {
      void onSendHistoryToApi(request);
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
      setError(STALE_RULE_MESSAGE);
      return;
    }
    if (id === "edit") setRuleDraft({ ...currentRule, headers: [...currentRule.headers] });
    else if (id === "duplicate") void onDuplicateRule(currentRule);
    else if (id === "delete") void onDeleteRule(currentRule);
  };

  return (
    <div className="app" aria-busy={busy}>
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
      {handoffNotice && <div className="handoff-notice" role="status" aria-live="polite">{handoffNotice}</div>}

      <div className="main">
        <section className="panel">
          <h2>Rules</h2>
          <div className="rule-editor">
            <div className="rule-field">
              <label htmlFor="rule-method">method</label>
              <input
                id="rule-method"
                placeholder="method (없으면 전체)"
                value={rule.method ?? ""}
                maxLength={MAX_METHOD_CHARS}
                aria-describedby={`rule-method-help${methodIssue ? " rule-method-error" : ""}`}
                aria-invalid={methodIssue ? "true" : undefined}
                onChange={(e) => setRuleDraft({ ...rule, method: e.currentTarget.value || null })}
              />
              <p id="rule-method-help" className="field-help">
                대소문자를 구분하지 않고 요청 method와 일치합니다. 비워두면 모든 method(*)에 적용됩니다. ASCII HTTP token, 최대 16자/16바이트입니다.
              </p>
              {methodIssue && <p id="rule-method-error" className="field-error">{methodIssue.message}</p>}
            </div>
            <div className="rule-field">
              <label htmlFor="rule-path">path</label>
              <input
                id="rule-path"
                placeholder="path (예: /hook 또는 /events/*)"
                value={rule.path}
                aria-describedby={`rule-path-help${pathIssue ? " rule-path-error" : ""}`}
                aria-invalid={pathIssue ? "true" : undefined}
                onChange={(e) => setRuleDraft({ ...rule, path: e.currentTarget.value })}
              />
              <p id="rule-path-help" className="field-help">
                경로 전체가 정확히 일치합니다. 마지막 문자가 *일 때만 그 앞부분으로 시작하는 경로와 일치합니다 (예: /events/* → /events/123). /로 시작하고 최대 4,096자/16,384바이트입니다.
              </p>
              {pathIssue && <p id="rule-path-error" className="field-error">{pathIssue.message}</p>}
            </div>
            <div className="rule-field">
              <label htmlFor="rule-status">status</label>
              <input
                id="rule-status"
                type="number"
                placeholder="status"
                value={rule.status}
                min={MIN_RESPONSE_STATUS}
                max={MAX_RESPONSE_STATUS}
                step={1}
                aria-describedby={`rule-status-help${statusIssue ? " rule-status-error" : ""}`}
                aria-invalid={statusIssue ? "true" : undefined}
                onChange={(e) => setRuleDraft({ ...rule, status: Number(e.currentTarget.value) })}
              />
              <p id="rule-status-help" className="field-help">
                매칭된 요청에 돌려줄 HTTP 응답 status 코드입니다 (허용 범위: 100~599, 예: 200, 404, 500).
              </p>
              {statusIssue && <p id="rule-status-error" className="field-error">{statusIssue.message}</p>}
            </div>
            <div className="rule-field">
              <label htmlFor="rule-delay">delay (ms)</label>
              <input
                id="rule-delay"
                type="number"
                placeholder="delay ms"
                value={rule.delayMs}
                min={0}
                max={MAX_RESPONSE_DELAY_MS}
                step={1}
                aria-describedby={`rule-delay-help${delayIssue ? " rule-delay-error" : ""}`}
                aria-invalid={delayIssue ? "true" : undefined}
                onChange={(e) => setRuleDraft({ ...rule, delayMs: Number(e.currentTarget.value) })}
              />
              <p id="rule-delay-help" className="field-help">
                응답 전에 기다릴 시간(밀리초)입니다. 0이면 지연 없이 바로 응답합니다 (허용 범위: 0~{MAX_RESPONSE_DELAY_MS}ms).
              </p>
              {delayIssue && <p id="rule-delay-error" className="field-error">{delayIssue.message}</p>}
            </div>
            <div className="rule-field">
              <label htmlFor="rule-body">응답 body</label>
              <textarea
                id="rule-body"
                placeholder="응답 body"
                value={rule.body}
                aria-describedby={`rule-body-help rule-headers-help${bodyIssue ? " rule-body-error" : ""}${headersIssue ? " rule-headers-error" : ""}`}
                aria-invalid={bodyIssue || headersIssue ? "true" : undefined}
                onChange={(e) => setRuleDraft({ ...rule, body: e.currentTarget.value })}
              />
              <p id="rule-body-help" className="field-help">
                매칭된 요청에 돌려줄 response body입니다. 저장된 headers와 함께 응답 규칙의 출력으로 사용됩니다. body는 최대 256,000자/1,024,000바이트입니다.
              </p>
              <p id="rule-headers-help" className="field-help">
                response headers는 최대 100개이며 이름 256자/256바이트, 값 16,384자/65,536바이트, 전체 64,000자/256,000바이트입니다.
              </p>
              {bodyIssue && <p id="rule-body-error" className="field-error">{bodyIssue.message}</p>}
              {headersIssue && <p id="rule-headers-error" className="field-error">{headersIssue.message}</p>}
            </div>
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
              <button
                type="button"
                className="mini"
                aria-label={`${targetRule.method ?? "*"} ${targetRule.path} 규칙 삭제`}
                disabled={busy}
                onClick={() => void onDeleteRule(targetRule)}
              >
                ✕
              </button>
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
              <button
                type="button"
                className="mini fixture-save"
                aria-label={`${request.method} ${request.url} masked fixture 저장`}
                disabled={busy}
                onClick={(event) => {
                  event.stopPropagation();
                  void onSaveFixture(request);
                }}
              >
                fixture 저장
              </button>
            </div>
          ))}
          {history.length === 0 && <div className="dim">수신 요청이 없습니다.</div>}
          <div className="fixture-section" aria-labelledby="fixtures-heading">
            <div className="fixture-heading">
              <h3 id="fixtures-heading">Fixtures ({fixtures.length})</h3>
              <button
                type="button"
                className="mini"
                aria-label="저장된 fixture 모두 삭제"
                disabled={busy || fixtures.length === 0}
                onClick={() => void onClearFixtures()}
              >
                전체 삭제
              </button>
            </div>
            <p className="field-help fixture-help">
              저장된 요청은 앱 전용 파일에 masked 상태로만 보관합니다. 원본 header·credential·안전하지 않은 path는 저장하지 않습니다.
            </p>
            {fixtures.map((fixture) => (
              <div
                key={fixture.id}
                className="fixture-row"
                aria-label={`${fixture.method} ${fixture.url} fixture`}
              >
                <div className="fixture-summary">
                  <span className="mono">{fixture.method} {fixture.url}</span>
                  <span className="dim">{formatFixtureTime(fixture.receivedAtMs)}</span>
                  <span className="masked">masked</span>
                </div>
                {fixture.body && <pre className="body">{fixture.body.slice(0, 200)}</pre>}
                <div className="fixture-actions">
                  <button
                    type="button"
                    className="mini"
                    disabled={busy}
                    aria-label={`${fixture.method} ${fixture.url} API Playground로 변환`}
                    onClick={() => void onSendFixtureToApi(fixture)}
                  >
                    API Playground로 변환
                  </button>
                  <button
                    type="button"
                    className="mini"
                    disabled={busy}
                    aria-label={`${fixture.method} ${fixture.url} 응답 rule 초안`}
                    onClick={() => void onDraftFixture(fixture)}
                  >
                    응답 rule 초안
                  </button>
                  <button
                    type="button"
                    className="mini danger-text"
                    disabled={busy}
                    aria-label={`${fixture.method} ${fixture.url} fixture 삭제`}
                    onClick={() => void onDeleteFixture(fixture)}
                  >
                    삭제
                  </button>
                </div>
              </div>
            ))}
            {fixtures.length === 0 && <div className="dim">저장된 fixture가 없습니다.</div>}
          </div>
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
