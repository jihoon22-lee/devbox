// @vitest-environment node
// v0.9.0 `requests/lib/websocket.test.ts`·`sse.test.ts`의 표시·상한 순수 시험 선택 이식(적응) + v1 연결 파라미터.
// 받은 메시지 가림은 데몬이 이 연결에서 푼 비밀만 가린다(S5-2 응답 가림과 같은 규칙, Rust `realtime.rs`).
import { describe, expect, it } from "vitest";
import type { RealtimeEvent } from "../../../../rpc/gen/rpc";
import { row } from "../model/draft";
import { Session } from "../runner/session";
import { ENV_REVIEW_BLOCK } from "../useSend";
import {
  apply,
  closeInput,
  compose,
  emptyLog,
  MAX_ENTRIES,
  MAX_ENTRY_BYTES,
  MAX_MESSAGE_BYTES,
  sseParams,
  type Target,
  targetVariables,
  wsParams,
} from "./messages";

const received = (data: string, atMs = 1): RealtimeEvent => ({
  type: "message",
  direction: "received",
  kind: "text",
  data,
  dataBase64: null,
  atMs,
});
const target = (extra: Partial<Target> = {}): Target => ({
  url: "wss://x.test/socket",
  headers: [],
  tls: { verify: true, caPath: null, clientCertPath: null, clientKeyPath: null },
  ...extra,
});

describe("message log", () => {
  it("evicts oldest messages by count and cumulative bytes", () => {
    let byCount = emptyLog();
    for (let n = 0; n <= MAX_ENTRIES; n += 1) byCount = apply(byCount, received(String(n)));
    expect(byCount.entries).toHaveLength(MAX_ENTRIES);
    expect(byCount.entries[0]!.text).toBe("1");
    expect(byCount.trimmed).toBe(1);

    let byBytes = emptyLog();
    byBytes = apply(byBytes, received("x".repeat(MAX_ENTRY_BYTES)));
    byBytes = apply(byBytes, received("y"));
    expect(byBytes.entries.map((e) => e.text)).toEqual(["y"]);
    expect(byBytes.bytes).toBe(1);
  });

  it("evicts oldest events at the retained history bound", () => {
    let log = emptyLog();
    for (let n = 0; n <= MAX_ENTRIES; n += 1)
      log = apply(log, { type: "sseEvent", id: null, event: "message", data: String(n), retryMs: null, atMs: 0 });
    expect(log.entries).toHaveLength(MAX_ENTRIES);
    expect(log.trimmed).toBe(1);
  });

  it("dropped_notices_add_up_and_status_follows_the_session", () => {
    let log = apply(emptyLog(), { type: "open", protocol: "chat" });
    expect(log.status).toBe("open");
    expect(log.protocol).toBe("chat");
    log = apply(log, { type: "dropped", count: 3 });
    log = apply(log, { type: "dropped", count: 2 });
    expect(log.dropped).toBe(5);
    expect(log.entries.at(-1)!.text).toContain("메시지 2개를 버렸습니다");
    log = apply(log, { type: "reconnecting", attempt: 2, lastEventId: "7" });
    expect(log.status).toBe("reconnecting");
    log = apply(log, { type: "closed", code: 4001, reason: "done" });
    expect(log.status).toBe("closed");
    expect(log.entries.at(-1)!.text).toBe("연결이 닫혔습니다 (4001 done)");
    log = apply(log, { type: "failed", error: { code: "api.network", detail: { kind: "tls" } } });
    expect(log.failure).toEqual({ code: "api.network", detail: { kind: "tls" } });
  });

  it("binary_messages_show_a_hex_preview_and_their_size", () => {
    const log = apply(emptyLog(), {
      type: "message",
      direction: "received",
      kind: "binary",
      data: null,
      dataBase64: btoa(String.fromCharCode(0, 159, 255)),
      atMs: 0,
    });
    expect(log.entries[0]).toMatchObject({ kind: "message", binary: true, text: "00 9f ff", bytes: 3 });
  });
});

describe("composer", () => {
  it("keeps message payload bounds and rejects broken JSON and Base64", () => {
    expect(compose("text", "hi")).toEqual({ input: { type: "text", data: "hi" } });
    expect(compose("json", '{"a":1}')).toEqual({ input: { type: "text", data: '{"a":1}' } });
    expect(compose("json", "{a:1}")).toEqual({ error: "JSON 형식이 올바르지 않습니다." });
    expect(compose("binary", "AJ// /w==")).toEqual({ input: { type: "binary", dataBase64: "AJ///w==" } });
    expect(compose("binary", "abc")).toEqual({ error: "Base64 형식이 아닙니다." });
    expect("error" in compose("text", "x".repeat(MAX_MESSAGE_BYTES + 1))).toBe(true);
    expect("input" in compose("text", "x".repeat(MAX_MESSAGE_BYTES))).toBe(true);
  });

  it("validates RFC close code and UTF-8 reason limits", () => {
    expect(closeInput("", "")).toEqual({ input: { type: "close", code: 1000, reason: null } });
    expect(closeInput("3000", "bye")).toEqual({ input: { type: "close", code: 3000, reason: "bye" } });
    for (const code of ["1006", "1005", "2999", "5000", "abc"]) expect("error" in closeInput(code, "")).toBe(true);
    expect("error" in closeInput("1000", "x".repeat(124))).toBe(true);
    expect("error" in closeInput("1000", "가".repeat(42))).toBe(true);
    expect("error" in closeInput("1000", "bad\0reason")).toBe(true);
  });
});

describe("connection params", () => {
  it("send_only_enabled_headers_with_screen_rows_and_subprotocol_tokens", () => {
    const t = target({ headers: [row("X-Off", "1", false), row("Authorization", "Bearer {{secret:GH}}")] });
    const ws = wsParams(t, " chat, ,v2 ", []);
    expect(ws.headers).toEqual([{ row: 1, name: "Authorization", value: "Bearer {{secret:GH}}" }]);
    expect(ws.subprotocols).toEqual(["chat", "v2"]);
    expect(ws.origin).toBeNull();
    expect(sseParams(t, "  ", []).lastEventId).toBeNull();
    expect(sseParams(t, "7", []).lastEventId).toBe("7");
  });

  it("variables_follow_the_editor_rules_and_unreviewed_environments_are_not_used", () => {
    const t = target({ url: "{{base}}/socket", headers: [row("X-Key", "{{key}}")] });
    const session = new Session();
    const reviewed = {
      name: "dev",
      variables: [
        { name: "base", value: "wss://x.test" },
        { name: "key", value: "{{secret:GH}}" },
      ],
    };
    const result = targetVariables(t, reviewed, session);
    expect(result).toEqual({
      variables: [
        { name: "base", value: "wss://x.test", sensitive: false, origin: "environment" },
        { name: "key", value: "{{secret:GH}}", sensitive: true, origin: "environment" },
      ],
    });
    expect(targetVariables(t, { ...reviewed, unreviewed: true }, session)).toEqual({ blocked: ENV_REVIEW_BLOCK });
    // 쓰지 않는 미검토 환경은 막지 않는다.
    expect(targetVariables(target(), { ...reviewed, unreviewed: true }, session)).toEqual({ variables: [] });
  });
});
