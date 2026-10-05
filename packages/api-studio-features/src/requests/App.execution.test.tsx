import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import App from "./App";
import { seedPreviewDocument, previewDocumentKey } from "../storage/testDocuments";
import { sendRequest, sanitizePersistedJson } from "./api";
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

afterEach(cleanup);
beforeEach(() => {
  localStorage.clear();
  vi.mocked(sendRequest).mockReset();
  vi.mocked(sanitizePersistedJson).mockImplementation(async (value) => value);
});
const response = (body: string) => ({
  status: 200,
  status_text: "OK",
  headers: [],
  duration_ms: 1,
  size_bytes: body.length,
  body,
  is_json: false,
  final_url: "https://example.test",
  redirects: [],
  cookies: [],
  response_id: null,
  raw_headers_available: false,
  headers_truncated: false,
});
it("keeps the previous response identified after cancellation and ignores late results", async () => {
  let finish!: (value: ReturnType<typeof response>) => void;
  vi.mocked(sendRequest)
    .mockResolvedValueOnce(response("A unique response"))
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
  render(<App />);
  const send = await screen.findByRole("button", { name: "보내기" });
  fireEvent.change(screen.getByRole("textbox", { name: "요청 URL" }), { target: { value: "https://example.test" } });
  fireEvent.click(send);
  await screen.findByText("A unique response");
  await waitFor(() =>
    expect((screen.getByRole("button", { name: "보내기" }) as HTMLButtonElement).disabled).toBe(false),
  );
  fireEvent.click(screen.getByRole("button", { name: "보내기" }));
  await screen.findByText(/이전 요청의 응답입니다/);
  fireEvent.click(screen.getByRole("button", { name: "취소" }));
  await screen.findByText(/서버 작업의 취소 여부/);
  finish(response("B unique response"));
  await waitFor(() => expect(screen.queryByText("B unique response")).toBeNull());
  expect(sendRequest).toHaveBeenCalledTimes(2);
});

it("shows a failed environment save in Protocols while keeping writes locked", async () => {
  seedPreviewDocument(
    "environments",
    JSON.stringify({
      version: 1,
      environments: [{ id: "e", name: "dev", variables: [{ key: "baseUrl", value: "original", secret: false }] }],
    }),
  );
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "dev" }));
  const variable = await screen.findByRole("textbox", { name: "환경 변수 baseUrl 값" });
  await waitFor(() => expect((variable as HTMLInputElement).disabled).toBe(false));
  fireEvent.click(screen.getByRole("button", { name: "Protocol Lab" }));
  const original = Storage.prototype.setItem;
  const write = vi.spyOn(Storage.prototype, "setItem").mockImplementation(function (this: Storage, key, value) {
    if (key === previewDocumentKey("environments")) throw new Error("synthetic write failure");
    return original.call(this, key, value);
  });
  fireEvent.change(variable, { target: { value: "unsaved draft" } });
  await screen.findByText(/환경 편집 내용을 저장하지 못했습니다/);
  expect((variable as HTMLInputElement).disabled).toBe(true);
  expect((variable as HTMLInputElement).value).toBe("unsaved draft");
  expect(JSON.parse(localStorage.getItem(previewDocumentKey("environments"))!).environments[0].variables[0].value).toBe(
    "original",
  );
  write.mockRestore();
});

it("names active body and authentication controls for real keyboard input", async () => {
  render(<App />);
  fireEvent.change(await screen.findByRole("textbox", { name: "요청 URL" }), {
    target: { value: "https://example.test" },
  });
  await waitFor(() =>
    expect((screen.getByRole("button", { name: "보내기" }) as HTMLButtonElement).disabled).toBe(false),
  );
  fireEvent.click(screen.getByRole("button", { name: "BODY" }));
  fireEvent.change(screen.getByRole("combobox", { name: "요청 본문 형식" }), { target: { value: "raw" } });
  expect(screen.getByRole("textbox", { name: "요청 본문" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "AUTH" }));
  fireEvent.change(screen.getByRole("combobox", { name: "인증 종류" }), { target: { value: "basic" } });
  expect(screen.getByRole("textbox", { name: "사용자 이름" })).toBeTruthy();
  expect(screen.getByLabelText("비밀번호")).toBeTruthy();
  fireEvent.change(screen.getByRole("combobox", { name: "인증 종류" }), { target: { value: "bearer" } });
  expect(screen.getByRole("textbox", { name: "토큰" })).toBeTruthy();
});

it("cancels a pending assertion worker without publishing its late result", async () => {
  let worker!: { terminate: ReturnType<typeof vi.fn>; onmessage?: (event: { data: unknown }) => void };
  vi.stubGlobal(
    "Worker",
    class {
      terminate = vi.fn();
      postMessage = vi.fn();
      constructor() {
        worker = this;
      }
    },
  );
  try {
    vi.mocked(sendRequest).mockResolvedValue(response("a".repeat(32) + "!"));
    render(<App />);
    fireEvent.change(await screen.findByRole("textbox", { name: "요청 URL" }), {
      target: { value: "https://example.test" },
    });
    fireEvent.click(screen.getByRole("button", { name: "검증" }));
    fireEvent.click(await screen.findByRole("button", { name: "검증 추가" }));
    fireEvent.change(screen.getByLabelText("출처 1"), { target: { value: "body" } });
    fireEvent.change(screen.getByLabelText("연산 1"), { target: { value: "matches" } });
    fireEvent.change(screen.getByLabelText("기대값 1"), { target: { value: "^(a+)+$" } });
    fireEvent.click(screen.getByRole("button", { name: "보내기" }));
    await waitFor(() => expect(worker).toBeDefined());
    fireEvent.click(screen.getByRole("button", { name: "취소" }));
    await waitFor(() => expect(worker.terminate).toHaveBeenCalledOnce());
    worker.onmessage?.({ data: { matched: true } });
    await screen.findByText(/서버 작업의 취소 여부/);
    expect(screen.queryByText("1개 중 1개 통과")).toBeNull();
    expect((screen.getByRole("button", { name: "보내기" }) as HTMLButtonElement).disabled).toBe(false);
  } finally {
    vi.unstubAllGlobals();
  }
});
