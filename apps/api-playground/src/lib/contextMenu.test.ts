import { describe, expect, it } from "vitest";
import type { HistoryItem, RequestTemplate } from "../types";
import {
  buildRequestItemContextMenu,
  duplicateHistoryItem,
  removeHistoryItem,
  renameHistoryItem,
} from "./contextMenu";
import { emptyHistoryStore, REDACTED, sanitizeRequestForPersistence } from "./persistence";

function request(): RequestTemplate {
  return {
    method: "POST",
    url: "https://api.example.com/items?token=direct-url-secret",
    headers: [{ key: "Authorization", value: "Bearer direct-header-secret" }],
    cookies: [{ name: "session", value: "direct-cookie-secret" }],
    params: [],
    body_kind: "json",
    body: JSON.stringify({ password: "direct-body-secret" }),
    auth: null,
    timeout_ms: 30_000,
  };
}

function historyItem(): HistoryItem {
  return {
    id: "h-1",
    saved_at: 1_000,
    request: sanitizeRequestForPersistence(request()),
    status: 200,
  };
}

describe("API Playground request item context menu", () => {
  it("설계의 정확한 네 항목과 danger 경계를 유지한다", () => {
    const items = buildRequestItemContextMenu(false);
    expect(items.map((item) => item.type === "item" ? item.label : "separator")).toEqual([
      "복제",
      "이름 변경",
      "삭제",
      "curl 복사",
    ]);
    const deletion = items.find((item) => item.type === "item" && item.id === "delete");
    expect(deletion?.type).toBe("item");
    expect(deletion?.type === "item" && deletion.danger).toBe(true);
    expect(items.every((item) => item.type !== "item" || item.disabled === false)).toBe(true);
  });

  it("저장 작업 중에는 모든 action을 비활성화한다", () => {
    expect(buildRequestItemContextMenu(true).every((item) => item.type !== "item" || item.disabled))
      .toBe(true);
  });

  it("History 복제는 저장된 마스킹 request만 깊은 복사한다", () => {
    const source = historyItem();
    const store = { ...emptyHistoryStore(), history: [source] };
    const next = duplicateHistoryItem(store, source.id, 2_000, () => "h-copy");

    expect(next.history).toHaveLength(2);
    expect(next.history[0]).toMatchObject({ id: "h-copy", saved_at: 2_000, name: expect.stringContaining("복사본") });
    expect(next.history[0].request).not.toBe(source.request);
    expect(next.history[0].request.headers).not.toBe(source.request.headers);
    expect(next.history[0].request.cookies).not.toBe(source.request.cookies);
    expect(JSON.stringify(next)).not.toContain("direct-header-secret");
    expect(next.history[0].request.headers[0].value).toBe(REDACTED);
    expect(next.history[0].request.cookies[0].value).toBe(REDACTED);
  });

  it("History 이름은 한 줄·120자로 제한하고 exact ID만 삭제한다", () => {
    const source = historyItem();
    const other = { ...historyItem(), id: "h-2" };
    const store = { ...emptyHistoryStore(), history: [source, other] };
    const renamed = renameHistoryItem(store, source.id, `  새\n이름${"x".repeat(200)}  `);

    expect(renamed.history[0].name).not.toContain("\n");
    expect(renamed.history[0].name?.length).toBe(120);
    expect(renamed.history[1].name).toBeUndefined();
    expect(removeHistoryItem(renamed, source.id).history.map((item) => item.id)).toEqual(["h-2"]);
  });
});
