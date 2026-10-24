/**
 * WebSocket·SSE 화면의 순수 로직: 데몬 이벤트 → 표시 목록(상한 5,000개·16 MiB, 넘치면 오래된 것부터 지움),
 * 보내기 입력 검사(텍스트·JSON·이진 Base64, 닫기 코드·이유), 연결 파라미터 만들기.
 * 변수는 편집기 보내기와 같은 규칙(`sendVariables`), 검토하지 않은 프로젝트 환경의 값은 쓰지 않는다.
 */
import type { EnvVar, Pair, RealtimeEvent, SseOpen, TlsOptions, WsInput, WsOpen } from "../../../../rpc/gen/rpc";
import { messageFor } from "../../../../rpc/messages";
import { emptyDraft, type Row, toSnapshot } from "../model/draft";
import type { Session } from "../runner/session";
import { sendVariables } from "../sendState";
import type { ActiveEnvironment } from "../store";
import { ENV_REVIEW_BLOCK } from "../useSend";

export const MAX_ENTRIES = 5_000;
export const MAX_ENTRY_BYTES = 16 * 1024 * 1024;
/** 데몬 상한과 같다(`realtime/ws.rs` MAX_MESSAGE_BYTES). */
export const MAX_MESSAGE_BYTES = 4 * 1024 * 1024;
const MAX_CLOSE_REASON_BYTES = 123;

export type Entry =
  | {
      id: number;
      kind: "message";
      direction: "sent" | "received";
      binary: boolean;
      text: string;
      bytes: number;
      atMs: number;
    }
  | { id: number; kind: "event"; event: string; eventId: string | null; text: string; bytes: number; atMs: number }
  | { id: number; kind: "notice"; text: string; bytes: number; atMs: number | null };

type WithoutId<T> = T extends unknown ? Omit<T, "id"> : never;
type NewEntry = WithoutId<Entry>;

export type Status = "idle" | "connecting" | "open" | "reconnecting" | "closed" | "failed";

export interface Log {
  entries: Entry[];
  bytes: number;
  /** 화면 상한으로 지운 오래된 항목 수. */
  trimmed: number;
  /** 데몬이 화면이 느려 버렸다고 알린 메시지 수(합계). */
  dropped: number;
  status: Status;
  protocol: string | null;
  failure?: { code: string; detail?: unknown };
  next: number;
}

export const emptyLog = (): Log => ({
  entries: [],
  bytes: 0,
  trimmed: 0,
  dropped: 0,
  status: "idle",
  protocol: null,
  next: 1,
});

const utf8 = new TextEncoder();
export const byteLength = (text: string) => utf8.encode(text).length;

/** 이진 미리보기: 앞 64바이트 16진수(전체 크기 함께). */
export function hexPreview(base64: string): { text: string; bytes: number } {
  const raw = atob(base64);
  const shown = Array.from(raw.slice(0, 64), (c) => c.charCodeAt(0).toString(16).padStart(2, "0")).join(" ");
  return { text: raw.length > 64 ? `${shown} …` : shown, bytes: raw.length };
}

function push(log: Log, entry: NewEntry): Log {
  const entries = [...log.entries, { ...entry, id: log.next } as Entry];
  let bytes = log.bytes + entry.bytes;
  let trimmed = log.trimmed;
  let start = 0;
  while (entries.length - start > MAX_ENTRIES || (bytes > MAX_ENTRY_BYTES && entries.length - start > 1)) {
    bytes -= entries[start]!.bytes;
    start += 1;
    trimmed += 1;
  }
  return { ...log, entries: start ? entries.slice(start) : entries, bytes, trimmed, next: log.next + 1 };
}

const notice = (log: Log, text: string): Log =>
  push(log, { kind: "notice", text, bytes: byteLength(text), atMs: null });

/** 데몬 이벤트 하나를 반영한다. */
export function apply(log: Log, event: RealtimeEvent): Log {
  switch (event.type) {
    case "open":
      return notice(
        { ...log, status: "open", protocol: event.protocol },
        event.protocol ? `연결됨 (${event.protocol})` : "연결됨",
      );
    case "message": {
      const binary = event.kind === "binary";
      const preview = binary ? hexPreview(event.dataBase64 ?? "") : null;
      const text = preview ? preview.text : (event.data ?? "");
      return push(log, {
        kind: "message",
        direction: event.direction,
        binary,
        text,
        bytes: preview ? preview.bytes : byteLength(text),
        atMs: event.atMs,
      });
    }
    case "sseEvent":
      return push(log, {
        kind: "event",
        event: event.event ?? "message",
        eventId: event.id,
        text: event.data,
        bytes: byteLength(event.data),
        atMs: event.atMs,
      });
    case "reconnecting":
      return notice({ ...log, status: "reconnecting" }, `다시 연결하는 중 (${event.attempt}/5)`);
    case "dropped":
      return notice(
        { ...log, dropped: log.dropped + Number(event.count) },
        `화면이 따라가지 못해 메시지 ${event.count}개를 버렸습니다`,
      );
    case "closed":
      return notice(
        { ...log, status: "closed" },
        event.code === null
          ? "연결이 닫혔습니다"
          : `연결이 닫혔습니다 (${event.code}${event.reason ? ` ${event.reason}` : ""})`,
      );
    case "failed":
      return { ...log, status: "failed", failure: { code: event.error.code, detail: event.error.detail } };
  }
}

export type ComposeMode = "text" | "json" | "binary";

/** 보내기 입력 → `WsInput`. 잘못되면 화면 문구. */
export function compose(mode: ComposeMode, text: string): { input: WsInput } | { error: string } {
  if (mode === "binary") {
    const compact = text.replace(/\s+/g, "");
    if (!/^[A-Za-z0-9+/]*={0,2}$/.test(compact) || compact.length % 4 !== 0)
      return { error: "Base64 형식이 아닙니다." };
    if ((compact.length / 4) * 3 > MAX_MESSAGE_BYTES + 2) return { error: "메시지는 4 MiB까지 보낼 수 있습니다." };
    return { input: { type: "binary", dataBase64: compact } };
  }
  if (byteLength(text) > MAX_MESSAGE_BYTES) return { error: "메시지는 4 MiB까지 보낼 수 있습니다." };
  if (mode === "json") {
    try {
      JSON.parse(text);
    } catch {
      return { error: "JSON 형식이 올바르지 않습니다." };
    }
  }
  return { input: { type: "text", data: text } };
}

/** RFC 6455 닫기 코드(1004–1006·1015 예약, 응용 3000–4999)와 이유(123바이트) — 데몬 검사와 같다. */
export function closeInput(code: string, reason: string): { input: WsInput } | { error: string } {
  const value = code.trim() === "" ? 1000 : Number(code);
  const allowed =
    Number.isInteger(value) &&
    ((value >= 1000 && value <= 1003) || (value >= 1007 && value <= 1014) || (value >= 3000 && value <= 4999));
  if (!allowed) return { error: "닫기 코드는 1000–1003, 1007–1014, 3000–4999 중 하나여야 합니다." };
  if (byteLength(reason) > MAX_CLOSE_REASON_BYTES || reason.includes("\0"))
    return { error: "닫기 이유는 123바이트까지 쓸 수 있습니다." };
  return { input: { type: "close", code: value, reason: reason || null } };
}

export interface Target {
  url: string;
  headers: Row[];
  tls: TlsOptions;
}

/** 연결에 쓸 변수. 검토하지 않은 프로젝트 환경의 값을 써야 하면 막는다(편집기 보내기와 같은 규칙). */
export function targetVariables(
  target: Target,
  environment: ActiveEnvironment | null,
  session: Session,
): { variables: EnvVar[] } | { blocked: string } {
  const draft = { ...emptyDraft(), url: target.url, headers: target.headers, tls: target.tls };
  const variables = sendVariables(draft, (environment?.variables ?? []) as Pair[], session);
  if (environment?.unreviewed && variables.some((v) => v.origin === "environment"))
    return { blocked: ENV_REVIEW_BLOCK };
  return { variables };
}

function base(target: Target) {
  const snapshot = toSnapshot({ ...emptyDraft(), url: target.url, headers: target.headers, tls: target.tls });
  return { url: snapshot.url, headers: snapshot.headers, tls: snapshot.tls };
}

export function wsParams(target: Target, subprotocols: string, variables: EnvVar[]): WsOpen {
  return {
    ...base(target),
    subprotocols: subprotocols
      .split(",")
      .map((p) => p.trim())
      .filter(Boolean),
    env: { variables },
    origin: null,
  };
}

export function sseParams(target: Target, lastEventId: string, variables: EnvVar[]): SseOpen {
  return { ...base(target), env: { variables }, lastEventId: lastEventId.trim() || null, origin: null };
}

/** 데몬 `api.network` detail.kind → 원인 문구. 정해 둔 kind만 보이고 모르는 값은 옮기지 않는다. */
const NETWORK_CAUSES: Record<string, string> = {
  connect: "서버에 연결하지 못했습니다",
  timeout: "시간 안에 응답이 없었습니다",
  tls: "TLS 인증서를 확인하지 못했습니다",
  closed: "연결이 예기치 않게 끊겼습니다",
  protocol: "프로토콜 오류가 났습니다",
  reconnect_limit: "다시 연결 횟수 한도에 닿았습니다",
  sse_invalid_utf8: "이벤트가 UTF-8이 아닙니다",
  sse_line_too_long: "이벤트 줄이 너무 깁니다",
  sse_event_too_large: "이벤트 데이터가 너무 큽니다",
  sse_field_too_long: "이벤트 이름이나 id가 너무 깁니다",
  sse_invalid_id: "이벤트 id가 올바르지 않습니다",
  sse_invalid_retry: "retry 값이 올바르지 않습니다",
};

/** 실패 배너 문구: 오류 표의 제목·안내에 네트워크 원인을 덧붙인다. */
export function failureText(failure: { code: string; detail?: unknown }): string {
  const message = messageFor(failure.code as never, failure.detail);
  const base = message ? `${message.title}${message.action ? ` ${message.action}` : ""}` : "연결하지 못했습니다.";
  const kind =
    failure.code === "api.network" && typeof failure.detail === "object" && failure.detail !== null
      ? (failure.detail as { kind?: unknown }).kind
      : undefined;
  const cause = typeof kind === "string" && Object.hasOwn(NETWORK_CAUSES, kind) ? NETWORK_CAUSES[kind] : undefined;
  return cause ? `${base} (원인: ${cause})` : base;
}
