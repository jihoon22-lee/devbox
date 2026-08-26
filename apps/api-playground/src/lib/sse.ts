/**
 * Browser-side counterpart of the native SSE parser.
 *
 * Keep this module independent of React, Tauri, fetch and persistence so chunk-boundary and
 * malformed-input fixtures exercise the same contract in browser preview and desktop builds.
 */

export const MAX_DECODED_BYTES = 20 * 1024 * 1024;
export const MAX_RETAINED_EVENTS = 10_000;
export const MAX_LINE_BYTES = 64 * 1024;
export const MAX_FIELD_BYTES = 64 * 1024;
export const MAX_EVENT_NAME_BYTES = 256;
export const MAX_EVENT_DATA_BYTES = 1024 * 1024;
export const MAX_EVENT_ID_BYTES = 256;
export const MAX_RETRY_MS = 60_000;

export interface SseEvent {
  event: string;
  data: string;
  id?: string;
  retryMs?: number;
}

export type SseParseErrorCode =
  | "invalid-utf8"
  | "line-too-long"
  | "field-too-long"
  | "data-too-long"
  | "event-name-too-long"
  | "event-id-too-long"
  | "invalid-event-id"
  | "invalid-retry"
  | "stream-too-large";

const PARSE_MESSAGES: Record<SseParseErrorCode, string> = {
  "invalid-utf8": "SSE stream text is invalid",
  "line-too-long": "SSE stream line is too long",
  "field-too-long": "SSE stream field is too long",
  "data-too-long": "SSE event data is too large",
  "event-name-too-long": "SSE event name is too long",
  "event-id-too-long": "SSE event id is too long",
  "invalid-event-id": "SSE event id is invalid",
  "invalid-retry": "SSE retry value is invalid",
  "stream-too-large": "SSE stream is too large",
};

export class SseParseError extends Error {
  readonly code: SseParseErrorCode;

  constructor(code: SseParseErrorCode) {
    super(PARSE_MESSAGES[code]);
    this.name = "SseParseError";
    this.code = code;
  }
}

export class SseParser {
  private readonly decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: false });
  private line = "";
  private lineBytes = 0;
  private pendingCr = false;
  private streamStarted = false;
  private decodedBytesValue = 0;
  private eventName = "";
  private data = "";
  private dataBytes = 0;
  private lastEventId = "";
  private retryValue: number | undefined;
  private ready: SseEvent[] = [];

  get decodedBytes(): number {
    return this.decodedBytesValue;
  }

  get retryMs(): number | undefined {
    return this.retryValue;
  }

  feed(bytes: Uint8Array): SseEvent[] {
    this.decodedBytesValue += bytes.byteLength;
    if (!Number.isSafeInteger(this.decodedBytesValue) || this.decodedBytesValue > MAX_DECODED_BYTES) {
      throw new SseParseError("stream-too-large");
    }
    let text: string;
    try {
      text = this.decoder.decode(bytes, { stream: true });
    } catch {
      throw new SseParseError("invalid-utf8");
    }
    this.consumeText(text);
    return this.takeEvents();
  }

  finish(): SseEvent[] {
    try {
      this.consumeText(this.decoder.decode());
    } catch {
      throw new SseParseError("invalid-utf8");
    }
    if (this.pendingCr) this.pendingCr = false;
    if (this.line) this.finishLine();
    this.dispatchToReady();
    return this.takeEvents();
  }

  private consumeText(text: string): void {
    if (!text) return;
    let characters = [...text];
    if (!this.streamStarted) {
      this.streamStarted = true;
      if (characters[0] === "\ufeff") characters = characters.slice(1);
    }
    for (const character of characters) {
      if (this.pendingCr) {
        this.pendingCr = false;
        if (character === "\n") continue;
      }
      if (character === "\r") {
        this.finishLine();
        this.pendingCr = true;
      } else if (character === "\n") {
        this.finishLine();
      } else {
        this.line += character;
        this.lineBytes += utf8CharBytes(character);
        if (this.lineBytes > MAX_LINE_BYTES) throw new SseParseError("line-too-long");
      }
    }
  }

  private takeEvents(): SseEvent[] {
    const events = this.ready;
    this.ready = [];
    return events;
  }

  private finishLine(): void {
    const line = this.line;
    this.line = "";
    this.lineBytes = 0;
    if (!line) {
      this.dispatchToReady();
      return;
    }
    if (line.startsWith(":")) return;
    const separator = line.indexOf(":");
    const field = separator < 0 ? line : line.slice(0, separator);
    if (!field || utf8Bytes(field) > MAX_FIELD_BYTES) throw new SseParseError("field-too-long");
    let value = separator < 0 ? "" : line.slice(separator + 1);
    if (value.startsWith(" ")) value = value.slice(1);
    if (utf8Bytes(value) > MAX_FIELD_BYTES) throw new SseParseError("field-too-long");

    switch (field) {
      case "event":
        if (utf8Bytes(value) > MAX_EVENT_NAME_BYTES) throw new SseParseError("event-name-too-long");
        if (value.includes("\0")) throw new SseParseError("invalid-event-id");
        this.eventName = value;
        break;
      case "data": {
        const next = this.dataBytes + utf8Bytes(value) + 1;
        if (next > MAX_EVENT_DATA_BYTES) throw new SseParseError("data-too-long");
        this.data += value + "\n";
        this.dataBytes = next;
        break;
      }
      case "id":
        if (utf8Bytes(value) > MAX_EVENT_ID_BYTES) throw new SseParseError("event-id-too-long");
        if (value.includes("\0")) throw new SseParseError("invalid-event-id");
        this.lastEventId = value;
        break;
      case "retry":
        this.retryValue = parseRetry(value);
        break;
      default:
        // Extension fields are ignored by the SSE specification after applying the bounds.
        break;
    }
  }

  private dispatchToReady(): void {
    if (!this.data) {
      this.eventName = "";
      this.dataBytes = 0;
      return;
    }
    if (this.data.endsWith("\n")) this.data = this.data.slice(0, -1);
    this.ready.push({
      event: this.eventName || "message",
      data: this.data,
      ...(this.lastEventId ? { id: this.lastEventId } : {}),
      ...(this.retryValue === undefined ? {} : { retryMs: this.retryValue }),
    });
    this.eventName = "";
    this.data = "";
    this.dataBytes = 0;
  }
}

function parseRetry(value: string): number {
  if (!value || !/^\d+$/u.test(value)) throw new SseParseError("invalid-retry");
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed > MAX_RETRY_MS) throw new SseParseError("invalid-retry");
  return parsed;
}

function utf8Bytes(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function utf8CharBytes(value: string): number {
  const codePoint = value.codePointAt(0) ?? 0;
  if (codePoint <= 0x7f) return 1;
  if (codePoint <= 0x7ff) return 2;
  if (codePoint <= 0xffff) return 3;
  return 4;
}

export function eventSize(event: SseEvent): number {
  return utf8Bytes(event.event) + utf8Bytes(event.data) + utf8Bytes(event.id ?? "") + 32;
}

export class SseEventBuffer {
  private eventsValue: SseEvent[] = [];
  private bytesValue = 0;
  private evictedValue = 0;

  push(event: SseEvent): number {
    this.eventsValue.push(event);
    this.bytesValue += eventSize(event);
    let removed = 0;
    while (this.eventsValue.length > MAX_RETAINED_EVENTS || this.bytesValue > MAX_DECODED_BYTES) {
      const oldest = this.eventsValue.shift();
      if (!oldest) {
        this.bytesValue = 0;
        break;
      }
      this.bytesValue -= eventSize(oldest);
      removed += 1;
      this.evictedValue += 1;
    }
    return removed;
  }

  get events(): readonly SseEvent[] {
    return this.eventsValue;
  }

  get bytes(): number {
    return this.bytesValue;
  }

  get evicted(): number {
    return this.evictedValue;
  }
}
