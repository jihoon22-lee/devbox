import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { sanitizePersistedJson } from "./api";
import {
  COLLECTION_V2_LS_KEY,
  type CollectionStore,
} from "./lib/collections";
import {
  HISTORY_V2_LS_KEY,
  sanitizeRequestForPersistence,
  type HistoryStore,
} from "./lib/persistence";
import type { RequestTemplate } from "./types";

vi.mock("./api", () => ({
  ackApiRequest: vi.fn(),
  buildRevealedCurl: vi.fn(),
  claimApiRequest: vi.fn(),
  copyRawResponseCookies: vi.fn(),
  copyRawResponseHeaders: vi.fn(),
  discardCurrentResponse: vi.fn(async () => undefined),
  onOpenRequest: vi.fn(async () => () => undefined),
  readJsonFile: vi.fn(),
  renewApiRequest: vi.fn(),
  restoreApiRequest: vi.fn(),
  saveJsonFile: vi.fn(),
  saveResponseBinary: vi.fn(),
  sanitizePersistedJson: vi.fn(),
  sealSecret: vi.fn(),
  sendSelectionToToolbox: vi.fn(),
  sendRequest: vi.fn(),
  startSseStream: vi.fn(),
  takePendingOpen: vi.fn(async () => null),
}));

const RAW_SECRET = "direct-context-secret";
const sanitizePersistedJsonMock = vi.mocked(sanitizePersistedJson);
const confirmMock = vi.fn<(message?: string) => boolean>();
const promptMock = vi.fn<(message?: string, defaultValue?: string) => string | null>();
const writeTextMock = vi.fn<(value: string) => Promise<void>>();

function rawRequest(): RequestTemplate {
  return {
    method: "POST",
    url: "https://api.example.com/items?token=url-secret",
    headers: [
      { key: "Authorization", value: `Bearer ${RAW_SECRET}` },
      { key: "X-Request-Id", value: "request-123" },
    ],
    cookies: [],
    multipart: [],
    params: [],
    body_kind: "json",
    body: JSON.stringify({ password: "body-secret", safe: "value" }),
    auth: null,
    timeout_ms: 30_000,
  };
}

function seedStores() {
  const request = sanitizeRequestForPersistence(rawRequest());
  const history: HistoryStore = {
    version: 2,
    history: [{ id: "h-1", saved_at: 1_000, request, status: 200 }],
  };
  const collections: CollectionStore = {
    version: 2,
    collections: [{
      id: "c-1",
      name: "저장 요청",
      folder: "dev",
      saved_at: 1_000,
      request,
      requiresSecretReview: request.requiresSecretReview,
    }],
  };
  localStorage.setItem(HISTORY_V2_LS_KEY, JSON.stringify(history));
  localStorage.setItem(COLLECTION_V2_LS_KEY, JSON.stringify(collections));
}

async function renderReady() {
  render(<App />);
  const historyRow = await screen.findByLabelText(/^기록 항목: .*api\.example\.com/u) as HTMLButtonElement;
  const collectionRow = await screen.findByLabelText("컬렉션 항목: 저장 요청") as HTMLDivElement;
  const inlineDelete = screen.getByRole("button", { name: "저장 요청 컬렉션 삭제" }) as HTMLButtonElement;
  await waitFor(() => expect(inlineDelete.disabled).toBe(false));
  return { historyRow, collectionRow };
}

beforeEach(() => {
  localStorage.clear();
  seedStores();
  sanitizePersistedJsonMock.mockReset().mockImplementation(async (serialized) => serialized);
  confirmMock.mockReset().mockReturnValue(false);
  promptMock.mockReset().mockReturnValue(null);
  writeTextMock.mockReset().mockResolvedValue(undefined);
  Object.defineProperty(window, "confirm", { configurable: true, value: confirmMock });
  Object.defineProperty(window, "prompt", { configurable: true, value: promptMock });
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: writeTextMock },
  });
});

afterEach(() => cleanup());

it("초기 앱 셸에 구조적 접근성 위반이 없다", async () => {
  const { container } = render(<App />);
  await waitFor(() => expect(screen.getByText("API Playground")).toBeTruthy());
  await assertNoA11yViolations(container);
});

describe("API Playground History and Collection context menus", () => {
  it("Enter와 Space로 Collection을 선택하되 IME 조합 키는 무시한다", async () => {
    const { collectionRow } = await renderReady();

    fireEvent.keyDown(collectionRow, { key: "Enter", isComposing: true });
    expect(collectionRow.getAttribute("aria-current")).toBeNull();
    fireEvent.keyDown(collectionRow, { key: " " });
    expect(collectionRow.getAttribute("aria-current")).toBe("true");
  });

  it("우클릭한 History를 먼저 선택하고 정확한 네 항목을 표시한다", async () => {
    const { historyRow } = await renderReady();

    fireEvent.contextMenu(historyRow, { clientX: 20, clientY: 24 });

    expect(historyRow.getAttribute("aria-current")).toBe("true");
    for (const label of ["복제", "이름 변경", "삭제", "curl 복사"]) {
      expect(screen.getByRole("menuitem", { name: label })).toBeTruthy();
    }
    expect(screen.getByRole("menuitem", { name: "삭제" }).className).toContain("danger");
  });

  it("Shift+F10은 Collection 메뉴를 열고 마스킹 cURL 복사 뒤 포커스를 복원한다", async () => {
    const { collectionRow } = await renderReady();
    collectionRow.focus();

    fireEvent.keyDown(collectionRow, { key: "F10", code: "F10", shiftKey: true });
    fireEvent.click(screen.getByRole("menuitem", { name: "curl 복사" }));

    await waitFor(() => expect(writeTextMock).toHaveBeenCalledTimes(1));
    const copied = writeTextMock.mock.calls[0][0];
    expect(copied).toContain("[REDACTED]");
    expect(copied).not.toContain(RAW_SECRET);
    expect(copied).not.toContain("body-secret");
    await waitFor(() => expect(document.activeElement).toBe(collectionRow));
  });

  it("Menu 키로 History를 복제하고 저장소에 raw credential을 만들지 않는다", async () => {
    const { historyRow } = await renderReady();
    historyRow.focus();

    fireEvent.keyDown(historyRow, { key: "ContextMenu", code: "ContextMenu" });
    fireEvent.click(screen.getByRole("menuitem", { name: "복제" }));

    await screen.findByText(/복사본/u);
    const stored = localStorage.getItem(HISTORY_V2_LS_KEY) ?? "";
    expect(JSON.parse(stored).history).toHaveLength(2);
    expect(stored).not.toContain(RAW_SECRET);
    expect(stored).not.toContain("body-secret");
  });

  it("History와 Collection 이름 변경은 exact 항목만 갱신한다", async () => {
    const { historyRow, collectionRow } = await renderReady();
    promptMock.mockReturnValueOnce("내 History");

    fireEvent.contextMenu(historyRow);
    fireEvent.click(screen.getByRole("menuitem", { name: "이름 변경" }));
    await screen.findByText("내 History");

    promptMock.mockReturnValueOnce("내 Collection");
    fireEvent.contextMenu(collectionRow);
    fireEvent.click(screen.getByRole("menuitem", { name: "이름 변경" }));
    await screen.findByLabelText("컬렉션 항목: 내 Collection");

    expect(localStorage.getItem(HISTORY_V2_LS_KEY)).toContain("내 History");
    expect(localStorage.getItem(COLLECTION_V2_LS_KEY)).toContain("내 Collection");
  });

  it("삭제는 danger 확인 전 상태를 바꾸지 않고 승인된 exact 항목만 제거한다", async () => {
    const { historyRow, collectionRow } = await renderReady();

    fireEvent.contextMenu(historyRow);
    fireEvent.click(screen.getByRole("menuitem", { name: "삭제" }));
    expect(JSON.parse(localStorage.getItem(HISTORY_V2_LS_KEY) ?? "null").history).toHaveLength(1);

    confirmMock.mockReturnValueOnce(true);
    fireEvent.contextMenu(historyRow);
    fireEvent.click(screen.getByRole("menuitem", { name: "삭제" }));
    await waitFor(() => {
      expect(JSON.parse(localStorage.getItem(HISTORY_V2_LS_KEY) ?? "null").history).toHaveLength(0);
    });

    confirmMock.mockReturnValueOnce(true);
    fireEvent.contextMenu(collectionRow);
    fireEvent.click(screen.getByRole("menuitem", { name: "삭제" }));
    await waitFor(() => {
      expect(JSON.parse(localStorage.getItem(COLLECTION_V2_LS_KEY) ?? "null").collections).toHaveLength(0);
    });
    expect(screen.queryByLabelText("컬렉션 항목: 저장 요청")).toBeNull();
  });

  it("sanitizer·clipboard 실패는 raw 오류를 화면에 반향하지 않는다", async () => {
    const { historyRow } = await renderReady();
    writeTextMock.mockRejectedValueOnce(new Error(`Bearer ${RAW_SECRET}`));

    fireEvent.contextMenu(historyRow);
    fireEvent.click(screen.getByRole("menuitem", { name: "curl 복사" }));

    expect(await screen.findByText("마스킹된 cURL을 복사하지 못했습니다.")).toBeTruthy();
    expect(document.body.textContent?.includes(RAW_SECRET)).toBe(false);
  });
});
