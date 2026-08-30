import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";

export interface ServerStatus {
  running: boolean;
  address: string | null;
}

export interface RequestRecord {
  id: number;
  method: string;
  url: string;
  headers: Array<[string, string]>;
  body: string;
  receivedAtMs: number;
}

export interface ResponseRule {
  id: string;
  priority: number;
  method: string | null;
  path: string;
  status: number;
  headers: Array<[string, string]>;
  body: string;
  delayMs: number;
  /** Additional responses after the base response; absent means no sequence. */
  sequence?: ResponseSequenceStep[];
}

/** Wire-compatible shape for rules written before priority was introduced. */
export type ResponseRulePayload = Omit<ResponseRule, "priority"> & {
  priority?: number | null;
};

export type RuleConflictKind =
  | "candidateShadowsExisting"
  | "existingShadowsCandidate"
  | "partialOverlap";

export type RuleConflictReason =
  | "priority"
  | "exactPath"
  | "methodSpecific"
  | "longerWildcardPrefix"
  | "idTieBreak";

export interface RuleConflict {
  existingRuleId: string;
  winnerRuleId: string;
  loserRuleId: string;
  kind: RuleConflictKind;
  reason: RuleConflictReason;
}

export interface RuleConflictPreview {
  candidateId: string;
  conflicts: RuleConflict[];
  requiresConfirmation: boolean;
}

/** Normalize a legacy rule payload without masking any non-legacy fields. */
export function normalizeResponseRule(rule: ResponseRulePayload): ResponseRule {
  return {
    ...rule,
    priority: rule.priority ?? 0,
  };
}

export interface ResponseSequenceStep {
  status: number;
  headers: Array<[string, string]>;
  body: string;
  delayMs: number;
}

export interface CapturedFixture {
  id: string;
  method: string;
  url: string;
  headers: Array<[string, string]>;
  body: string;
  receivedAtMs: number;
}

export interface ApiHandoffDispatch {
  handoffId: string;
  producerId: string;
  consumerId: string;
  createdAtMs: number;
  expiresAtMs: number;
}

export interface ReplayResult {
  sourceId: string;
  status: number;
}

/** The disabled Run Manager service definition returned by the native export. */
export interface RunServiceDefinition {
  id: string;
  kind: string;
  name: string;
  command: string;
  cwd: string | null;
  targetKind: string;
  targetDistro: string | null;
  envConfigured: boolean;
  cronExpr: string | null;
  enabled: boolean;
  overlapPolicy: string;
  catchUp: boolean;
  lastEvaluatedAt: number | null;
  nextQueueSequence: number;
  restartPolicy: string | null;
  autoStart: boolean | null;
  healthTcpAddress: string | null;
  healthTcpPort: number | null;
  healthStartGraceMs: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface RunDefinitionExport {
  schemaVersion: number;
  exportedAt: string;
  jobs: RunServiceDefinition[];
  services: RunServiceDefinition[];
}

const HANDOFF_BROWSER_ERROR =
  "API Playground handoff는 데스크톱 앱에서만 사용할 수 있습니다. 클립보드로 자동 전환하지 않습니다";
const REPLAY_BROWSER_ERROR =
  "replay는 데스크톱 앱에서만 사용할 수 있습니다";

// Keep browser previews harmless: this definition has a fixed harmless command,
// a disabled service, and is never persisted by the mock API.
const MOCK_RUN_DEFINITION: RunDefinitionExport = {
  schemaVersion: 1,
  exportedAt: "1",
  jobs: [],
  services: [{
    id: "00000000-0000-4000-8000-000000000001",
    kind: "service",
    name: "Webhook Lab (browser preview)",
    command: "exit /b 1",
    cwd: null,
    targetKind: "windows",
    targetDistro: null,
    envConfigured: false,
    cronExpr: null,
    enabled: false,
    overlapPolicy: "skip",
    catchUp: false,
    lastEvaluatedAt: null,
    nextQueueSequence: 0,
    restartPolicy: "never",
    autoStart: false,
    healthTcpAddress: "127.0.0.1",
    healthTcpPort: 9000,
    healthStartGraceMs: 10_000,
    createdAt: 1,
    updatedAt: 1,
  }],
};

const MOCK_HISTORY: RequestRecord[] = [
  { id: 1, method: "POST", url: "/hook", headers: [["content-type", "application/json"]], body: '{"event":"push"}', receivedAtMs: Date.now() - 30000 },
  { id: 2, method: "GET", url: "/health", headers: [], body: "", receivedAtMs: Date.now() - 10000 },
];

const MOCK_FIXTURES: CapturedFixture[] = [];
let nextMockFixtureId = 1;

function fixtureOrder(left: CapturedFixture, right: CapturedFixture): number {
  if (left.receivedAtMs < right.receivedAtMs) return 1;
  if (left.receivedAtMs > right.receivedAtMs) return -1;
  if (left.id < right.id) return -1;
  if (left.id > right.id) return 1;
  return 0;
}

export function serverStatus(): Promise<ServerStatus> {
  if (!isTauri()) return Promise.resolve({ running: false, address: null });
  return invoke<ServerStatus>("server_status");
}

/** Export only the backend-owned, disabled Run Manager definition. */
export function exportRunServiceDefinition(): Promise<RunDefinitionExport> {
  if (!isTauri()) {
    return Promise.resolve({
      ...MOCK_RUN_DEFINITION,
      jobs: [],
      services: MOCK_RUN_DEFINITION.services.map((service) => ({ ...service })),
    });
  }
  return invoke<RunDefinitionExport>("export_run_service_definition");
}

export function startServer(bind: string | null, port: number, allowLan = false): Promise<ServerStatus> {
  if (!isTauri()) return Promise.resolve({ running: true, address: `${bind ?? "127.0.0.1"}:${port}` });
  return invoke<ServerStatus>("start_server", { bind, port, allowLan });
}

export function stopServer(): Promise<ServerStatus> {
  if (!isTauri()) return Promise.resolve({ running: false, address: null });
  return invoke<ServerStatus>("stop_server");
}

export function listHistory(): Promise<RequestRecord[]> {
  if (!isTauri()) return Promise.resolve(MOCK_HISTORY);
  return invoke<RequestRecord[]>("list_history");
}

export function clearHistory(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("clear_history");
}

function mockHistoryRecord(id: number): RequestRecord {
  const record = MOCK_HISTORY.find((candidate) => candidate.id === id);
  if (!record) throw new Error("요청 기록을 찾을 수 없습니다");
  return record;
}

export function copyMaskedHistory(id: number): Promise<string> {
  if (!isTauri()) return Promise.resolve(JSON.stringify(mockHistoryRecord(id), null, 2));
  return invoke<string>("copy_masked_history", { id });
}

export function copyRawHistory(id: number): Promise<string> {
  if (!isTauri()) return Promise.reject(new Error("원본 요청 복사는 데스크톱 앱에서만 사용할 수 있습니다"));
  return invoke<string>("copy_raw_history", { id });
}

export function copyHistoryHeaders(id: number): Promise<string> {
  if (!isTauri()) {
    const headers = mockHistoryRecord(id).headers;
    return Promise.resolve(headers.map(([name, value]) => `${name}: ${value}`).join("\n"));
  }
  return invoke<string>("copy_history_headers", { id });
}

export function deleteHistory(id: number): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("delete_history", { id });
}

/** Replay only a backend-owned masked history snapshot to the local server. */
export function replayHistory(id: number): Promise<ReplayResult> {
  if (!isTauri()) return Promise.reject(new Error(REPLAY_BROWSER_ERROR));
  return invoke<ReplayResult>("replay_history", { historyId: id });
}

export function listFixtures(): Promise<CapturedFixture[]> {
  if (!isTauri()) {
    return Promise.resolve(MOCK_FIXTURES.map((fixture) => ({ ...fixture })).sort(fixtureOrder));
  }
  return invoke<CapturedFixture[]>("list_fixtures");
}

export function saveFixture(historyId: number): Promise<CapturedFixture> {
  if (!isTauri()) {
    const request = MOCK_HISTORY.find((candidate) => candidate.id === historyId);
    if (!request) return Promise.reject(new Error("fixture를 찾을 수 없습니다"));
    const fixture: CapturedFixture = {
      id: `fixture-${nextMockFixtureId++}`,
      method: request.method,
      url: request.url,
      headers: request.headers.map((header) => [...header] as [string, string]),
      body: request.body,
      receivedAtMs: request.receivedAtMs,
    };
    MOCK_FIXTURES.push(fixture);
    return Promise.resolve(fixture);
  }
  return invoke<CapturedFixture>("save_fixture", { historyId });
}

export function deleteFixture(id: string): Promise<void> {
  if (!isTauri()) {
    const index = MOCK_FIXTURES.findIndex((fixture) => fixture.id === id);
    if (index >= 0) MOCK_FIXTURES.splice(index, 1);
    return Promise.resolve();
  }
  return invoke<void>("delete_fixture", { id });
}

export function clearFixtures(): Promise<void> {
  if (!isTauri()) {
    MOCK_FIXTURES.splice(0, MOCK_FIXTURES.length);
    return Promise.resolve();
  }
  return invoke<void>("clear_fixtures");
}

export function fixtureToRule(id: string): Promise<ResponseRule> {
  if (!isTauri()) {
    const fixture = MOCK_FIXTURES.find((candidate) => candidate.id === id);
    if (!fixture) return Promise.reject(new Error("fixture를 찾을 수 없습니다"));
    return Promise.resolve({
      id: "",
      priority: 0,
      method: fixture.method,
      path: fixture.url,
      status: 200,
      headers: [],
      body: "",
      delayMs: 0,
    });
  }
  return invoke<ResponseRulePayload>("fixture_to_rule", { id }).then(normalizeResponseRule);
}

/** Replay only a backend-owned masked fixture to the local server. */
export function replayFixture(id: string): Promise<ReplayResult> {
  if (!isTauri()) return Promise.reject(new Error(REPLAY_BROWSER_ERROR));
  return invoke<ReplayResult>("replay_fixture", { id });
}

/** Send only a backend-owned masked history projection to API Playground. */
export function sendHistoryToApi(historyId: number): Promise<ApiHandoffDispatch> {
  if (!isTauri()) return Promise.reject(new Error(HANDOFF_BROWSER_ERROR));
  return invoke<ApiHandoffDispatch>("send_history_to_api", { historyId });
}

/** Send only a backend-owned masked fixture to API Playground. */
export function sendFixtureToApi(id: string): Promise<ApiHandoffDispatch> {
  if (!isTauri()) return Promise.reject(new Error(HANDOFF_BROWSER_ERROR));
  return invoke<ApiHandoffDispatch>("send_fixture_to_api", { id });
}

export function listRules(): Promise<ResponseRule[]> {
  if (!isTauri()) return Promise.resolve([]);
  return invoke<ResponseRulePayload[]>("list_rules").then((rules) => rules.map(normalizeResponseRule));
}

export function previewRuleConflicts(rule: ResponseRulePayload): Promise<RuleConflictPreview> {
  const normalizedRule = normalizeResponseRule(rule);
  if (!isTauri()) {
    return Promise.resolve({
      candidateId: normalizedRule.id || "mock-rule",
      conflicts: [],
      requiresConfirmation: false,
    });
  }
  return invoke<RuleConflictPreview>("preview_rule_conflicts", { rule: normalizedRule });
}

export function setRule(rule: ResponseRulePayload, confirmConflicts: boolean): Promise<string> {
  const normalizedRule = normalizeResponseRule(rule);
  if (!isTauri()) return Promise.resolve(normalizedRule.id || "mock-rule");
  return invoke<string>("set_rule", { rule: normalizedRule, confirmConflicts });
}

export function deleteRule(id: string): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("delete_rule", { id });
}

export function resetRuleSequence(id: string): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("reset_rule_sequence", { id });
}
