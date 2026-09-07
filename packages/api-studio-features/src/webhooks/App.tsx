import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { isKeyboardActivation } from "@devbox/a11y";
import { OPENAPI_DOCUMENT_LIMITS, type OpenApiDocumentFormat } from "@devbox/openapi";
import { useCallback, useEffect, useMemo, useRef, useState, type ChangeEvent } from "react";
import {
  clearFixtures,
  clearHistory,
  deleteFixture,
  copyHistoryHeaders,
  copyMaskedHistory,
  copyRawHistory,
  deleteHistory,
  deleteRule,
  exportRunServiceDefinition,
  fixtureToRule,
  listFixtures,
  listHistory,
  listRules,
  previewRuleConflicts,
  replayFixture,
  replayHistory,
  resetRuleSequence,
  saveFixture,
  sendFixtureToApi,
  sendFixtureToLogLens,
  sendHistoryToApi,
  sendHistoryToLogLens,
  serverStatus,
  setRule,
  startServer,
  stopServer,
  type RequestRecord,
  type ResponseRule,
  type ServerStatus,
  type CapturedFixture,
  type HandoffDispatch,
  type ResponseSequenceStep,
} from "./api";
import { buildHistoryContextMenu, buildRuleContextMenu } from "./lib/contextMenus";
import { buildExampleCurl, type CurlShell } from "./lib/exampleCurl";
import {
  openApiOperationToRule,
  previewOpenApiRules,
  type OpenApiRuleOperation,
  type OpenApiRulePreview,
} from "./lib/openapiRules";
import {
  MAX_METHOD_CHARS,
  MAX_RULE_PRIORITY,
  MAX_RESPONSE_DELAY_MS,
  MAX_RESPONSE_SEQUENCE,
  MAX_RESPONSE_STATUS,
  MIN_RESPONSE_STATUS,
  MIN_RULE_PRIORITY,
  validateRule,
  validateRuleCollection,
  type RuleValidationField,
} from "./lib/ruleValidation";
import "./App.css";

const DEFAULT_PORT = 9000;
const GENERIC_ERROR_MESSAGE = "요청을 처리하지 못했습니다. 입력과 서버 상태를 확인하세요.";
const STALE_HISTORY_MESSAGE = "선택한 요청 기록이 더 이상 존재하지 않습니다. 목록을 새로 고친 뒤 다시 시도하세요.";
const STALE_RULE_MESSAGE = "선택한 규칙이 더 이상 존재하지 않습니다. 목록을 새로 고친 뒤 다시 시도하세요.";
const OPENAPI_FILE_FORMAT_ERROR = "OpenAPI 파일 형식을 확인하세요. .json, .yaml, .yml만 선택할 수 있습니다.";
const OPENAPI_FILE_TOO_LARGE_ERROR = `OpenAPI 파일이 너무 큽니다. ${OPENAPI_DOCUMENT_LIMITS.maxBytes}바이트 이하 파일을 선택하세요.`;
const OPENAPI_FILE_READ_ERROR = "OpenAPI 파일을 읽지 못했습니다. JSON 또는 YAML 파일을 확인하세요.";
const RUN_DEFINITION_EXPORT_ERROR = "Run Manager 정의를 다운로드하지 못했습니다. 서버 상태를 확인한 뒤 다시 시도하세요.";
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
  "API Playground를 실행하지 못했습니다. handoff를 안전하게 정리했으며 클립보드로 자동 전환하지 않습니다",
  "API Playground handoff를 만들지 못했습니다. 클립보드로 자동 전환하지 않습니다",
  "Log Lens를 사용할 수 없습니다. 설치 또는 업데이트 후 다시 시도하세요. 클립보드로 자동 전환하지 않습니다",
  "Log Lens를 실행하지 못했습니다. handoff를 안전하게 정리했으며 클립보드로 자동 전환하지 않습니다",
  "Log Lens handoff를 만들지 못했습니다. 클립보드로 자동 전환하지 않습니다",
  "대상 앱 실행 실패 후 handoff를 정리하지 못했습니다. 잠시 후 다시 시도하세요",
  "handoff 요청에 사용할 fixture가 유효하지 않습니다",
  "localhost 서버가 실행 중이 아니거나 주소가 유효하지 않습니다",
  "replay 입력이 유효하지 않습니다",
  "replay 요청이 너무 많습니다. 잠시 후 다시 시도하세요",
  "replay 요청을 보내지 못했습니다",
  "replay 응답을 읽지 못했습니다",
  "replay는 데스크톱 앱에서만 사용할 수 있습니다",
  "response sequence를 초기화하지 못했습니다",
  "겹치는 규칙을 저장하려면 충돌 확인이 필요합니다",
  "실행 중인 loopback 서버만 Run Manager 서비스로 내보낼 수 있습니다",
  "Webhook service profile을 만들 수 없습니다",
  "credential 형태의 응답이 포함된 규칙은 service profile로 내보낼 수 없습니다",
  "Webhook service profile 개수 제한에 도달했습니다",
]);

const SAFE_ERROR_DISPLAY: Record<string, string> = {
  "replay 입력이 유효하지 않습니다": "재전송 입력이 유효하지 않습니다",
  "replay 요청이 너무 많습니다. 잠시 후 다시 시도하세요": "재전송 요청이 너무 많습니다. 잠시 후 다시 시도하세요",
  "replay 요청을 보내지 못했습니다": "재전송 요청을 보내지 못했습니다",
  "replay 응답을 읽지 못했습니다": "재전송 응답을 읽지 못했습니다",
  "replay는 데스크톱 앱에서만 사용할 수 있습니다": "재전송은 데스크톱 앱에서만 사용할 수 있습니다",
  "response sequence를 초기화하지 못했습니다": "응답 시퀀스를 초기화하지 못했습니다",
  "Webhook service profile을 만들 수 없습니다": "Webhook 서비스 프로필을 만들 수 없습니다",
  "credential 형태의 응답이 포함된 규칙은 service profile로 내보낼 수 없습니다": "인증 정보 형태의 응답이 포함된 규칙은 서비스 프로필로 내보낼 수 없습니다",
  "Webhook service profile 개수 제한에 도달했습니다": "Webhook 서비스 프로필 개수 제한에 도달했습니다",
};

function emptyRule(): ResponseRule {
  return {
    id: "",
    priority: 0,
    method: "POST",
    path: "/hook",
    status: 200,
    headers: [],
    body: "",
    delayMs: 0,
    sequence: [],
  };
}

function normalizeRule(rule: ResponseRule): ResponseRule {
  return {
    ...rule,
    priority: rule.priority ?? 0,
  };
}

function emptySequenceStep(): ResponseSequenceStep {
  return {
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
  return SAFE_ERROR_MESSAGES.has(message) ? SAFE_ERROR_DISPLAY[message] ?? message : GENERIC_ERROR_MESSAGE;
}

function safeExportMessage(error: unknown): string {
  const message = safeMessage(error);
  return message === GENERIC_ERROR_MESSAGE ? RUN_DEFINITION_EXPORT_ERROR : message;
}

function formatFixtureTime(receivedAtMs: number): string {
  const date = new Date(receivedAtMs);
  return Number.isFinite(date.getTime()) ? date.toISOString() : "시간 미상";
}

function compactRulePart(value: string, maxChars = 120): string {
  return value.length <= maxChars ? value : `${value.slice(0, maxChars - 1)}…`;
}

function formatRuleLabel(rule: ResponseRule | undefined, fallbackId: string): string {
  const method = compactRulePart(rule?.method ?? "*");
  const path = compactRulePart(rule?.path ?? "(경로 미상)");
  const id = compactRulePart(rule?.id || fallbackId);
  return `${method} ${path} [${id}]`;
}

function buildRuleConflictSummary(
  candidate: ResponseRule,
  conflicts: readonly {
    existingRuleId: string;
    winnerRuleId: string;
  }[],
  knownRules: readonly ResponseRule[],
): string {
  const candidateLabel = formatRuleLabel(candidate, candidate.id);
  const lines = conflicts.slice(0, 5).map((conflict) => {
    const existing = knownRules.find((knownRule) => knownRule.id === conflict.existingRuleId);
    const winner = conflict.winnerRuleId === candidate.id
      ? candidate
      : knownRules.find((knownRule) => knownRule.id === conflict.winnerRuleId);
    return `${candidateLabel} ↔ ${formatRuleLabel(existing, conflict.existingRuleId)} · 적용: ${formatRuleLabel(winner, conflict.winnerRuleId)}`;
  });
  if (conflicts.length > lines.length) lines.push(`외 ${conflicts.length - lines.length}개 충돌`);
  return [
    `겹치는 응답 규칙 ${conflicts.length}개가 있습니다.`,
    ...lines,
    "우선순위를 확인하고 저장할까요?",
  ].join("\n");
}

function openApiFormatForFileName(fileName: string): OpenApiDocumentFormat | null {
  const lowerName = fileName.toLowerCase();
  if (lowerName.endsWith(".json")) return "json";
  if (lowerName.endsWith(".yaml") || lowerName.endsWith(".yml")) return "yaml";
  return null;
}

function openApiSkipReason(reason: OpenApiRuleOperation["reason"]): string {
  switch (reason) {
    case "pathParametersUnsupported":
      return "경로 파라미터({param})가 있어 적용할 수 없습니다.";
    case "pathUnsupported":
      return "안전하지 않은 경로라 적용할 수 없습니다.";
    case "operationInvalid":
      return "operation 구조가 올바르지 않아 적용할 수 없습니다.";
    case "referenceUnsupported":
      return "참조($ref) operation은 적용할 수 없습니다.";
    default:
      return "이 operation은 적용할 수 없습니다.";
  }
}

function isLoopbackAddress(address: string | null): boolean {
  if (!address) return false;
  const value = address.trim();
  if (value === "localhost" || value.startsWith("localhost:")) return true;
  if (value.startsWith("[")) {
    const closingBracket = value.indexOf("]");
    return closingBracket > 1 && value.slice(1, closingBracket) === "::1";
  }
  const separator = value.lastIndexOf(":");
  const host = separator >= 0 ? value.slice(0, separator) : value;
  return host === "127.0.0.1" || host === "::1";
}

async function readOpenApiFile(file: File): Promise<string> {
  if (typeof file.text === "function") return file.text();
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(typeof reader.result === "string" ? reader.result : "");
    reader.onerror = () => reject(new Error("file read failed"));
    reader.readAsText(file);
  });
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
  const [openApiPreview, setOpenApiPreview] = useState<OpenApiRulePreview | null>(null);
  const [openApiError, setOpenApiError] = useState<string | null>(null);
  const [selectedOpenApiOperationId, setSelectedOpenApiOperationId] = useState<string | null>(null);
  const mountedRef = useRef(true);
  const operationInFlight = useRef(false);
  const refreshRequest = useRef(0);
  const openApiInputRef = useRef<HTMLInputElement>(null);

  const beginBusy = useCallback(() => {
    if (!mountedRef.current || operationInFlight.current) return false;
    operationInFlight.current = true;
    setBusy(true);
    return true;
  }, []);

  const endBusy = useCallback(() => {
    operationInFlight.current = false;
    if (mountedRef.current) setBusy(false);
  }, []);

  const prepareHistoryContext = useCallback((target: HTMLElement) => {
    const id = Number(target.dataset.historyId);
    const request = history.find((candidate) => candidate.id === id);
    if (!request) {
      setContextHistory(null);
      setSelectedHistoryId(null);
      setError(STALE_HISTORY_MESSAGE);
      return;
    }
    setSelectedHistoryId(request.id);
    setContextHistory(request);
  }, [history]);
  const historyContextMenu = useContextMenu({
    disabled: busy,
    onBeforeOpen: (_reason, target) => prepareHistoryContext(target),
  });
  const historyContextTrigger = historyContextMenu.triggerProps;

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
  const ruleContextTrigger = ruleContextMenu.triggerProps;

  const refresh = useCallback(async () => {
    const request = refreshRequest.current + 1;
    refreshRequest.current = request;
    const [statusResult, historyResult, rulesResult, fixtureResult] = await Promise.allSettled([
      serverStatus(),
      listHistory(),
      listRules(),
      listFixtures(),
    ]);
    if (!mountedRef.current || refreshRequest.current !== request) return;
    if (statusResult.status === "fulfilled") setStatus(statusResult.value);
    if (historyResult.status === "fulfilled") setHistory(historyResult.value);
    if (rulesResult.status === "fulfilled") {
      setRules(rulesResult.value.map(normalizeRule));
    }
    if (fixtureResult.status === "fulfilled") setFixtures(fixtureResult.value);
    else setFixtures([]);
    const failure = [statusResult, historyResult, rulesResult, fixtureResult]
      .find((result): result is PromiseRejectedResult => result.status === "rejected");
    if (failure) setError(safeMessage(failure.reason));
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    void refresh();
    return () => {
      // Invalidate the mount refresh and any action-owned refresh before a
      // late promise can update a newer view or an unmounted app.
      refreshRequest.current += 1;
      operationInFlight.current = false;
      mountedRef.current = false;
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
      const nextStatus = await startServer(bind, port, lanBind);
      if (!mountedRef.current) return;
      setStatus(nextStatus);
      await refresh();
    } catch (e) {
      if (mountedRef.current) setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onStop = async () => {
    if (!beginBusy()) return;
    setError(null);
    try {
      const nextStatus = await stopServer();
      if (!mountedRef.current) return;
      setStatus(nextStatus);
      // Stop changes the source of truth. A refresh that started before the
      // stop must not restore the old running status or stale rule/history view.
      await refresh();
    } catch (e) {
      if (mountedRef.current) setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onOpenApiFile = async (file: File, format: OpenApiDocumentFormat) => {
    // Reject oversized input before touching File.text() so the renderer never
    // loads an unbounded local document into memory.
    if (file.size > OPENAPI_DOCUMENT_LIMITS.maxBytes) {
      setOpenApiPreview(null);
      setSelectedOpenApiOperationId(null);
      setOpenApiError(OPENAPI_FILE_TOO_LARGE_ERROR);
      return;
    }
    if (!beginBusy()) return;
    setOpenApiPreview(null);
    setSelectedOpenApiOperationId(null);
    setOpenApiError(null);
    try {
      const text = await readOpenApiFile(file);
      const result = previewOpenApiRules(text, format, file.name);
      if (!mountedRef.current) return;
      if (result.ok) setOpenApiPreview(result.preview);
      else setOpenApiError(result.message);
    } catch {
      if (mountedRef.current) setOpenApiError(OPENAPI_FILE_READ_ERROR);
    } finally {
      endBusy();
    }
  };

  const onOpenApiFileChange = (event: ChangeEvent<HTMLInputElement>) => {
    const input = event.currentTarget;
    const file = input.files?.[0];
    // Reset immediately so selecting the same file again still emits change.
    input.value = "";
    if (!file) return;
    const format = openApiFormatForFileName(file.name);
    if (!format) {
      setOpenApiPreview(null);
      setSelectedOpenApiOperationId(null);
      setOpenApiError(OPENAPI_FILE_FORMAT_ERROR);
      return;
    }
    void onOpenApiFile(file, format);
  };

  const onApplyOpenApiDraft = () => {
    if (!openApiPreview || !selectedOpenApiOperationId) return;
    const operation = openApiPreview.operations.find(({ id }) => id === selectedOpenApiOperationId);
    if (!operation || !operation.applyable) return;
    const draft = openApiOperationToRule(operation);
    if (!draft) return;
    if (!window.confirm(`${operation.method} ${operation.path} → ${operation.status} operation을 규칙 초안으로 편집기에 채울까요?`)) return;
    setRuleDraft(draft);
    setOpenApiError(null);
  };

  const canExportRunDefinition = status.running && isLoopbackAddress(status.address);

  const onExportRunDefinition = async () => {
    if (!canExportRunDefinition || !beginBusy()) return;
    setError(null);
    let objectUrl: string | null = null;
    try {
      const definition = await exportRunServiceDefinition();
      if (!mountedRef.current) return;
      const blob = new Blob([JSON.stringify(definition, null, 2)], { type: "application/json" });
      objectUrl = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = objectUrl;
      link.download = "webhook-lab-run-manager-definition.json";
      document.body.appendChild(link);
      try {
        link.click();
      } finally {
        link.remove();
      }
    } catch (caught) {
      if (mountedRef.current) setError(safeExportMessage(caught));
    } finally {
      if (objectUrl) {
        try {
          URL.revokeObjectURL(objectUrl);
        } catch {
          // Cleanup failure must not replace the fixed user-facing result.
        }
      }
      endBusy();
    }
  };

  const onSaveRule = async () => {
    const draft = normalizeRule(rule);
    const [validationIssue] = validateRule(draft);
    if (validationIssue) {
      setError(validationIssue.message);
      return;
    }

    if (draft.id && !rules.some((candidate) => candidate.id === draft.id)) {
      setError(STALE_RULE_MESSAGE);
      return;
    }

    const projectedRules = draft.id
      ? [...rules.filter((candidate) => candidate.id !== draft.id), draft]
      : [...rules, draft];
    const [collectionIssue] = validateRuleCollection(projectedRules);
    if (collectionIssue) {
      setError(collectionIssue.message);
      return;
    }

    if (!beginBusy()) return;
    setError(null);
    try {
      const preview = await previewRuleConflicts(draft);
      if (!mountedRef.current) return;
      const candidate = { ...draft, id: preview.candidateId };
      const requiresConfirmation = preview.requiresConfirmation || preview.conflicts.length > 0;
      if (
        requiresConfirmation
        && !window.confirm(buildRuleConflictSummary(candidate, preview.conflicts, rules))
      ) {
        return;
      }
      await setRule(candidate, requiresConfirmation);
      if (!mountedRef.current) return;
      setRuleDraft(emptyRule());
      await refresh();
    } catch (e) {
      if (mountedRef.current) setError(safeMessage(e));
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
      // Do not let a late IPC response trigger a clipboard write after the
      // view that requested it has been unmounted.
      if (!mountedRef.current) return;
      await navigator.clipboard.writeText(text);
    } catch {
      if (mountedRef.current) setError(failureMessage);
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
      if (!mountedRef.current || refreshRequest.current !== request) return;
      setStatus(freshStatus);
      setRules(freshRules.map(normalizeRule));

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
        if (mountedRef.current) setError("예시 curl을 복사하지 못했습니다.");
      }
    } catch (e) {
      if (mountedRef.current) setError(safeMessage(e));
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
      if (!mountedRef.current) return;
      setSelectedHistoryId(null);
      await refresh();
    } catch (e) {
      if (mountedRef.current) setError(safeMessage(e));
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
      if (!mountedRef.current) return;
      setSelectedHistoryId(null);
      await refresh();
    } catch (e) {
      if (mountedRef.current) setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onSaveFixture = async (request: RequestRecord) => {
    if (!beginBusy()) return;
    setError(null);
    try {
      await saveFixture(request.id);
      if (!mountedRef.current) return;
      await refresh();
    } catch (e) {
      if (mountedRef.current) setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const showReplaySuccess = (source: string, statusCode: number) => {
    if (mountedRef.current) {
      const sourceLabel = source === "history" ? "기록" : "fixture";
      setHandoffNotice("마스킹된 " + sourceLabel + " 요청을 localhost에 재전송했습니다 (현재 로컬 listener). 응답 status: " + statusCode + ".");
    }
  };

  const onReplayHistory = async (request: RequestRecord) => {
    if (!beginBusy()) return;
    setError(null);
    setHandoffNotice(null);
    try {
      const result = await replayHistory(request.id);
      if (!mountedRef.current) return;
      showReplaySuccess("history", result.status);
      await refresh();
    } catch (e) {
      if (mountedRef.current) setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onReplayFixture = async (fixture: CapturedFixture) => {
    if (!beginBusy()) return;
    setError(null);
    setHandoffNotice(null);
    try {
      const result = await replayFixture(fixture.id);
      if (!mountedRef.current) return;
      showReplaySuccess("fixture", result.status);
      await refresh();
    } catch (e) {
      if (mountedRef.current) setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const showHandoffSuccess = (dispatch: HandoffDispatch, targetName: string) => {
    if (!mountedRef.current) return;
    setHandoffNotice(
      `${targetName} 미리보기로 전달했습니다. producer: ${dispatch.producerId} · consumer: ${dispatch.consumerId} · handoff: ${dispatch.handoffId}. 적용 전 내용을 확인하세요.`,
    );
  };

  const onSendHistoryToApi = async (request: RequestRecord) => {
    if (!beginBusy()) return;
    setError(null);
    setHandoffNotice(null);
    try {
      const dispatch = await sendHistoryToApi(request.id);
      if (!mountedRef.current) return;
      showHandoffSuccess(dispatch, "API Playground");
    } catch (e) {
      if (mountedRef.current) setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onSendFixtureToApi = async (fixture: CapturedFixture) => {
    if (!beginBusy()) return;
    setError(null);
    setHandoffNotice(null);
    try {
      const dispatch = await sendFixtureToApi(fixture.id);
      if (!mountedRef.current) return;
      showHandoffSuccess(dispatch, "API Playground");
    } catch (e) {
      if (mountedRef.current) setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onSendHistoryToLogLens = async (request: RequestRecord) => {
    if (!beginBusy()) return;
    setError(null);
    setHandoffNotice(null);
    try {
      const dispatch = await sendHistoryToLogLens(request.id);
      if (!mountedRef.current) return;
      showHandoffSuccess(dispatch, "Log Lens");
    } catch (e) {
      if (mountedRef.current) setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onSendFixtureToLogLens = async (fixture: CapturedFixture) => {
    if (!beginBusy()) return;
    setError(null);
    setHandoffNotice(null);
    try {
      const dispatch = await sendFixtureToLogLens(fixture.id);
      if (!mountedRef.current) return;
      showHandoffSuccess(dispatch, "Log Lens");
    } catch (e) {
      if (mountedRef.current) setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onResetSequence = async (targetRule: ResponseRule) => {
    const currentRule = rules.find((candidate) => candidate.id === targetRule.id);
    if (!currentRule) {
      setError(STALE_RULE_MESSAGE);
      return;
    }
    if (!beginBusy()) return;
    setError(null);
    setHandoffNotice(null);
    try {
      await resetRuleSequence(currentRule.id);
      if (!mountedRef.current) return;
      setHandoffNotice("응답 시퀀스의 현재 위치를 첫 응답으로 초기화했습니다.");
    } catch (e) {
      if (mountedRef.current) setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onDraftFixture = async (fixture: CapturedFixture) => {
    if (!beginBusy()) return;
    setError(null);
    try {
      const draft = await fixtureToRule(fixture.id);
      if (!mountedRef.current) return;
      setRuleDraft(normalizeRule(draft));
    } catch (e) {
      if (mountedRef.current) setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onDeleteFixture = async (fixture: CapturedFixture) => {
    if (!window.confirm("선택한 마스킹된 fixture를 삭제할까요?")) return;
    if (!beginBusy()) return;
    setError(null);
    try {
      await deleteFixture(fixture.id);
      if (!mountedRef.current) return;
      await refresh();
    } catch (e) {
      if (mountedRef.current) setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const onClearFixtures = async () => {
    if (!window.confirm("저장된 마스킹된 fixture를 모두 삭제할까요? 이 작업은 되돌릴 수 없습니다.")) return;
    if (!beginBusy()) return;
    setError(null);
    try {
      await clearFixtures();
      if (!mountedRef.current) return;
      await refresh();
    } catch (e) {
      if (mountedRef.current) setError(safeMessage(e));
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
      if (!mountedRef.current) return;
      setSelectedRuleId(null);
      if (rule.id === targetRule.id) setRuleDraft(emptyRule());
      await refresh();
    } catch (e) {
      if (mountedRef.current) setError(safeMessage(e));
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

    const duplicate = normalizeRule({
      ...currentRule,
      id: "",
      headers: [...currentRule.headers],
      sequence: (currentRule.sequence ?? []).map((step) => ({
        ...step,
        headers: [...step.headers],
      })),
    });
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
      const preview = await previewRuleConflicts(duplicate);
      if (!mountedRef.current) return;
      const candidate = { ...duplicate, id: preview.candidateId };
      const requiresConfirmation = preview.requiresConfirmation || preview.conflicts.length > 0;
      if (
        requiresConfirmation
        && !window.confirm(buildRuleConflictSummary(candidate, preview.conflicts, rules))
      ) {
        return;
      }
      await setRule(candidate, requiresConfirmation);
      if (!mountedRef.current) return;
      await refresh();
    } catch (e) {
      if (mountedRef.current) setError(safeMessage(e));
    } finally {
      endBusy();
    }
  };

  const historyContextItems = useMemo<readonly ContextMenuEntry[]>(
    () => buildHistoryContextMenu(busy, status.running && Boolean(status.address)),
    [busy, status.address, status.running],
  );
  const ruleContextItems = useMemo<readonly ContextMenuEntry[]>(
    () => buildRuleContextMenu(busy, status.running && Boolean(status.address)),
    [busy, status.address, status.running],
  );

  const ruleIssues = validateRule(rule);
  const sequence = rule.sequence ?? [];
  const ruleIssueFor = (field: RuleValidationField) =>
    ruleIssues.find((issue) => issue.field === field);
  const methodIssue = ruleIssueFor("method");
  const pathIssue = ruleIssueFor("path");
  const priorityIssue = ruleIssueFor("priority");
  const statusIssue = ruleIssueFor("status");
  const headersIssue = ruleIssueFor("headers");
  const bodyIssue = ruleIssueFor("body");
  const delayIssue = ruleIssueFor("delayMs");
  const sequenceIssue = ruleIssueFor("sequence");

  const addSequenceStep = () => {
    if (sequence.length >= MAX_RESPONSE_SEQUENCE) return;
    setRuleDraft({ ...rule, sequence: [...sequence, emptySequenceStep()] });
  };

  const updateSequenceStep = (index: number, patch: Partial<ResponseSequenceStep>) => {
    setRuleDraft({
      ...rule,
      sequence: sequence.map((step, stepIndex) => stepIndex === index ? { ...step, ...patch } : step),
    });
  };

  const removeSequenceStep = (index: number) => {
    setRuleDraft({
      ...rule,
      sequence: sequence.filter((_step, stepIndex) => stepIndex !== index),
    });
  };

  const onHistoryContextSelect = (id: string) => {
    const request = contextHistory;
    if (!request) {
      setError(STALE_HISTORY_MESSAGE);
      return;
    }
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
    } else if (id === "replay") {
      void onReplayHistory(request);
    } else if (id === "convert-api-playground") {
      void onSendHistoryToApi(request);
    } else if (id === "inspect-log-lens") {
      void onSendHistoryToLogLens(request);
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
    if (id === "edit") {
      setRuleDraft({
        ...currentRule,
        headers: [...currentRule.headers],
        sequence: (currentRule.sequence ?? []).map((step) => ({
          ...step,
          headers: [...step.headers],
        })),
      });
    }
    else if (id === "duplicate") void onDuplicateRule(currentRule);
    else if (id === "reset-sequence") void onResetSequence(currentRule);
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
        <button
          type="button"
          className="btn"
          disabled={busy || !canExportRunDefinition}
          onClick={() => void onExportRunDefinition()}
          title="실행 중인 loopback 서버에서만 사용할 수 있습니다"
        >
          Run Manager 정의 JSON 다운로드
        </button>
      </header>

      {lanBind && <div className="warn">LAN 공개는 명시적 설정입니다. 외부에서 접근 가능합니다.</div>}
      {error && <div className="error" role="alert" aria-live="assertive">{error}</div>}
      {handoffNotice && <div className="handoff-notice" role="status" aria-live="polite">{handoffNotice}</div>}

      <div className="main">
        <section className="panel">
          <h2>규칙</h2>
          <p className="field-help precedence-help">
            우선순위가 높을수록 먼저 적용됩니다. 같으면 정확한 path, method 지정, 긴 와일드카드 순서이며 마지막에는 규칙 ID로 결정합니다.
          </p>
          <div className="rule-editor">
            <div className="rule-field">
              <label htmlFor="rule-method">method</label>
              <input
                id="rule-method"
                placeholder="method (없으면 전체)"
                value={rule.method ?? ""}
                maxLength={MAX_METHOD_CHARS}
                disabled={busy}
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
                disabled={busy}
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
              <label htmlFor="rule-priority">priority</label>
              <input
                id="rule-priority"
                type="number"
                placeholder="priority"
                value={rule.priority ?? 0}
                min={MIN_RULE_PRIORITY}
                max={MAX_RULE_PRIORITY}
                step={1}
                disabled={busy}
                aria-describedby={`rule-priority-help${priorityIssue ? " rule-priority-error" : ""}`}
                aria-invalid={priorityIssue ? "true" : undefined}
                onChange={(e) => setRuleDraft({ ...rule, priority: Number(e.currentTarget.value) })}
              />
              <p id="rule-priority-help" className="field-help">
                우선순위가 높을수록 먼저 적용됩니다 (허용 범위: {MIN_RULE_PRIORITY}~{MAX_RULE_PRIORITY}). 같은 값이면 정확한 path와 method 지정 규칙이 우선합니다.
              </p>
              {priorityIssue && <p id="rule-priority-error" className="field-error">{priorityIssue.message}</p>}
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
                disabled={busy}
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
                disabled={busy}
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
                disabled={busy}
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
            <div
              className="sequence-editor"
              aria-describedby={"rule-sequence-help" + (sequenceIssue ? " rule-sequence-error" : "")}
              aria-invalid={sequenceIssue ? "true" : undefined}
            >
              <div className="sequence-heading">
                <div>
                  <h3>응답 시퀀스</h3>
                  <p id="rule-sequence-help" className="field-help">
                    위 응답을 첫 단계로 사용한 뒤 아래 단계를 순서대로 적용합니다. 마지막 단계는 유지되며 자동 반복하지 않습니다 (최대 {MAX_RESPONSE_SEQUENCE}단계).
                  </p>
                </div>
                <button
                  type="button"
                  className="mini"
                  disabled={busy || sequence.length >= MAX_RESPONSE_SEQUENCE}
                  onClick={addSequenceStep}
                >
                  응답 단계 추가 ({sequence.length}/{MAX_RESPONSE_SEQUENCE})
                </button>
              </div>
              {sequence.map((step, index) => (
                <div className="sequence-step" key={index}>
                  <div className="sequence-step-heading">
                    <span className="mono">단계 {index + 1}</span>
                    <button
                      type="button"
                      className="mini danger-text"
                      disabled={busy}
                      aria-label={"응답 단계 " + (index + 1) + " 삭제"}
                      onClick={() => removeSequenceStep(index)}
                    >
                      삭제
                    </button>
                  </div>
                  <div className="sequence-step-grid">
                    <label>
                      status
                      <input
                        type="number"
                        min={MIN_RESPONSE_STATUS}
                        max={MAX_RESPONSE_STATUS}
                        step={1}
                        value={step.status}
                        aria-label={"응답 단계 " + (index + 1) + " status"}
                        disabled={busy}
                        onChange={(event) => updateSequenceStep(index, { status: Number(event.currentTarget.value) })}
                      />
                    </label>
                    <label>
                      delay (ms)
                      <input
                        type="number"
                        min={0}
                        max={MAX_RESPONSE_DELAY_MS}
                        step={1}
                        value={step.delayMs}
                        aria-label={"응답 단계 " + (index + 1) + " delay"}
                        disabled={busy}
                        onChange={(event) => updateSequenceStep(index, { delayMs: Number(event.currentTarget.value) })}
                      />
                    </label>
                  </div>
                  <label>
                    응답 body
                    <textarea
                      value={step.body}
                      aria-label={"응답 단계 " + (index + 1) + " body"}
                      disabled={busy}
                      onChange={(event) => updateSequenceStep(index, { body: event.currentTarget.value })}
                    />
                  </label>
                </div>
              ))}
              {sequenceIssue && <p id="rule-sequence-error" className="field-error">{sequenceIssue.message}</p>}
            </div>
            <button className="btn primary" disabled={busy} onClick={() => void onSaveRule()}>{rule.id ? "규칙 저장" : "규칙 추가"}</button>
          </div>
          <section className="openapi-section" aria-labelledby="openapi-heading">
            <div className="openapi-heading">
              <div>
                <h3 id="openapi-heading">OpenAPI → 규칙 초안</h3>
                <p className="field-help">
                  JSON/YAML 파일을 선택하면 안전한 operation만 미리봅니다. 선택하고 확인해야 편집기에 채워지며 자동 저장하지 않습니다.
                </p>
              </div>
              <div className="openapi-file-actions">
                <input
                  ref={openApiInputRef}
                  className="visually-hidden"
                  type="file"
                  accept=".json,.yaml,.yml,application/json,application/yaml,text/yaml"
                  aria-label="OpenAPI JSON/YAML 파일 선택"
                  disabled={busy}
                  onChange={onOpenApiFileChange}
                />
                <button
                  type="button"
                  className="mini"
                  disabled={busy}
                  onClick={() => openApiInputRef.current?.click()}
                >
                  OpenAPI 파일 선택
                </button>
              </div>
            </div>
            {openApiError && <p className="field-error" role="alert">{openApiError}</p>}
            {openApiPreview && (
              <div className="openapi-preview" aria-label="OpenAPI operation 미리보기">
                <p className="field-help">
                  {openApiPreview.sourceName} · OpenAPI {openApiPreview.version} · {openApiPreview.operations.length}개 operation
                </p>
                {openApiPreview.operations.length === 0 && (
                  <p className="dim">미리볼 operation이 없습니다.</p>
                )}
                <div className="openapi-operations" role="list">
                  {openApiPreview.operations.map((operation) => (
                    <label
                      className={`openapi-operation ${operation.applyable ? "" : "unavailable"}`}
                      key={operation.id}
                      role="listitem"
                    >
                      <input
                        type="radio"
                        name="openapi-operation"
                        value={operation.id}
                        checked={selectedOpenApiOperationId === operation.id}
                        disabled={busy || !operation.applyable}
                        aria-label={`${operation.method} ${operation.path} (${operation.status})`}
                        onChange={() => setSelectedOpenApiOperationId(operation.id)}
                      />
                      <span className="mono">{operation.method} {operation.path} → {operation.status}</span>
                      {!operation.applyable && (
                        <span className="field-error">{openApiSkipReason(operation.reason)}</span>
                      )}
                    </label>
                  ))}
                </div>
                <button
                  type="button"
                  className="mini"
                  disabled={busy || !selectedOpenApiOperationId}
                  onClick={onApplyOpenApiDraft}
                >
                  선택한 operation을 규칙 초안에 적용
                </button>
              </div>
            )}
          </section>
          {rules.map((targetRule) => (
            <div
              key={targetRule.id}
              className={`rule-row ${selectedRuleId === targetRule.id ? "selected" : ""}`}
              tabIndex={0}
              aria-current={selectedRuleId === targetRule.id ? "true" : undefined}
              aria-label={`${targetRule.method ?? "*"} ${targetRule.path} 규칙`}
              data-rule-id={targetRule.id}
              onClick={() => setSelectedRuleId(targetRule.id)}
              onContextMenu={ruleContextTrigger.onContextMenu}
              onKeyDown={(event) => {
                ruleContextTrigger.onKeyDown?.(event);
                if (
                  event.defaultPrevented
                  || event.target !== event.currentTarget
                  || !isKeyboardActivation(event)
                ) return;
                event.preventDefault();
                setSelectedRuleId(targetRule.id);
              }}
            >
              {(targetRule.sequence?.length ?? 0) > 0 && (
                <span className="sequence-badge">{(targetRule.sequence?.length ?? 0) + 1}개 응답</span>
              )}
              <button
                type="button"
                className="mini"
                aria-label={(targetRule.method ?? "*") + " " + targetRule.path + " 응답 시퀀스 초기화"}
                disabled={busy}
                onClick={(event) => {
                  event.stopPropagation();
                  void onResetSequence(targetRule);
                }}
              >
                시퀀스 초기화
              </button>
              <span className="mono">{targetRule.method ?? "*"} {targetRule.path} → {targetRule.status}{targetRule.delayMs ? ` (+${targetRule.delayMs}ms)` : ""}</span>
              <span className="priority-badge">우선순위 {targetRule.priority ?? 0}</span>
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
          <h2>요청 기록 ({history.length})</h2>
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
              onContextMenu={historyContextTrigger.onContextMenu}
              onKeyDown={(event) => {
                historyContextTrigger.onKeyDown?.(event);
                if (
                  event.defaultPrevented
                  || event.target !== event.currentTarget
                  || !isKeyboardActivation(event)
                ) return;
                event.preventDefault();
                setSelectedHistoryId(request.id);
              }}
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
                className="mini"
                aria-label={request.method + " " + request.url + " 마스킹된 재전송"}
                disabled={busy || !status.running || !status.address}
                onClick={(event) => {
                  event.stopPropagation();
                  void onReplayHistory(request);
                }}
              >
                마스킹된 재전송
              </button>
              <button
                type="button"
                className="mini fixture-save"
                aria-label={`${request.method} ${request.url} 마스킹된 fixture 저장`}
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
              <h3 id="fixtures-heading">저장된 fixture ({fixtures.length})</h3>
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
              저장된 요청은 앱 전용 파일에 마스킹된 상태로만 보관합니다. 원본 헤더·인증 정보·안전하지 않은 path는 저장하지 않습니다.
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
                  <span className="masked">마스킹됨</span>
                </div>
                {fixture.body && <pre className="body">{fixture.body.slice(0, 200)}</pre>}
                <div className="fixture-actions">
                  <button
                    type="button"
                    className="mini"
                    disabled={busy || !status.running || !status.address}
                    aria-label={fixture.method + " " + fixture.url + " 마스킹된 재전송"}
                    onClick={() => void onReplayFixture(fixture)}
                  >
                    마스킹된 재전송
                  </button>
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
                    aria-label={`${fixture.method} ${fixture.url} Log Lens에서 보기`}
                    onClick={() => void onSendFixtureToLogLens(fixture)}
                  >
                    Log Lens에서 보기
                  </button>
                  <button
                    type="button"
                    className="mini"
                    disabled={busy}
                    aria-label={`${fixture.method} ${fixture.url} 응답 규칙 초안`}
                    onClick={() => void onDraftFixture(fixture)}
                  >
                    응답 규칙 초안
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
