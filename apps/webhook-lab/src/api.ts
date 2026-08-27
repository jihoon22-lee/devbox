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
  method: string | null;
  path: string;
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

const HANDOFF_BROWSER_ERROR =
  "API Playground handoff는 데스크톱 앱에서만 사용할 수 있습니다. 클립보드로 자동 전환하지 않습니다";

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
      method: fixture.method,
      path: fixture.url,
      status: 200,
      headers: [],
      body: "",
      delayMs: 0,
    });
  }
  return invoke<ResponseRule>("fixture_to_rule", { id });
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
  return invoke<ResponseRule[]>("list_rules");
}

export function setRule(rule: ResponseRule): Promise<string> {
  if (!isTauri()) return Promise.resolve(rule.id || "mock-rule");
  return invoke<string>("set_rule", { rule });
}

export function deleteRule(id: string): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("delete_rule", { id });
}
