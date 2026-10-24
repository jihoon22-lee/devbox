/**
 * gRPC 화면의 순수 로직(07b S5-5): 메서드별 요청 초안(`대상\u0000메서드` 키, 필터가 선택을 바꿔도 지우지 않음, AR-F07),
 * 필터와 자동 선택, 호출 기록(호출은 만든 메서드에 묶이고 늦은 응답은 "<A>의 결과", AR-F08), `*.grpc.json` 파일 읽기.
 */
import type { GrpcCatalog, GrpcEvent, GrpcMethod, Pair, TlsOptions } from "../../../../rpc/gen/rpc";
import { type Row, row } from "../model/draft";

export const GRPC_FILE_SUFFIX = ".grpc.json";

export interface GrpcDraft {
  json: string;
  dirty: boolean;
}
export type Drafts = ReadonlyMap<string, GrpcDraft>;

export const emptyDrafts = (): Drafts => new Map();
export const draftKey = (target: string, method: string) => `${target}\u0000${method}`;

export function draftOf(drafts: Drafts, target: string, method: GrpcMethod): GrpcDraft {
  return drafts.get(draftKey(target, method.fullName)) ?? { json: method.inputDefault, dirty: false };
}

export function editDraft(drafts: Drafts, target: string, method: string, json: string): Drafts {
  const next = new Map(drafts);
  next.set(draftKey(target, method), { json, dirty: true });
  return next;
}

export function allMethods(catalog: GrpcCatalog | null): GrpcMethod[] {
  return catalog?.services.flatMap((s) => s.methods) ?? [];
}

/** 대소문자 무시 부분 일치(서비스·메서드 전체 이름). */
export function filterMethods(catalog: GrpcCatalog | null, filter: string): GrpcMethod[] {
  const needle = filter.trim().toLowerCase();
  const methods = allMethods(catalog);
  return needle ? methods.filter((m) => m.fullName.toLowerCase().includes(needle)) : methods;
}

/** 지금 선택이 보이면 그대로, 아니면 보이는 첫 메서드. 하나도 안 보이면 선택을 바꾸지 않는다. */
export function autoSelect(selected: string | null, visible: GrpcMethod[]): string | null {
  if (visible.length === 0) return selected;
  if (selected && visible.some((m) => m.fullName === selected)) return selected;
  return visible[0]!.fullName;
}

export type CallStatus = "running" | "ok" | "failed" | "cancelled";

export interface CallRecord {
  requestId: number;
  target: string;
  method: string;
  startedAtMs: number;
  status: CallStatus;
  headers: Pair[];
  messages: string[];
  trailers?: { code: number; message: string; metadata: Pair[] };
  failure?: { code: string; detail?: unknown };
}

/** 화면에 남기는 메시지 수(데몬 상한 10,000과 별개로 화면 메모리 보호). */
export const MAX_SHOWN_MESSAGES = 1_000;

export function newCall(requestId: number, target: string, method: string, startedAtMs: number): CallRecord {
  return { requestId, target, method, startedAtMs, status: "running", headers: [], messages: [] };
}

export function applyGrpc(call: CallRecord, event: GrpcEvent): CallRecord {
  switch (event.type) {
    case "headers":
      return { ...call, headers: event.metadata };
    case "message":
      return { ...call, messages: [...call.messages, event.json].slice(-MAX_SHOWN_MESSAGES) };
    case "trailers":
      return {
        ...call,
        status: call.status === "running" ? (event.code === 0 ? "ok" : "failed") : call.status,
        trailers: { code: event.code, message: event.message, metadata: event.metadata },
      };
    case "failed":
      return {
        ...call,
        status: call.status === "running" ? "failed" : call.status,
        failure: { code: event.error.code, detail: event.error.detail },
      };
  }
}

export function resultTitle(call: CallRecord, selected: string | null): string {
  return call.method === selected ? "결과" : `${call.method}의 결과`;
}

export const STATUS_TEXT: Record<CallStatus, string> = {
  running: "진행 중",
  ok: "성공",
  failed: "실패",
  cancelled: "취소",
};

export interface GrpcFile {
  name: string;
  target: string;
  metadata: Row[];
  tls: TlsOptions;
  protoFiles: string[];
  method: string | null;
  json: string | null;
}

const isObject = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);
const text = (v: unknown, fallback = ""): string => (typeof v === "string" ? v : fallback);

/** `{ version: 1, kind: "grpc", name, grpc: { target, metadata, tls, protoFiles, method?, json? } }`. */
export function parseGrpcFile(source: string): GrpcFile {
  const invalid = () => new Error("gRPC 파일 형식이 올바르지 않습니다");
  let value: unknown;
  try {
    value = JSON.parse(source);
  } catch {
    throw invalid();
  }
  if (!isObject(value) || value.version !== 1 || value.kind !== "grpc" || !isObject(value.grpc)) throw invalid();
  const grpc = value.grpc;
  const tls = isObject(grpc.tls) ? grpc.tls : {};
  const path = (v: unknown) => (typeof v === "string" && v ? v : null);
  const metadata = Array.isArray(grpc.metadata) ? grpc.metadata : [];
  const protoFiles = Array.isArray(grpc.protoFiles) ? grpc.protoFiles : [];
  if (typeof grpc.target !== "string" || protoFiles.some((p) => typeof p !== "string")) throw invalid();
  return {
    name: text(value.name, "gRPC"),
    target: grpc.target,
    metadata: metadata.filter(isObject).map((m) => ({
      ...row(text(m.name), text(m.value)),
      enabled: m.enabled !== false,
    })),
    tls: {
      verify: tls.verify !== false,
      caPath: path(tls.caPath),
      clientCertPath: path(tls.clientCertPath),
      clientKeyPath: path(tls.clientKeyPath),
    },
    protoFiles: protoFiles as string[],
    method: typeof grpc.method === "string" ? grpc.method : null,
    json: typeof grpc.json === "string" ? grpc.json : null,
  };
}
