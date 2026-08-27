import { describe, expect, it } from "vitest";
import type { RequestTemplate, WebSocketMessage } from "../types";
import {
  buildWebSocketUrl,
  MAX_BUFFER_BYTES,
  MAX_CONTROL_PAYLOAD_BYTES,
  MAX_RETAINED_MESSAGES,
  WebSocketMessageBuffer,
  hexToBytes,
  makeBinaryMessage,
  makeTextMessage,
  toNativeMessageInput,
  validateCloseCode,
  validateCloseReason,
  validateWebSocketEndpoint,
} from "./websocket";

function request(overrides: Partial<RequestTemplate> = {}): RequestTemplate {
  return {
    method: "GET",
    url: "ws://127.0.0.1:9000/socket",
    headers: [],
    cookies: [],
    multipart: [],
    params: [],
    body_kind: "none",
    body: "",
    auth: { kind: "none", username: "", password: "", token: "", api_key: "", api_value: "" },
    timeout_ms: 10_000,
    ...overrides,
  };
}

function message(id: number, size: number): WebSocketMessage {
  return { id, direction: "received", kind: "binary", binarySize: size };
}

describe("WebSocket request bounds", () => {
  it("accepts only ws/wss endpoints and rejects userinfo, fragments and credentials", () => {
    expect(() => validateWebSocketEndpoint("ws://localhost:9000/socket")).not.toThrow();
    expect(() => validateWebSocketEndpoint("wss://localhost:9000/socket")).not.toThrow();
    for (const value of [
      "https://localhost/socket",
      "ws://user:password@localhost/socket",
      "ws://localhost/socket#fragment",
      "ws://localhost/socket?access_token=secret",
    ]) {
      expect(() => validateWebSocketEndpoint(value)).toThrow();
    }
  });

  it("URL-encodes ordinary params but rejects credential-shaped params", () => {
    expect(buildWebSocketUrl("ws://localhost/socket", [{ key: "q", value: "a b" }]))
      .toBe("ws://localhost/socket?q=a+b");
    expect(() => buildWebSocketUrl("ws://localhost/socket", [{ key: "token", value: "secret" }]))
      .toThrow("credential");
  });

  it("keeps message payload and control payload bounds separate", () => {
    expect(toNativeMessageInput("binary", "a".repeat(MAX_CONTROL_PAYLOAD_BYTES + 1))).toMatchObject({ kind: "binary" });
    expect(() => toNativeMessageInput("ping", "a".repeat(MAX_CONTROL_PAYLOAD_BYTES + 1))).toThrow("초과");
    expect(() => hexToBytes("abc")).toThrow();
  });

  it("validates RFC close code and UTF-8 reason limits", () => {
    expect(validateCloseCode(undefined)).toBe(1000);
    expect(validateCloseCode(3000)).toBe(3000);
    expect(() => validateCloseCode(1006)).toThrow();
    expect(() => validateCloseCode(2999)).toThrow();
    expect(() => validateCloseReason("x".repeat(124))).toThrow();
    expect(() => validateCloseReason("bad\0reason")).toThrow();
  });
});

describe("WebSocket message projection and retention", () => {
  it("masks request secrets from text and binary previews", () => {
    const req = request({
      auth: { kind: "bearer", username: "", password: "", token: "binary-secret", api_key: "", api_value: "" },
      headers: [{ key: "X-Trace", value: "safe", enabled: true }],
    });
    const text = makeTextMessage(1, "received", "echo binary-secret", req);
    const binary = makeBinaryMessage(2, "received", new TextEncoder().encode("binary-secret"), req);
    expect(text.text).toBe("echo [REDACTED]");
    expect(binary.binaryHex).toBe("[REDACTED]");
    expect(binary.binaryText).toBe("[REDACTED]");
  });

  it("masks known token patterns from binary hex previews", () => {
    const req = request();
    const binary = makeBinaryMessage(1, "received", new TextEncoder().encode("ghp_1234567890abcdef"), req);
    expect(binary.binaryHex).toBe("[REDACTED]");
  });

  it("masks sensitive JSON fields from text and binary UTF-8 previews", () => {
    const req = request();
    const value = '{"token":"server-only-secret","value":"safe"}';
    expect(makeTextMessage(1, "received", value, req).text).toContain("[REDACTED]");
    const binary = makeBinaryMessage(2, "received", new TextEncoder().encode(value), req);
    expect(binary.binaryText).toContain("[REDACTED]");
    expect(binary.binaryHex).toBe("[REDACTED]");
  });

  it("evicts oldest messages by count and cumulative bytes", () => {
    const byCount = new WebSocketMessageBuffer();
    for (let id = 1; id <= MAX_RETAINED_MESSAGES + 1; id += 1) byCount.push(message(id, 1));
    expect(byCount.messages).toHaveLength(MAX_RETAINED_MESSAGES);
    expect(byCount.messages[0].id).toBe(2);
    expect(byCount.evicted).toBe(1);

    const byBytes = new WebSocketMessageBuffer();
    byBytes.push(message(1, MAX_BUFFER_BYTES));
    byBytes.push(message(2, 1));
    expect(byBytes.messages.map((item) => item.id)).toEqual([2]);
    expect(byBytes.bytes).toBe(1);
  });
});
