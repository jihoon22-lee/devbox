import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  clearHistory,
  copyHistoryHeaders,
  copyMaskedHistory,
  copyRawHistory,
  deleteHistory,
  deleteRule,
  listHistory,
  listRules,
  serverStatus,
  setRule,
  startServer,
  stopServer,
  type RequestRecord,
  type ResponseRule,
} from "./api";

vi.mock("./api", () => ({
  clearHistory: vi.fn(),
  copyHistoryHeaders: vi.fn(),
  copyMaskedHistory: vi.fn(),
  copyRawHistory: vi.fn(),
  deleteHistory: vi.fn(),
  deleteRule: vi.fn(),
  listHistory: vi.fn(),
  listRules: vi.fn(),
  serverStatus: vi.fn(),
  setRule: vi.fn(),
  startServer: vi.fn(),
  stopServer: vi.fn(),
}));

const initialHistory: RequestRecord[] = [
  {
    id: 1,
    method: "POST",
    url: "/hook",
    headers: [["Authorization", "•••••"], ["Content-Type", "application/json"]],
    body: '{"event":"push"}',
    receivedAtMs: 1_700_000_000_000,
  },
  {
    id: 2,
    method: "GET",
    url: "/health",
    headers: [],
    body: "",
    receivedAtMs: 1_700_000_001_000,
  },
];

const initialRule: ResponseRule = {
  id: "rule-1",
  method: "GET",
  path: "/health",
  status: 204,
  headers: [],
  body: "",
  delayMs: 25,
};

const clearHistoryMock = vi.mocked(clearHistory);
const copyHistoryHeadersMock = vi.mocked(copyHistoryHeaders);
const copyMaskedHistoryMock = vi.mocked(copyMaskedHistory);
const copyRawHistoryMock = vi.mocked(copyRawHistory);
const deleteHistoryMock = vi.mocked(deleteHistory);
const deleteRuleMock = vi.mocked(deleteRule);
const listHistoryMock = vi.mocked(listHistory);
const listRulesMock = vi.mocked(listRules);
const serverStatusMock = vi.mocked(serverStatus);
const setRuleMock = vi.mocked(setRule);
const startServerMock = vi.mocked(startServer);
const stopServerMock = vi.mocked(stopServer);
const confirmMock = vi.fn<(message?: string) => boolean>();
const writeTextMock = vi.fn<(value: string) => Promise<void>>();
let history: RequestRecord[];
let rules: ResponseRule[];

beforeEach(() => {
  history = initialHistory.map((request) => ({
    ...request,
    headers: request.headers.map((header) => [...header] as [string, string]),
  }));
  rules = [{ ...initialRule }];
  serverStatusMock.mockReset().mockResolvedValue({ running: false, address: null });
  listHistoryMock.mockReset().mockImplementation(async () => history.map((request) => ({ ...request })));
  listRulesMock.mockReset().mockImplementation(async () => rules.map((rule) => ({ ...rule })));
  clearHistoryMock.mockReset().mockImplementation(async () => { history = []; });
  deleteHistoryMock.mockReset().mockImplementation(async (id) => {
    history = history.filter((request) => request.id !== id);
  });
  copyMaskedHistoryMock.mockReset().mockImplementation(async (id) => `masked:${id}`);
  copyRawHistoryMock.mockReset().mockImplementation(async (id) => `raw-secret:${id}`);
  copyHistoryHeadersMock.mockReset().mockImplementation(async (id) => `masked-headers:${id}`);
  setRuleMock.mockReset().mockImplementation(async (rule) => {
    const id = rule.id || `rule-${rules.length + 1}`;
    const stored = { ...rule, id };
    const index = rules.findIndex((candidate) => candidate.id === id);
    if (index >= 0) rules[index] = stored;
    else rules.push(stored);
    return id;
  });
  deleteRuleMock.mockReset().mockImplementation(async (id) => {
    rules = rules.filter((rule) => rule.id !== id);
  });
  startServerMock.mockReset().mockResolvedValue({ running: true, address: "127.0.0.1:9000" });
  stopServerMock.mockReset().mockResolvedValue({ running: false, address: null });
  confirmMock.mockReset().mockReturnValue(false);
  writeTextMock.mockReset().mockResolvedValue(undefined);
  Object.defineProperty(window, "confirm", { configurable: true, value: confirmMock });
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: writeTextMock },
  });
});

afterEach(() => cleanup());

describe("Webhook Lab history and rule context menus", () => {
  it("마스킹된 history와 rule을 렌더링한다", async () => {
    render(<App />);

    await screen.findByText("History (2)");
    expect(screen.getByText("/hook")).toBeTruthy();
    expect(screen.getByText("민감 헤더 마스킹됨")).toBeTruthy();
    expect(screen.getByText(/GET \/health → 204/)).toBeTruthy();
  });

  it("우클릭한 요청을 먼저 선택하고 정확한 메뉴와 후속 기능 경계를 표시한다", async () => {
    render(<App />);
    const target = await screen.findByLabelText("GET /health 요청") as HTMLDivElement;

    fireEvent.contextMenu(target, { clientX: 20, clientY: 20 });

    expect(target.getAttribute("aria-current")).toBe("true");
    for (const label of ["마스킹 복사", "원본 복사", "헤더 복사", "API Playground로 변환", "삭제"]) {
      expect(screen.getByRole("menuitem", { name: label })).toBeTruthy();
    }
    expect(screen.getByRole("menuitem", { name: "API Playground로 변환" }).getAttribute("aria-disabled"))
      .toBe("true");
    expect(screen.getByRole("menuitem", { name: "삭제" }).className).toContain("danger");
  });

  it("마스킹 복사와 헤더 복사는 정확한 요청 ID의 안전한 backend 결과만 쓴다", async () => {
    render(<App />);
    const target = await screen.findByLabelText("GET /health 요청") as HTMLDivElement;

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "마스킹 복사" }));
    await waitFor(() => expect(copyMaskedHistoryMock).toHaveBeenCalledWith(2));
    expect(copyRawHistoryMock).not.toHaveBeenCalled();
    expect(writeTextMock).toHaveBeenCalledWith("masked:2");

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "헤더 복사" }));
    await waitFor(() => expect(copyHistoryHeadersMock).toHaveBeenCalledWith(2));
    expect(writeTextMock).toHaveBeenCalledWith("masked-headers:2");
  });

  it("원본 복사는 확인 전 backend를 호출하지 않고 키보드 메뉴 종료 후 포커스를 복원한다", async () => {
    render(<App />);
    const target = await screen.findByLabelText("POST /hook 요청") as HTMLDivElement;
    target.focus();

    fireEvent.keyDown(target, { key: "F10", code: "F10", shiftKey: true });
    fireEvent.click(screen.getByRole("menuitem", { name: "원본 복사" }));

    expect(confirmMock).toHaveBeenCalledTimes(1);
    expect(copyRawHistoryMock).not.toHaveBeenCalled();
    await waitFor(() => expect(document.activeElement).toBe(target));

    confirmMock.mockReturnValueOnce(true);
    fireEvent.keyDown(target, { key: "ContextMenu", code: "ContextMenu" });
    fireEvent.click(screen.getByRole("menuitem", { name: "원본 복사" }));

    await waitFor(() => expect(copyRawHistoryMock).toHaveBeenCalledWith(1));
    expect(writeTextMock).toHaveBeenCalledWith("raw-secret:1");
    await waitFor(() => expect(document.activeElement).toBe(target));
  });

  it("원본 복사 실패는 backend 원문을 화면 오류에 반향하지 않는다", async () => {
    confirmMock.mockReturnValue(true);
    copyRawHistoryMock.mockRejectedValueOnce(new Error("Bearer backend-raw-secret"));
    render(<App />);
    const target = await screen.findByLabelText("POST /hook 요청") as HTMLDivElement;

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "원본 복사" }));

    expect(await screen.findByText("원본 요청을 안전하게 만들거나 복사하지 못했습니다.")).toBeTruthy();
    expect(document.body.textContent?.includes("backend-raw-secret")).toBe(false);
  });

  it("history 삭제와 전체 비우기는 확인된 경우에만 실행한다", async () => {
    render(<App />);
    const target = await screen.findByLabelText("GET /health 요청") as HTMLDivElement;

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "삭제" }));
    expect(deleteHistoryMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "삭제" }));
    await waitFor(() => expect(deleteHistoryMock).toHaveBeenCalledWith(2));
    await screen.findByText("History (1)");

    fireEvent.click(screen.getByRole("button", { name: "비우기" }));
    expect(clearHistoryMock).not.toHaveBeenCalled();
    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(screen.getByRole("button", { name: "비우기" }));
    await waitFor(() => expect(clearHistoryMock).toHaveBeenCalledTimes(1));
    await screen.findByText("History (0)");
  });

  it("rule 메뉴에서 정확한 규칙을 편집하고 새 ID로 복제한다", async () => {
    render(<App />);
    const target = await screen.findByLabelText("GET /health 규칙") as HTMLDivElement;

    fireEvent.contextMenu(target);
    expect(target.getAttribute("aria-current")).toBe("true");
    expect(screen.getByRole("menuitem", { name: "예시 curl 복사" }).getAttribute("aria-disabled"))
      .toBe("true");
    fireEvent.click(screen.getByRole("menuitem", { name: "편집" }));
    expect((screen.getByPlaceholderText("method (없으면 전체)") as HTMLInputElement).value).toBe("GET");
    expect(screen.getByRole("button", { name: "규칙 저장" })).toBeTruthy();

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "복제" }));
    await waitFor(() => expect(setRuleMock).toHaveBeenCalledWith(expect.objectContaining({ id: "", path: "/health" })));
  });

  it("rule 삭제는 danger 확인을 거치며 취소하면 상태를 유지한다", async () => {
    render(<App />);
    const target = await screen.findByLabelText("GET /health 규칙") as HTMLDivElement;

    fireEvent.contextMenu(target);
    const deleteItem = screen.getByRole("menuitem", { name: "삭제" });
    expect(deleteItem.className).toContain("danger");
    fireEvent.click(deleteItem);
    expect(deleteRuleMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "삭제" }));
    await waitFor(() => expect(deleteRuleMock).toHaveBeenCalledWith("rule-1"));
    await screen.findByText(/규칙 없음/);
  });

  it("LAN 공개를 켜면 경고를 표시한다", async () => {
    render(<App />);
    await screen.findByText(/중지/, { selector: ".status" });

    fireEvent.click(screen.getByRole("checkbox"));

    expect(await screen.findByText(/LAN 공개는 명시적 설정입니다/)).toBeTruthy();
  });
});
