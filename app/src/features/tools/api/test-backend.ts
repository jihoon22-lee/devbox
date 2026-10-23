/** 화면 시험용 가짜 데몬: 메모리 파일·digest CAS·송신 기록. */
import { vi } from "vitest";
import type { CollectionExpect, CollectionRoot, HistoryPage, SendRequest } from "../../../rpc/gen/rpc";
import { type ApiBackend, SendFailure, type SentResponse } from "./backend";

const key = (root: CollectionRoot, path: string) => `${root.kind === "project" ? root.projectId : "~"}:${path}`;

export function textResponse(body: string, status = 200, extra: Partial<SentResponse> = {}): SentResponse {
  return {
    historyId: "h1",
    sentAtMs: 1_700_000_000_000,
    status,
    reason: status === 200 ? "OK" : "Error",
    httpVersion: "HTTP/1.1",
    headers: [{ name: "content-type", value: "application/json" }],
    body: new TextEncoder().encode(body),
    truncated: false,
    elapsedMs: 12,
    sizeBytes: body.length,
    binary: false,
    ...extra,
  };
}

export class FakeBackend implements ApiBackend {
  files = new Map<string, { text: string; digest: string }>();
  secrets = new Map<string, string>();
  reviewed = new Set<string>();
  failWrite = new Set<string>();
  private next = 0;
  reply: (request: SendRequest, signal: AbortSignal) => Promise<SentResponse> = async () => textResponse("{}");

  seed(root: CollectionRoot, path: string, text: string, digest = `d${++this.next}`) {
    this.files.set(key(root, path), { text, digest });
    return digest;
  }
  text(root: CollectionRoot, path: string) {
    return this.files.get(key(root, path))?.text;
  }
  list = vi.fn(async (root: CollectionRoot) => ({
    root,
    items: [...this.files]
      .filter(([k]) => k.startsWith(key(root, "")))
      .map(([k, file]) => ({
        path: k.slice(key(root, "").length),
        size: file.text.length,
        digest: file.digest,
        reviewed: root.kind === "personal" || this.reviewed.has(`${k}@${file.digest}`),
        unreadable: false,
      })),
    truncated: false,
  }));
  read = vi.fn(async (root: CollectionRoot, path: string) => {
    const file = this.files.get(key(root, path));
    if (!file) throw new SendFailure("api.collection_not_found");
    const reviewed = root.kind === "personal" || this.reviewed.has(`${key(root, path)}@${file.digest}`);
    return { text: file.text, digest: file.digest, reviewed };
  });
  write = vi.fn(async (root: CollectionRoot, path: string, text: string, expect: CollectionExpect) => {
    const current = this.files.get(key(root, path));
    if (this.failWrite.has(path)) throw new SendFailure("api.collection_conflict", { digest: null });
    if (expect.kind === "absent" ? current : current?.digest !== expect.digest)
      throw new SendFailure("api.collection_conflict", { digest: current?.digest ?? null });
    const digest = `d${++this.next}`;
    this.files.set(key(root, path), { text, digest });
    return { digest, rev: String(this.next) };
  });
  remove = vi.fn(async (root: CollectionRoot, path: string, digest: string) => {
    const current = this.files.get(key(root, path));
    if (!current || current.digest !== digest)
      throw new SendFailure("api.collection_conflict", { digest: current?.digest ?? null });
    this.files.delete(key(root, path));
  });
  review = vi.fn(async (root: CollectionRoot, path: string, digest: string) => {
    this.reviewed.add(`${key(root, path)}@${digest}`);
  });
  send = vi.fn((request: SendRequest, signal: AbortSignal) => this.reply(request, signal));
  history = vi.fn(async (): Promise<HistoryPage> => ({ items: [], next: null }));
  historyBody = vi.fn(async () => "");
  secretNames = vi.fn(async () => [...this.secrets.keys()]);
  setSecret = vi.fn(async (name: string, value: string) => {
    this.secrets.set(name, value);
  });
}
