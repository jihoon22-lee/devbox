/**
 * MCP 화면의 순수 로직(07b S5-5): `*.mcp.json` 읽기, 실행할 명령의 정확한 표시(셸 인용, env는 이름만),
 * 데몬 MCP 이벤트 → 화면 상태. 파일 형식 `{ version: 1, kind: "mcp", name, transport }`의 `transport`는 데몬
 * `profile::matches`가 찾는 모양과 같다(stdio `command`·`args`·`cwd`·`env`, HTTP `url`·`headers`).
 */
import type { McpEvent, McpList, McpTransport, TlsOptions } from "../../../../rpc/gen/rpc";

export const MCP_FILE_SUFFIX = ".mcp.json";

export interface FileRow {
  name: string;
  value: string;
  enabled: boolean;
}
export type FileTransport =
  | { kind: "stdio"; command: string; args: string[]; cwd: string | null; env: FileRow[] }
  | { kind: "http"; url: string; headers: FileRow[]; tls: TlsOptions };
export interface McpFile {
  name: string;
  transport: FileTransport;
}

const isObject = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);
const invalid = () => new Error("MCP 파일 형식이 올바르지 않습니다");

function rows(value: unknown): FileRow[] {
  if (value === undefined || value === null) return [];
  if (!Array.isArray(value)) throw invalid();
  return value.map((item) => {
    if (!isObject(item) || typeof item.name !== "string" || typeof item.value !== "string") throw invalid();
    return { name: item.name, value: item.value, enabled: item.enabled !== false };
  });
}

function tlsOf(value: unknown): TlsOptions {
  const tls = isObject(value) ? value : {};
  const path = (v: unknown) => (typeof v === "string" && v ? v : null);
  return {
    verify: tls.verify !== false,
    caPath: path(tls.caPath),
    clientCertPath: path(tls.clientCertPath),
    clientKeyPath: path(tls.clientKeyPath),
  };
}

export function parseMcpFile(text: string): McpFile {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    throw invalid();
  }
  if (!isObject(value) || value.version !== 1 || value.kind !== "mcp" || !isObject(value.transport)) throw invalid();
  const t = value.transport;
  const name = typeof value.name === "string" ? value.name : "MCP";
  if (t.kind === "stdio") {
    const args = t.args === undefined || t.args === null ? [] : t.args;
    if (typeof t.command !== "string" || !Array.isArray(args) || args.some((a) => typeof a !== "string"))
      throw invalid();
    if (t.cwd !== undefined && t.cwd !== null && typeof t.cwd !== "string") throw invalid();
    return {
      name,
      transport: { kind: "stdio", command: t.command, args, cwd: t.cwd ?? null, env: rows(t.env) },
    };
  }
  if (t.kind === "http" && typeof t.url === "string")
    return { name, transport: { kind: "http", url: t.url, headers: rows(t.headers), tls: tlsOf(t.tls) } };
  throw invalid();
}

/** 데몬 요청 모양. 꺼진 행은 보내지 않는다(데몬도 파일의 꺼진 행을 비교에서 뺀다). 값은 치환 전 그대로. */
export function toTransport(t: FileTransport): McpTransport {
  if (t.kind === "stdio")
    return {
      kind: "stdio",
      command: t.command,
      args: t.args,
      cwd: t.cwd,
      env: t.env.filter((r) => r.enabled).map(({ name, value }) => ({ name, value })),
    };
  return {
    kind: "http",
    url: t.url,
    headers: t.headers.flatMap((r, row) => (r.enabled ? [{ row, name: r.name, value: r.value }] : [])),
    tls: t.tls,
  };
}

const PLAIN = /^[A-Za-z0-9_./:=@%+,-]+$/;
const quote = (s: string) => (PLAIN.test(s) ? s : `'${s.replaceAll("'", `'\\''`)}'`);

/** 실행할 argv를 POSIX 셸 인용으로 정확히 보인다(빈 인자·공백·따옴표도 구분된다). */
export function commandLine(t: FileTransport): string {
  return t.kind === "stdio" ? [t.command, ...t.args].map(quote).join(" ") : t.url;
}

/** 검토 화면·확인 대화상자의 env: 켜진 행의 `이름=값`을 적힌 그대로(비밀 참조는 `{{secret:…}}`로, 풀지 않음). */
export function envPairs(t: FileTransport): string[] {
  return t.kind === "stdio" ? t.env.filter((r) => r.enabled).map((r) => `${r.name}=${r.value}`) : [];
}

/**
 * 실제로 쓸 작업 폴더(데몬 규칙과 같다): 적었으면 그 값, 비었으면 프로젝트 파일은 그 프로젝트 루트,
 * 초안·개인 파일은 없음(`null`, 실행하지 않음).
 */
export function effectiveCwd(t: FileTransport, project: { label: string } | null): string | null {
  if (t.kind !== "stdio") return null;
  if (t.cwd) return t.cwd;
  return project ? `프로젝트 루트(${project.label})` : null;
}

export interface McpItem {
  /** 도구·프롬프트 이름 또는 리소스 URI. */
  name: string;
  description: string;
}
export interface McpResult {
  id: string;
  label: string;
  json?: string;
  error?: string;
}
export type Pending = { kind: "list"; what: McpList; append?: boolean } | { kind: "call"; label: string };
export interface McpState {
  status: "connecting" | "ready" | "failed" | "closed";
  server: { name: string; version: string; protocol: string } | null;
  authorizeUrl: string | null;
  tools: McpItem[];
  resources: McpItem[];
  prompts: McpItem[];
  /** 목록마다 다음 쪽 커서(`nextCursor`). */
  next: Partial<Record<McpList, string>>;
  pending: Record<string, Pending>;
  /** 최근 것이 앞. */
  results: McpResult[];
  failure: { code: string; detail?: unknown } | null;
}

const MAX_RESULTS = 50;
const MAX_ITEMS = 1_000;

export function initialMcp(): McpState {
  return {
    status: "connecting",
    server: null,
    authorizeUrl: null,
    tools: [],
    resources: [],
    prompts: [],
    next: {},
    pending: {},
    results: [],
    failure: null,
  };
}

function items(json: string, what: McpList): { items: McpItem[]; next?: string } {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    return { items: [] };
  }
  if (!isObject(value)) return { items: [] };
  const list = Array.isArray(value[what]) ? (value[what] as unknown[]) : [];
  const key = what === "resources" ? "uri" : "name";
  const out = list.flatMap((item) =>
    isObject(item) && typeof item[key] === "string"
      ? [{ name: item[key], description: typeof item.description === "string" ? item.description : "" }]
      : [],
  );
  return { items: out, ...(typeof value.nextCursor === "string" ? { next: value.nextCursor } : {}) };
}

function settle(state: McpState, id: string): [McpState, Pending | undefined] {
  const { [id]: pending, ...rest } = state.pending;
  return [{ ...state, pending: rest }, pending];
}

const pushResult = (state: McpState, result: McpResult) => ({
  ...state,
  results: [result, ...state.results].slice(0, MAX_RESULTS),
});

export function applyMcp(state: McpState, event: McpEvent): McpState {
  switch (event.type) {
    case "authorize":
      return { ...state, authorizeUrl: event.url };
    case "ready":
      return {
        ...state,
        status: "ready",
        authorizeUrl: null,
        server: { name: event.serverName, version: event.serverVersion, protocol: event.protocolVersion },
      };
    case "result": {
      const [next, pending] = settle(state, event.id);
      if (pending?.kind === "list") {
        const page = items(event.json, pending.what);
        const { [pending.what]: _, ...cursors } = next.next;
        return {
          ...next,
          [pending.what]: [...(pending.append ? next[pending.what] : []), ...page.items].slice(0, MAX_ITEMS),
          next: page.next ? { ...cursors, [pending.what]: page.next } : cursors,
        };
      }
      return pushResult(next, { id: event.id, label: pending?.label ?? event.id, json: event.json });
    }
    case "error": {
      const [next, pending] = settle(state, event.id);
      const label = pending?.kind === "call" ? pending.label : pending ? `${pending.what} 목록` : event.id;
      return pushResult(next, { id: event.id, label, error: `${event.code} ${event.message}` });
    }
    case "failed":
      return { ...state, status: "failed", failure: { code: event.error.code, detail: event.error.detail } };
    case "closed":
      return state.status === "failed" ? state : { ...state, status: "closed" };
  }
}
