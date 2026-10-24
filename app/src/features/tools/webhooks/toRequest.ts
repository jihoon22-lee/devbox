import type { WebhookReceivedDetail } from "../../../rpc/gen/rpc";
import { emptyDraft, type RequestDraft, row } from "../api/model/draft";

const HOP = [
  "connection",
  "keep-alive",
  "proxy-authenticate",
  "proxy-authorization",
  "te",
  "trailer",
  "transfer-encoding",
  "upgrade",
  "content-length",
  "host",
];
/** Conversion only. The regular editor owns explicit sending and storage sanitization. */
export function toRequest(detail: WebhookReceivedDetail, origin: string): RequestDraft {
  if (detail.previewTruncated || detail.received.truncated) throw new Error("잘린 요청은 다시 보낼 수 없습니다.");
  const excluded = new Set(HOP);
  for (const [name, value] of detail.headers)
    if (name.toLowerCase() === "connection")
      for (const token of value.split(",")) excluded.add(token.trim().toLowerCase());
  const draft = emptyDraft();
  const base = new URL(origin);
  if (!["http:", "https:"].includes(base.protocol)) throw new Error("HTTP 주소가 필요합니다.");
  draft.url = `${base.origin}${detail.received.path}${detail.query ? `?${detail.query}` : ""}`;
  draft.method = detail.received.method;
  draft.headers = detail.headers
    .filter(([name]) => !excluded.has(name.toLowerCase()))
    .map(([name, value]) => row(name, value));
  draft.bodyKind = "raw";
  const bytes = Uint8Array.from(atob(detail.bodyBase64), (c) => c.charCodeAt(0));
  const contentType = detail.headers.find(([n]) => n.toLowerCase() === "content-type")?.[1] ?? "";
  try {
    draft.bodies.raw = { text: new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes), contentType };
  } catch {
    draft.bodies.raw = { text: detail.bodyBase64, contentType, encoding: "base64" };
  }
  return draft;
}
