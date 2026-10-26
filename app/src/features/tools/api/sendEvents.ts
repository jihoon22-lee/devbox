/** `api.send` 스트림의 NDJSON 이벤트(`SendEvent`)를 응답 하나로 모은다. 화면 메모리의 본문은 10 MiB까지다. */
import type { SendEvent } from "../../../rpc/gen/rpc";
import { type SendAuthorization, SendFailure, type SentResponse } from "./backend";
import type { ApiResponse } from "./lib/template";

export const MAX_SCREEN_BODY_BYTES = 10 * 1024 * 1024;

function base64(text: string): Uint8Array {
  const binary = atob(text);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i);
  return out;
}

export class SendCollector {
  private decoder = new TextDecoder();
  private pending = "";
  private chunks: Uint8Array[] = [];
  private kept = 0;
  private response: SentResponse = {
    historyId: null,
    sentAtMs: Date.now(),
    status: 0,
    reason: "",
    httpVersion: "",
    headers: [],
    body: new Uint8Array(),
    truncated: false,
    elapsedMs: 0,
    sizeBytes: 0,
    binary: false,
  };
  ended = false;
  failure: SendFailure | null = null;

  constructor(private onAuthorize?: (event: SendAuthorization | null) => void) {}

  push(bytes: Uint8Array): void {
    this.pending += this.decoder.decode(bytes, { stream: true });
    let at = this.pending.indexOf("\n");
    while (at >= 0) {
      const line = this.pending.slice(0, at).trim();
      this.pending = this.pending.slice(at + 1);
      if (line) this.event(JSON.parse(line) as SendEvent);
      at = this.pending.indexOf("\n");
    }
  }

  private event(event: SendEvent): void {
    if (this.ended || this.failure) return;
    const r = this.response;
    switch (event.type) {
      case "started":
        if (r.historyId !== null || typeof event.historyId !== "string" || !event.historyId) {
          this.failure = new SendFailure("internal");
          return;
        }
        r.historyId = event.historyId;
        r.sentAtMs = event.sentAtMs;
        break;
      case "head":
        this.onAuthorize?.(null);
        Object.assign(r, {
          status: event.status,
          reason: event.reason,
          httpVersion: event.httpVersion,
          headers: event.headers,
          elapsedMs: event.elapsedMs,
        });
        break;
      case "body": {
        const data = base64(event.dataBase64);
        const room = MAX_SCREEN_BODY_BYTES - this.kept;
        if (data.length > room) r.truncated = true;
        if (room > 0) {
          const part = data.subarray(0, room);
          this.chunks.push(part);
          this.kept += part.length;
        }
        break;
      }
      case "end":
        this.onAuthorize?.(null);
        Object.assign(r, { sizeBytes: event.sizeBytes, elapsedMs: event.elapsedMs, binary: event.binary });
        this.ended = true;
        break;
      case "failed":
        this.onAuthorize?.(null);
        this.failure = new SendFailure(event.error.code, event.error.detail);
        break;
      case "authorize":
        if (!r.historyId || typeof event.url !== "string") this.failure = new SendFailure("internal");
        else this.onAuthorize?.({ historyId: r.historyId, url: event.url });
        break;
    }
  }

  finish(): SentResponse {
    const body = new Uint8Array(this.kept);
    let offset = 0;
    for (const chunk of this.chunks) {
      body.set(chunk, offset);
      offset += chunk.length;
    }
    return { ...this.response, body };
  }
}

export function bodyText(response: SentResponse): string {
  return new TextDecoder().decode(response.body);
}

/** assertion·캡처가 읽는 v0.9.0 응답 모양으로 바꾼다. */
export function toApiResponse(response: SentResponse): ApiResponse {
  const body = response.binary ? "" : bodyText(response);
  let isJson = false;
  try {
    JSON.parse(body);
    isJson = body.trim() !== "";
  } catch {}
  return {
    status: response.status,
    status_text: response.reason,
    headers: response.headers.map((h) => ({ key: h.name, value: h.value })),
    duration_ms: response.elapsedMs,
    size_bytes: response.sizeBytes,
    body,
    is_json: isJson,
    final_url: "",
    redirects: [],
    cookies: [],
    response_id: response.historyId,
    raw_headers_available: false,
    headers_truncated: false,
  };
}
