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

const MOCK_HISTORY: RequestRecord[] = [
  { id: 1, method: "POST", url: "/hook", headers: [["content-type", "application/json"]], body: '{"event":"push"}', receivedAtMs: Date.now() - 30000 },
  { id: 2, method: "GET", url: "/health", headers: [], body: "", receivedAtMs: Date.now() - 10000 },
];

export function serverStatus(): Promise<ServerStatus> {
  if (!isTauri()) return Promise.resolve({ running: false, address: null });
  return invoke<ServerStatus>("server_status");
}

export function startServer(bind: string | null, port: number): Promise<ServerStatus> {
  if (!isTauri()) return Promise.resolve({ running: true, address: `${bind ?? "127.0.0.1"}:${port}` });
  return invoke<ServerStatus>("start_server", { bind, port });
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
