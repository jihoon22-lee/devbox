import { browserDocumentStorage } from "../storage/documentStorage";
import { previewDocumentKey, seedPreviewDocument } from "../storage/testDocuments";
const COLLECTION_V2_LS_KEY = previewDocumentKey("collections");
const HISTORY_V2_LS_KEY = previewDocumentKey("history");
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  sanitizePersistedJson,
  pickCollectionFolder,
  writeCollectionFolder,
  sendRequest,
  revealCapture,
  sealSecret,
  authorizeOAuth2,
  oauth2Status,
} from "./api";
import { type CollectionStore } from "./lib/collections";
import { sanitizeRequestForPersistence, type HistoryStore } from "./lib/persistence";
import type { RequestTemplate } from "./types";

const nativeMode = vi.hoisted(() => ({ value: false }));
vi.mock("./lib/isTauri", () => ({ isTauri: () => nativeMode.value }));

vi.mock("./api", () => ({
  ackApiRequest: vi.fn(),
  buildRevealedCurl: vi.fn(),
  claimApiRequest: vi.fn(),
  copyRawResponseCookies: vi.fn(),
  copyRawResponseHeaders: vi.fn(),
  discardCurrentResponse: vi.fn(async () => undefined),
  onOpenRequest: vi.fn(async () => () => undefined),
  readJsonFile: vi.fn(),
  pickCollectionFolder: vi.fn(),
  writeCollectionFolder: vi.fn(),
  renewApiRequest: vi.fn(),
  restoreApiRequest: vi.fn(),
  saveJsonFile: vi.fn(),
  saveResponseBinary: vi.fn(),
  sanitizePersistedJson: vi.fn(),
  sealSecret: vi.fn(),
  oauth2Status: vi.fn(async () => ({ state: "missing", expiresAtMs: null, scope: null })),
  authorizeOAuth2: vi.fn(),
  cancelOAuth2: vi.fn(async () => {}),
  fetchOAuth2Token: vi.fn(),
  clearOAuth2Token: vi.fn(async () => {}),
  revealCapture: vi.fn(),
  discardCaptures: vi.fn(async () => {}),
  restoreCaptures: vi.fn(async () => {}),
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
    collections: [
      {
        id: "c-1",
        name: "저장 요청",
        folder: "dev",
        saved_at: 1_000,
        request,
        requiresSecretReview: request.requiresSecretReview,
      },
    ],
  };
  seedPreviewDocument("history", JSON.stringify(history));
  seedPreviewDocument("collections", JSON.stringify(collections));
}

async function renderReady() {
  render(<App />);
  const historyRow = (await screen.findByLabelText(/^기록 항목: .*api\.example\.com/u)) as HTMLButtonElement;
  const collectionRow = (await screen.findByLabelText("컬렉션 항목: 저장 요청")) as HTMLDivElement;
  const inlineDelete = screen.getByRole("button", { name: "저장 요청 컬렉션 삭제" }) as HTMLButtonElement;
  await waitFor(() => expect(inlineDelete.disabled).toBe(false));
  return { historyRow, collectionRow };
}

beforeEach(() => {
  nativeMode.value = false;
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

it("keeps rapid environment edits in order while the native-style save is delayed", async () => {
  seedPreviewDocument(
    "environments",
    JSON.stringify({
      version: 1,
      environments: [{ id: "e", name: "fixture-env", variables: [{ key: "VALUE", value: "initial", secret: false }] }],
    }),
  );
  await renderReady();
  fireEvent.click(screen.getByRole("button", { name: "fixture-env" }));
  const input = screen.getByDisplayValue("initial");
  const storage = browserDocumentStorage();
  const save = storage.save.bind(storage);
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const writes: string[] = [];
  const spy = vi.spyOn(storage, "save").mockImplementation(async (kind, body, expected) => {
    if (kind === "environments") {
      writes.push(body);
      if (writes.length === 1) await gate;
    }
    return save(kind, body, expected);
  });
  try {
    fireEvent.change(input, { target: { value: "a" } });
    await waitFor(() => expect(writes).toHaveLength(1));
    fireEvent.change(input, { target: { value: "ab" } });
    expect((input as HTMLInputElement).value).toBe("ab");
    release();
    await waitFor(() =>
      expect(
        JSON.parse(localStorage.getItem(previewDocumentKey("environments")) ?? "null").environments[0].variables[0]
          .value,
      ).toBe("ab"),
    );
    expect(writes.map((body) => JSON.parse(body).environments[0].variables[0].value)).toEqual(["a", "ab"]);
  } finally {
    release();
    spy.mockRestore();
  }
});

it("imports a preview into the persisted collection and conditionally undoes it", async () => {
  await renderReady();
  fireEvent.click(screen.getByRole("button", { name: "가져오기" }));
  fireEvent.change(await screen.findByLabelText("curl 명령"), {
    target: { value: "curl https://import.example.test/health" },
  });
  fireEvent.click(screen.getByRole("button", { name: "미리 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "선택한 1개 가져오기" }));
  await screen.findByText("1개를 가져왔습니다.");
  expect(JSON.parse(localStorage.getItem(COLLECTION_V2_LS_KEY)!).collections).toHaveLength(2);
  fireEvent.click(screen.getByRole("button", { name: "되돌리기" }));
  await waitFor(() => expect(JSON.parse(localStorage.getItem(COLLECTION_V2_LS_KEY)!).collections).toHaveLength(1));
});

it("exports safe request files and reports retained stale paths", async () => {
  vi.mocked(pickCollectionFolder).mockResolvedValue({ grant_id: "g", name: "Demo" });
  vi.mocked(writeCollectionFolder).mockResolvedValue({ written: 2, stale: ["Old.request.json"] });
  await renderReady();
  fireEvent.click(screen.getByRole("button", { name: "폴더로 내보내기" }));
  await screen.findByText(/요청 1개를 폴더에 저장했습니다/);
  expect(screen.getByText(/Old.request.json/)).toBeTruthy();
  const [grant, files] = vi.mocked(writeCollectionFolder).mock.calls.slice(-1)[0];
  expect(grant).toBe("g");
  expect(files[0].relativePath).toBe("collection.devbox.json");
  expect(JSON.stringify(files)).not.toContain(RAW_SECRET);
});
it("explains that an unrelated export folder cannot be overwritten", async () => {
  vi.mocked(pickCollectionFolder).mockResolvedValue({ grant_id: "g", name: "Other" });
  const error = new Error();
  error.name = "folder_not_collection";
  vi.mocked(writeCollectionFolder).mockRejectedValue(error);
  await renderReady();
  fireEvent.click(screen.getByRole("button", { name: "폴더로 내보내기" }));
  await screen.findByText("다른 파일이 있는 폴더입니다. 빈 폴더나 이전에 내보낸 폴더를 골라 주세요.");
});

it("evaluates and captures manual responses and saves checks with a collection request", async () => {
  vi.mocked(sendRequest).mockResolvedValue({
    status: 200,
    status_text: "OK",
    headers: [],
    duration_ms: 5,
    size_bytes: 15,
    body: '{"token":"captured-value"}',
    is_json: true,
    final_url: "https://x.test",
    redirects: [],
    cookies: [],
    response_id: null,
    raw_headers_available: false,
    headers_truncated: false,
  });
  await renderReady();
  fireEvent.change(screen.getByPlaceholderText("https://api.example.com/users"), {
    target: { value: "https://x.test/login" },
  });
  fireEvent.click(screen.getByRole("button", { name: "검증" }));
  fireEvent.click(await screen.findByRole("button", { name: "검증 추가" }));
  fireEvent.click(screen.getByRole("button", { name: "캡처" }));
  fireEvent.click(await screen.findByRole("button", { name: "캡처 추가" }));
  fireEvent.change(screen.getByLabelText("변수 이름 1"), { target: { value: "token" } });
  fireEvent.change(screen.getByLabelText("캡처 대상 1"), { target: { value: "$.token" } });
  fireEvent.click(screen.getByRole("button", { name: "보내기" }));
  await screen.findByRole("button", { name: "token 보기" });
  expect(screen.queryByText("captured-value")).toBeNull();
  fireEvent.click(screen.getByRole("tab", { name: "검증" }));
  expect(screen.getByText("1개 중 1개 통과")).toBeTruthy();
  await waitFor(() =>
    expect((screen.getByRole("button", { name: "보내기" }) as HTMLButtonElement).disabled).toBe(false),
  );
  fireEvent.change(screen.getByPlaceholderText("저장 이름"), { target: { value: "Login" } });
  fireEvent.click(screen.getByRole("button", { name: "저장" }));
  await waitFor(() =>
    expect(JSON.parse(localStorage.getItem(COLLECTION_V2_LS_KEY)!).collections[0]).toMatchObject({
      name: "Login",
      assertions: [{ source: "status", expected: "200" }],
      captures: [{ variable: "token", target: "$.token" }],
    }),
  );
});

it("manual native sends pass capture definitions and keep tokens sealed until explicit reveal", async () => {
  vi.mocked(sendRequest).mockResolvedValue({
    status: 200,
    status_text: "OK",
    headers: [],
    duration_ms: 5,
    size_bytes: 20,
    body: '{"token":"[REDACTED]"}',
    is_json: true,
    final_url: "https://x.test",
    redirects: [],
    cookies: [],
    response_id: null,
    raw_headers_available: false,
    headers_truncated: false,
    captures: { values: [{ name: "token", value: "native-sealed", reference: "native-ref" }], missing: [], errors: [] },
  });
  vi.mocked(revealCapture).mockResolvedValue("revealed-native-token");
  await renderReady();
  nativeMode.value = true;
  fireEvent.change(screen.getByPlaceholderText("https://api.example.com/users"), {
    target: { value: "https://x.test/login" },
  });
  fireEvent.click(screen.getByRole("button", { name: "캡처" }));
  fireEvent.click(await screen.findByRole("button", { name: "캡처 추가" }));
  fireEvent.change(screen.getByLabelText("변수 이름 1"), { target: { value: "token" } });
  fireEvent.change(screen.getByLabelText("캡처 대상 1"), { target: { value: "$.token" } });
  fireEvent.click(screen.getByRole("button", { name: "보내기" }));
  await screen.findByRole("button", { name: "token 보기" });
  expect(vi.mocked(sendRequest).mock.calls.slice(-1)[0]?.[3]).toEqual([
    expect.objectContaining({ variable: "token", target: "$.token" }),
  ]);
  expect(sealSecret).not.toHaveBeenCalled();
  expect(revealCapture).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "token 보기" }));
  expect(await screen.findByText("revealed-native-token")).toBeTruthy();
  expect(revealCapture).toHaveBeenCalledWith("native-ref");
});

it("opens OAuth login from a native authorization-required response", async () => {
  await renderReady();
  nativeMode.value = true;
  vi.mocked(sendRequest).mockRejectedValue(
    Object.assign(new Error("private-provider-detail"), { name: "oauth2_authorization_required" }),
  );
  vi.mocked(oauth2Status).mockResolvedValue({ state: "missing", expiresAtMs: null, scope: null });
  vi.mocked(authorizeOAuth2).mockResolvedValue({ state: "valid", expiresAtMs: null, scope: "read" });
  fireEvent.change(screen.getByPlaceholderText("https://api.example.com/users"), {
    target: { value: "https://api.test/me" },
  });
  fireEvent.click(screen.getByRole("button", { name: "AUTH" }));
  fireEvent.change(screen.getByLabelText("인증 종류"), { target: { value: "oauth2" } });
  fireEvent.change(await screen.findByLabelText("Authorization URL"), {
    target: { value: "https://auth.test/authorize" },
  });
  fireEvent.change(screen.getByLabelText("Token URL"), { target: { value: "https://auth.test/token" } });
  fireEvent.change(screen.getByLabelText("Client ID"), { target: { value: "client" } });
  fireEvent.click(screen.getByRole("button", { name: "PARAMS" }));
  fireEvent.click(screen.getByRole("button", { name: "보내기" }));
  await screen.findByText("로그인이 필요합니다.");
  expect(screen.queryByText("private-provider-detail")).toBeNull();
  const authorizationAlert = screen
    .getAllByRole("alert")
    .find((alert) => alert.contains(screen.getByText("로그인이 필요합니다.")));
  if (!authorizationAlert) throw new Error("Authorization-required alert was not rendered");
  fireEvent.click(within(authorizationAlert).getByRole("button", { name: "로그인" }));
  await waitFor(() => expect(authorizeOAuth2).toHaveBeenCalledTimes(1));
  expect(await screen.findByText("유효 · 만료 시각 없음")).toBeTruthy();
});

it("TLS verification warning remains above the editor after changing tabs", async () => {
  await renderReady();
  fireEvent.click(screen.getByRole("button", { name: "TLS" }));
  fireEvent.click(await screen.findByLabelText("인증서 검증"));
  fireEvent.click(screen.getByRole("button", { name: "PARAMS" }));
  expect(screen.getByText("인증서 검증 꺼짐")).toBeTruthy();
});

it("opens generated code while keeping the existing one-time curl copy", async () => {
  await renderReady();
  fireEvent.change(screen.getByPlaceholderText("https://api.example.com/users"), {
    target: { value: "https://example.test/" },
  });
  fireEvent.click(screen.getByRole("button", { name: "코드" }));
  expect(await screen.findByRole("tab", { name: "JavaScript fetch" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "cURL" }));
  expect(screen.getByRole("button", { name: "원문 1회 복사" })).toBeTruthy();
});
