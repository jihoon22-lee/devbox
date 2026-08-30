import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { OPENAPI_DOCUMENT_LIMITS } from "@devbox/openapi";
import App from "./App";
import {
  clearFixtures,
  clearHistory,
  copyHistoryHeaders,
  copyMaskedHistory,
  copyRawHistory,
  deleteFixture,
  deleteHistory,
  deleteRule,
  exportRunServiceDefinition,
  fixtureToRule,
  listFixtures,
  listHistory,
  listRules,
  previewRuleConflicts,
  replayFixture,
  replayHistory,
  resetRuleSequence,
  saveFixture,
  sendFixtureToApi,
  sendFixtureToLogLens,
  sendHistoryToApi,
  sendHistoryToLogLens,
  serverStatus,
  setRule,
  startServer,
  stopServer,
  type RequestRecord,
  type RuleConflictPreview,
  type ResponseRule,
  type CapturedFixture,
  type RunDefinitionExport,
} from "./api";

vi.mock("./api", () => ({
  clearFixtures: vi.fn(),
  clearHistory: vi.fn(),
  copyHistoryHeaders: vi.fn(),
  copyMaskedHistory: vi.fn(),
  copyRawHistory: vi.fn(),
  deleteFixture: vi.fn(),
  deleteHistory: vi.fn(),
  deleteRule: vi.fn(),
  exportRunServiceDefinition: vi.fn(),
  fixtureToRule: vi.fn(),
  listFixtures: vi.fn(),
  listHistory: vi.fn(),
  listRules: vi.fn(),
  previewRuleConflicts: vi.fn(),
  replayFixture: vi.fn(),
  replayHistory: vi.fn(),
  resetRuleSequence: vi.fn(),
  saveFixture: vi.fn(),
  sendFixtureToApi: vi.fn(),
  sendFixtureToLogLens: vi.fn(),
  sendHistoryToApi: vi.fn(),
  sendHistoryToLogLens: vi.fn(),
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
  priority: 0,
  method: "GET",
  path: "/health",
  status: 204,
  headers: [],
  body: "",
  delayMs: 25,
};

const safeRunDefinition: RunDefinitionExport = {
  schemaVersion: 1,
  exportedAt: "1700000000000",
  jobs: [],
  services: [{
    id: "00000000-0000-4000-8000-000000000001",
    kind: "service",
    name: "Webhook Lab · 127.0.0.1:9000",
    command: "exit /b 1",
    cwd: null,
    targetKind: "windows",
    targetDistro: null,
    envConfigured: false,
    cronExpr: null,
    enabled: false,
    overlapPolicy: "skip",
    catchUp: false,
    lastEvaluatedAt: null,
    nextQueueSequence: 0,
    restartPolicy: "never",
    autoStart: false,
    healthTcpAddress: "127.0.0.1",
    healthTcpPort: 9000,
    healthStartGraceMs: 10000,
    createdAt: 1700000000000,
    updatedAt: 1700000000000,
  }],
};

const clearFixturesMock = vi.mocked(clearFixtures);
const clearHistoryMock = vi.mocked(clearHistory);
const copyHistoryHeadersMock = vi.mocked(copyHistoryHeaders);
const copyMaskedHistoryMock = vi.mocked(copyMaskedHistory);
const copyRawHistoryMock = vi.mocked(copyRawHistory);
const deleteFixtureMock = vi.mocked(deleteFixture);
const deleteHistoryMock = vi.mocked(deleteHistory);
const deleteRuleMock = vi.mocked(deleteRule);
const exportRunServiceDefinitionMock = vi.mocked(exportRunServiceDefinition);
const fixtureToRuleMock = vi.mocked(fixtureToRule);
const listFixturesMock = vi.mocked(listFixtures);
const listHistoryMock = vi.mocked(listHistory);
const listRulesMock = vi.mocked(listRules);
const previewRuleConflictsMock = vi.mocked(previewRuleConflicts);
const replayFixtureMock = vi.mocked(replayFixture);
const replayHistoryMock = vi.mocked(replayHistory);
const resetRuleSequenceMock = vi.mocked(resetRuleSequence);
const saveFixtureMock = vi.mocked(saveFixture);
const sendFixtureToApiMock = vi.mocked(sendFixtureToApi);
const sendFixtureToLogLensMock = vi.mocked(sendFixtureToLogLens);
const sendHistoryToApiMock = vi.mocked(sendHistoryToApi);
const sendHistoryToLogLensMock = vi.mocked(sendHistoryToLogLens);
const serverStatusMock = vi.mocked(serverStatus);
const setRuleMock = vi.mocked(setRule);
const startServerMock = vi.mocked(startServer);
const stopServerMock = vi.mocked(stopServer);
const confirmMock = vi.fn<(message?: string) => boolean>();
const writeTextMock = vi.fn<(value: string) => Promise<void>>();
let history: RequestRecord[];
let rules: ResponseRule[];
let fixtures: CapturedFixture[];

beforeEach(() => {
  history = initialHistory.map((request) => ({
    ...request,
    headers: request.headers.map((header) => [...header] as [string, string]),
  }));
  rules = [{ ...initialRule }];
  fixtures = [];
  serverStatusMock.mockReset().mockResolvedValue({ running: false, address: null });
  listHistoryMock.mockReset().mockImplementation(async () => history.map((request) => ({ ...request })));
  listRulesMock.mockReset().mockImplementation(async () => rules.map((rule) => ({ ...rule })));
  listFixturesMock.mockReset().mockImplementation(async () => fixtures.map((fixture) => ({
    ...fixture,
    headers: fixture.headers.map((header) => [...header] as [string, string]),
  })));
  exportRunServiceDefinitionMock.mockReset().mockResolvedValue({
    ...safeRunDefinition,
    jobs: [],
    services: safeRunDefinition.services.map((service) => ({ ...service })),
  });
  previewRuleConflictsMock.mockReset().mockImplementation(async (candidate): Promise<RuleConflictPreview> => ({
    candidateId: candidate.id || `rule-${rules.length + 1}`,
    conflicts: [],
    requiresConfirmation: false,
  }));
  replayHistoryMock.mockReset().mockResolvedValue({
    sourceId: "history-1",
    status: 200,
  });
  replayFixtureMock.mockReset().mockResolvedValue({
    sourceId: "fixture-fixture-1",
    status: 200,
  });
  resetRuleSequenceMock.mockReset().mockResolvedValue();
  saveFixtureMock.mockReset().mockImplementation(async (historyId) => {
    const request = history.find((candidate) => candidate.id === historyId);
    if (!request) throw new Error("fixture를 찾을 수 없습니다");
    const fixture: CapturedFixture = {
      id: `fixture-${historyId}`,
      method: request.method,
      url: request.url,
      headers: request.headers.map((header) => [...header] as [string, string]),
      body: request.body,
      receivedAtMs: request.receivedAtMs,
    };
    fixtures.push(fixture);
    return fixture;
  });
  sendHistoryToApiMock.mockReset().mockResolvedValue({
    handoffId: "0123456789abcdef0123456789abcdef",
    producerId: "webhook-lab",
    consumerId: "api-playground",
    createdAtMs: 1_700_000_000_000,
    expiresAtMs: 1_700_000_600_000,
  });
  sendFixtureToApiMock.mockReset().mockResolvedValue({
    handoffId: "fedcba9876543210fedcba9876543210",
    producerId: "webhook-lab",
    consumerId: "api-playground",
    createdAtMs: 1_700_000_000_000,
    expiresAtMs: 1_700_000_600_000,
  });
  sendHistoryToLogLensMock.mockReset().mockResolvedValue({
    handoffId: "abcdef0123456789abcdef0123456789",
    producerId: "webhook-lab",
    consumerId: "log-lens",
    createdAtMs: 1_700_000_000_000,
    expiresAtMs: 1_700_000_600_000,
  });
  sendFixtureToLogLensMock.mockReset().mockResolvedValue({
    handoffId: "9876543210fedcba9876543210fedcba",
    producerId: "webhook-lab",
    consumerId: "log-lens",
    createdAtMs: 1_700_000_000_000,
    expiresAtMs: 1_700_000_600_000,
  });
  fixtureToRuleMock.mockImplementation(async (id) => {
    const fixture = fixtures.find((candidate) => candidate.id === id);
    if (!fixture) throw new Error("fixture를 찾을 수 없습니다");
    return {
      id: "",
      priority: 0,
      method: fixture.method,
      path: fixture.url,
      status: 200,
      headers: [],
      body: "",
      delayMs: 0,
    };
  });
  clearHistoryMock.mockReset().mockImplementation(async () => { history = []; });
  clearFixturesMock.mockReset().mockImplementation(async () => { fixtures = []; });
  deleteHistoryMock.mockReset().mockImplementation(async (id) => {
    history = history.filter((request) => request.id !== id);
  });
  deleteFixtureMock.mockReset().mockImplementation(async (id) => {
    fixtures = fixtures.filter((fixture) => fixture.id !== id);
  });
  copyMaskedHistoryMock.mockReset().mockImplementation(async (id) => `masked:${id}`);
  copyRawHistoryMock.mockReset().mockImplementation(async (id) => `raw-secret:${id}`);
  copyHistoryHeadersMock.mockReset().mockImplementation(async (id) => `masked-headers:${id}`);
  setRuleMock.mockReset().mockImplementation(async (rule) => {
    const id = rule.id || `rule-${rules.length + 1}`;
    const stored: ResponseRule = { ...rule, id, priority: rule.priority ?? 0 };
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

function makeOpenApiFile(name: string, document: unknown): File {
  const source = typeof document === "string" ? document : JSON.stringify(document);
  return new File([source], name, { type: name.endsWith(".json") ? "application/json" : "text/yaml" });
}

describe("Webhook Lab history and rule context menus", () => {
  it("마스킹된 history와 rule을 렌더링한다", async () => {
    render(<App />);

    await screen.findByText("History (2)");
    expect(screen.getByText("/hook")).toBeTruthy();
    expect(screen.getByText("민감 헤더 마스킹됨")).toBeTruthy();
    expect(screen.getByText(/GET \/health → 204/)).toBeTruthy();
    expect(screen.getByText("priority 0")).toBeTruthy();
  });

  it("rule 매칭 필드의 의미를 값이 채워져도 항상 표시한다", async () => {
    render(<App />);

    await screen.findByText("History (2)");

    expect(screen.getByLabelText("method").getAttribute("aria-describedby")).toBe("rule-method-help");
    expect(screen.getByLabelText("path").getAttribute("aria-describedby")).toBe("rule-path-help");
    expect(screen.getByLabelText("priority").getAttribute("aria-describedby")).toBe("rule-priority-help");
    expect(screen.getByLabelText("status").getAttribute("aria-describedby")).toBe("rule-status-help");
    expect(screen.getByLabelText("delay (ms)").getAttribute("aria-describedby")).toBe("rule-delay-help");
    expect(screen.getByLabelText("응답 body").getAttribute("aria-describedby")).toBe("rule-body-help rule-headers-help");
    expect(screen.getByText("대소문자를 구분하지 않고 요청 method와 일치합니다. 비워두면 모든 method(*)에 적용됩니다. ASCII HTTP token, 최대 16자/16바이트입니다.")).toBeTruthy();
    expect(screen.getByText("경로 전체가 정확히 일치합니다. 마지막 문자가 *일 때만 그 앞부분으로 시작하는 경로와 일치합니다 (예: /events/* → /events/123). /로 시작하고 최대 4,096자/16,384바이트입니다.")).toBeTruthy();
    expect(screen.getByText("priority가 높을수록 먼저 적용됩니다. 같으면 정확한 path, method 지정, 긴 wildcard 순서이며 마지막에는 rule ID로 결정합니다.")).toBeTruthy();
    expect(screen.getByText("매칭된 요청에 돌려줄 HTTP 응답 status 코드입니다 (허용 범위: 100~599, 예: 200, 404, 500).")).toBeTruthy();
    expect(screen.getByText("응답 전에 기다릴 시간(밀리초)입니다. 0이면 지연 없이 바로 응답합니다 (허용 범위: 0~60000ms).")).toBeTruthy();
    expect(screen.getByText("매칭된 요청에 돌려줄 response body입니다. 저장된 headers와 함께 응답 규칙의 출력으로 사용됩니다. body는 최대 256,000자/1,024,000바이트입니다.")).toBeTruthy();
    expect(screen.getByText("response headers는 최대 100개이며 이름 256자/256바이트, 값 16,384자/65,536바이트, 전체 64,000자/256,000바이트입니다.")).toBeTruthy();
  });

  it("응답 status 범위를 벗어난 rule은 저장하지 않고 입력 오류를 연결한다", async () => {
    render(<App />);
    await screen.findByText("History (2)");

    const status = screen.getByLabelText("status");
    fireEvent.change(status, { target: { value: "99" } });

    expect(status.getAttribute("aria-invalid")).toBe("true");
    expect(status.getAttribute("min")).toBe("100");
    expect(status.getAttribute("max")).toBe("599");
    fireEvent.click(screen.getByRole("button", { name: "규칙 추가" }));

    expect(setRuleMock).not.toHaveBeenCalled();
    expect((await screen.findByRole("alert")).textContent).toContain("status는 100~599 범위의 정수여야 합니다.");
  });

  it("priority 범위를 벗어난 rule은 저장하지 않고 입력 오류를 연결한다", async () => {
    render(<App />);
    await screen.findByText("History (2)");

    const priority = screen.getByLabelText("priority");
    fireEvent.change(priority, { target: { value: "1001" } });

    expect(priority.getAttribute("aria-invalid")).toBe("true");
    expect(priority.getAttribute("min")).toBe("-1000");
    expect(priority.getAttribute("max")).toBe("1000");
    fireEvent.click(screen.getByRole("button", { name: "규칙 추가" }));

    expect(previewRuleConflictsMock).not.toHaveBeenCalled();
    expect(setRuleMock).not.toHaveBeenCalled();
    expect((await screen.findByRole("alert")).textContent).toContain("priority는 -1000~1000 범위의 정수여야 합니다.");
  });

  it("method/path/body의 잘못된 raw draft를 보존하고 저장을 차단한다", async () => {
    render(<App />);
    await screen.findByText("History (2)");

    const method = screen.getByLabelText("method") as HTMLInputElement;
    fireEvent.change(method, { target: { value: "POST JSON" } });
    expect(method.value).toBe("POST JSON");
    expect(method.getAttribute("aria-invalid")).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: "규칙 추가" }));
    expect(setRuleMock).not.toHaveBeenCalled();
    expect((await screen.findByRole("alert")).textContent).toContain("method는 ASCII HTTP token이어야 합니다");

    const path = screen.getByLabelText("path") as HTMLInputElement;
    fireEvent.change(path, { target: { value: "/bad\u0001path" } });
    expect(path.value).toBe("/bad\u0001path");
    expect(path.getAttribute("aria-invalid")).toBe("true");

    const body = screen.getByLabelText("응답 body") as HTMLTextAreaElement;
    fireEvent.change(body, { target: { value: "draft body" } });
    expect(body.value).toBe("draft body");
    expect(body.getAttribute("aria-describedby")).toContain("rule-body-help");
    expect(body.getAttribute("aria-describedby")).toContain("rule-headers-help");
  });

  it("편집 중인 rule이 refresh에서 사라지면 stale 저장을 차단하고 draft를 유지한다", async () => {
    render(<App />);
    const target = await screen.findByLabelText("GET /health 규칙") as HTMLDivElement;
    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "편집" }));

    const path = screen.getByLabelText("path") as HTMLInputElement;
    fireEvent.change(path, { target: { value: "/stale-draft" } });
    listRulesMock.mockResolvedValueOnce([]);
    fireEvent.click(screen.getByRole("button", { name: "시작" }));
    await waitFor(() => expect(screen.getByText("규칙 없음 — 매치 없으면 404.")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "규칙 저장" }));
    expect(setRuleMock).not.toHaveBeenCalled();
    expect((screen.getByLabelText("path") as HTMLInputElement).value).toBe("/stale-draft");
    expect((await screen.findByRole("alert")).textContent).toContain("선택한 규칙이 더 이상 존재하지 않습니다");
  });

  it("동일한 busy 작업의 double action을 한 번만 실행하고 aria-busy를 표시한다", async () => {
    let release!: (id: string) => void;
    const pending = new Promise<string>((resolve) => { release = resolve; });
    setRuleMock.mockReturnValueOnce(pending);
    render(<App />);
    await screen.findByText("History (2)");

    const save = screen.getByRole("button", { name: "규칙 추가" });
    fireEvent.click(save);
    fireEvent.click(save);
    await waitFor(() => expect(setRuleMock).toHaveBeenCalledTimes(1));
    expect(save.hasAttribute("disabled")).toBe(true);
    for (const label of ["method", "path", "priority", "status", "delay (ms)", "응답 body"]) {
      expect(screen.getByLabelText(label).hasAttribute("disabled")).toBe(true);
    }
    expect(screen.getByRole("button", { name: "규칙 추가" }).closest(".app")?.getAttribute("aria-busy")).toBe("true");

    release("rule-2");
    await waitFor(() => expect(save.hasAttribute("disabled")).toBe(false));
    expect(screen.getByRole("button", { name: "규칙 추가" }).closest(".app")?.getAttribute("aria-busy")).toBe("false");
  });

  it("늦게 끝난 mount refresh가 더 최신 action refresh를 덮어쓰지 않는다", async () => {
    let releaseStatus!: (value: { running: boolean; address: string | null }) => void;
    let releaseHistory!: (value: RequestRecord[]) => void;
    let releaseRules!: (value: ResponseRule[]) => void;
    serverStatusMock
      .mockReset()
      .mockReturnValueOnce(new Promise((resolve) => { releaseStatus = resolve; }))
      .mockResolvedValue({ running: true, address: "127.0.0.1:9000" });
    listHistoryMock
      .mockReset()
      .mockReturnValueOnce(new Promise((resolve) => { releaseHistory = resolve; }))
      .mockImplementation(async () => history.map((request) => ({ ...request })));
    listRulesMock
      .mockReset()
      .mockReturnValueOnce(new Promise((resolve) => { releaseRules = resolve; }))
      .mockImplementation(async () => rules.map((candidate) => ({ ...candidate })));

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "시작" }));
    await screen.findByText("History (2)");
    expect(screen.getByText(/듣는 중 127\.0\.0\.1:9000/u)).toBeTruthy();

    releaseStatus({ running: false, address: null });
    releaseHistory([]);
    releaseRules([]);
    await waitFor(() => {
      expect(screen.getByText("History (2)")).toBeTruthy();
      expect(screen.getByText(/GET \/health → 204/u)).toBeTruthy();
      expect(screen.getByText(/듣는 중 127\.0\.0\.1:9000/u)).toBeTruthy();
    });
  });

  it("빈 method를 모든 method를 뜻하는 null로 저장한다", async () => {
    render(<App />);
    await screen.findByText("History (2)");

    fireEvent.change(screen.getByLabelText("method"), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "규칙 추가" }));

    await waitFor(() => expect(setRuleMock).toHaveBeenCalledWith(expect.objectContaining({ method: null }), false));
    expect(previewRuleConflictsMock).toHaveBeenCalledWith(expect.objectContaining({ method: null }));
    expect(previewRuleConflictsMock.mock.invocationCallOrder[0]).toBeLessThan(setRuleMock.mock.invocationCallOrder[0] ?? Infinity);
  });

  it("충돌 preview를 먼저 확인하고 취소하면 저장하지 않는다", async () => {
    previewRuleConflictsMock.mockResolvedValueOnce({
      candidateId: "candidate-1",
      conflicts: [{
        existingRuleId: "rule-1",
        winnerRuleId: "candidate-1",
        loserRuleId: "rule-1",
        kind: "candidateShadowsExisting",
        reason: "priority",
      }],
      requiresConfirmation: true,
    });
    confirmMock.mockReturnValueOnce(false);
    render(<App />);
    await screen.findByText("History (2)");

    fireEvent.change(screen.getByLabelText("method"), { target: { value: "GET" } });
    fireEvent.change(screen.getByLabelText("path"), { target: { value: "/health" } });
    fireEvent.change(screen.getByLabelText("priority"), { target: { value: "10" } });
    fireEvent.click(screen.getByRole("button", { name: "규칙 추가" }));

    await waitFor(() => expect(previewRuleConflictsMock).toHaveBeenCalledTimes(1));
    expect(confirmMock).toHaveBeenCalledTimes(1);
    const summary = confirmMock.mock.calls[0]?.[0] ?? "";
    expect(summary).toContain("GET /health [candidate-1]");
    expect(summary).toContain("GET /health [rule-1]");
    expect(setRuleMock).not.toHaveBeenCalled();
  });

  it("충돌 확인을 승인하면 preview candidateId와 확인 flag로 저장한다", async () => {
    previewRuleConflictsMock.mockResolvedValueOnce({
      candidateId: "candidate-1",
      conflicts: [{
        existingRuleId: "rule-1",
        winnerRuleId: "candidate-1",
        loserRuleId: "rule-1",
        kind: "partialOverlap",
        reason: "priority",
      }],
      requiresConfirmation: true,
    });
    confirmMock.mockReturnValueOnce(true);
    render(<App />);
    await screen.findByText("History (2)");

    fireEvent.change(screen.getByLabelText("method"), { target: { value: "GET" } });
    fireEvent.change(screen.getByLabelText("path"), { target: { value: "/health" } });
    fireEvent.change(screen.getByLabelText("priority"), { target: { value: "10" } });
    fireEvent.click(screen.getByRole("button", { name: "규칙 추가" }));

    await waitFor(() => expect(setRuleMock).toHaveBeenCalledWith(expect.objectContaining({
      id: "candidate-1",
      method: "GET",
      path: "/health",
      priority: 10,
    }), true));
    expect(previewRuleConflictsMock.mock.invocationCallOrder[0]).toBeLessThan(setRuleMock.mock.invocationCallOrder[0] ?? Infinity);
  });

  it("OpenAPI 파일 취소와 재선택은 저장 없이 preview만 갱신한다", async () => {
    render(<App />);
    await screen.findByText("History (2)");

    const input = screen.getByLabelText("OpenAPI JSON/YAML 파일 선택") as HTMLInputElement;
    fireEvent.change(input, { target: { files: [] } });
    expect(input.value).toBe("");
    expect(screen.queryByText(/OpenAPI 3\.0/u)).toBeNull();
    expect(setRuleMock).not.toHaveBeenCalled();
    expect(previewRuleConflictsMock).not.toHaveBeenCalled();

    const file = makeOpenApiFile("openapi.json", {
      openapi: "3.0.3",
      paths: { "/health": { get: { responses: { "200": { description: "ok" } } } } },
    });
    fireEvent.change(input, { target: { files: [file] } });
    await screen.findByText(/openapi\.json · OpenAPI 3\.0 · 1개 operation/u);
    expect(input.value).toBe("");

    // A second selection of the same File is accepted because the input was reset.
    fireEvent.change(input, { target: { files: [file] } });
    await waitFor(() => expect(screen.getByText(/openapi\.json · OpenAPI 3\.0 · 1개 operation/u)).toBeTruthy());
    expect(setRuleMock).not.toHaveBeenCalled();
  });

  it("OpenAPI 크기와 확장자를 preview 전에 차단한다", async () => {
    render(<App />);
    await screen.findByText("History (2)");
    const input = screen.getByLabelText("OpenAPI JSON/YAML 파일 선택") as HTMLInputElement;

    const oversized = makeOpenApiFile("large.json", { openapi: "3.0.3", paths: {} });
    Object.defineProperty(oversized, "size", {
      configurable: true,
      value: OPENAPI_DOCUMENT_LIMITS.maxBytes + 1,
    });
    fireEvent.change(input, { target: { files: [oversized] } });
    expect((await screen.findByRole("alert")).textContent).toContain("OpenAPI 파일이 너무 큽니다");
    expect(screen.queryByText(/OpenAPI 3\.0/u)).toBeNull();

    fireEvent.change(input, { target: { files: [makeOpenApiFile("openapi.txt", "openapi: 3.0.3\npaths: {}")] } });
    expect((await screen.findByRole("alert")).textContent).toContain(".json, .yaml, .yml");
    expect(input.value).toBe("");
  });

  it("unmount 뒤 늦게 끝난 OpenAPI 파일 read는 새 view를 갱신하지 않는다", async () => {
    let resolveText!: (value: string) => void;
    const file = makeOpenApiFile("late.json", {
      openapi: "3.0.3",
      paths: { "/late": { get: { responses: { "200": { description: "ok" } } } } },
    });
    Object.defineProperty(file, "text", {
      configurable: true,
      value: () => new Promise<string>((resolve) => { resolveText = resolve; }),
    });

    const view = render(<App />);
    await screen.findByText("History (2)");
    fireEvent.change(screen.getByLabelText("OpenAPI JSON/YAML 파일 선택"), { target: { files: [file] } });
    await waitFor(() => expect(view.container.querySelector(".app")?.getAttribute("aria-busy")).toBe("true"));

    view.unmount();
    resolveText(JSON.stringify({
      openapi: "3.0.3",
      paths: { "/late": { get: { responses: { "200": { description: "ok" } } } } },
    }));
    await Promise.resolve();

    render(<App />);
    await screen.findByText("History (2)");
    expect(screen.queryByText(/late\.json · OpenAPI/u)).toBeNull();
    expect(setRuleMock).not.toHaveBeenCalled();
  });

  it("OpenAPI 부분 오류는 비적용 operation을 이유와 함께 비활성 표시하고 민감 필드를 노출하지 않는다", async () => {
    render(<App />);
    await screen.findByText("History (2)");
    const input = screen.getByLabelText("OpenAPI JSON/YAML 파일 선택");
    fireEvent.change(input, {
      target: {
        files: [makeOpenApiFile("webhooks.json", {
          openapi: "3.0.3",
          info: { title: "Webhook", version: "1" },
          servers: [{ url: "https://private.example" }],
          components: { securitySchemes: { bearer: { type: "http", scheme: "bearer" } } },
          paths: {
            "/events/{id}": {
              get: {
                security: [{ bearer: [] }],
                requestBody: { content: { "application/json": { example: { token: "secret" } } } },
                responses: { "200": { description: "ok" } },
              },
            },
            "/health": {
              post: { responses: { "201": { description: "created" } } },
            },
          },
        })],
      },
    });

    await screen.findByText(/webhooks\.json · OpenAPI 3\.0 · 2개 operation/u);
    const unavailable = screen.getByRole("radio", { name: "GET /events/{id} (200)" }) as HTMLInputElement;
    expect(unavailable.disabled).toBe(true);
    expect(screen.getByText("경로 파라미터({param})가 있어 적용할 수 없습니다.")).toBeTruthy();
    expect((screen.getByRole("radio", { name: "POST /health (201)" }) as HTMLInputElement).disabled).toBe(false);
    expect(document.body.textContent).not.toContain("private.example");
    expect(document.body.textContent).not.toContain("securitySchemes");
    expect(document.body.textContent).not.toContain("requestBody");
    expect(document.body.textContent).not.toContain("secret");
    expect(setRuleMock).not.toHaveBeenCalled();
    expect(previewRuleConflictsMock).not.toHaveBeenCalled();
  });

  it("잘못된 OpenAPI 문서는 고정 오류로 끝나고 draft를 만들지 않는다", async () => {
    render(<App />);
    await screen.findByText("History (2)");
    fireEvent.change(screen.getByLabelText("OpenAPI JSON/YAML 파일 선택"), {
      target: { files: [makeOpenApiFile("legacy.json", { openapi: "2.0", paths: {} })] },
    });

    expect((await screen.findByRole("alert")).textContent).toBe("OpenAPI 3.0 또는 3.1 문서만 사용할 수 있습니다.");
    expect(screen.queryByRole("radio")).toBeNull();
    expect(setRuleMock).not.toHaveBeenCalled();
  });

  it("선택하고 확인한 OpenAPI operation만 editor draft로 적용하며 자동 저장하지 않는다", async () => {
    render(<App />);
    await screen.findByText("History (2)");
    fireEvent.change(screen.getByLabelText("OpenAPI JSON/YAML 파일 선택"), {
      target: {
        files: [makeOpenApiFile("draft.yaml", "openapi: 3.1.0\npaths:\n  /payments:\n    post:\n      responses:\n        '202':\n          description: accepted\n")],
      },
    });

    await screen.findByText(/draft\.yaml · OpenAPI 3\.1 · 1개 operation/u);
    const apply = screen.getByRole("button", { name: "선택한 operation을 rule 초안에 적용" });
    expect(apply.hasAttribute("disabled")).toBe(true);
    fireEvent.click(screen.getByRole("radio", { name: "POST /payments (202)" }));
    expect(apply.hasAttribute("disabled")).toBe(false);

    confirmMock.mockReturnValueOnce(false);
    fireEvent.click(apply);
    expect((screen.getByLabelText("path") as HTMLInputElement).value).toBe("/hook");
    expect(setRuleMock).not.toHaveBeenCalled();
    expect(previewRuleConflictsMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(apply);
    expect((screen.getByLabelText("method") as HTMLInputElement).value).toBe("POST");
    expect((screen.getByLabelText("path") as HTMLInputElement).value).toBe("/payments");
    expect((screen.getByLabelText("status") as HTMLInputElement).value).toBe("202");
    expect((screen.getByLabelText("priority") as HTMLInputElement).value).toBe("0");
    expect(setRuleMock).not.toHaveBeenCalled();
  });

  it("Run Manager definition 다운로드는 실행 중 loopback에서만 활성화한다", async () => {
    render(<App />);
    await screen.findByText("History (2)");
    const stopped = screen.getByRole("button", { name: "Run Manager definition JSON 다운로드" });
    expect(stopped.hasAttribute("disabled")).toBe(true);
    expect(exportRunServiceDefinitionMock).not.toHaveBeenCalled();

    cleanup();
    serverStatusMock.mockResolvedValue({ running: true, address: "0.0.0.0:9000" });
    render(<App />);
    await screen.findByText(/듣는 중 0\.0\.0\.0:9000/u);
    const lan = screen.getByRole("button", { name: "Run Manager definition JSON 다운로드" });
    expect(lan.hasAttribute("disabled")).toBe(true);
    fireEvent.click(lan);
    expect(exportRunServiceDefinitionMock).not.toHaveBeenCalled();
  });

  it("loopback export는 disabled definition JSON을 Blob으로 다운로드한다", async () => {
    serverStatusMock.mockResolvedValue({ running: true, address: "127.0.0.1:9000" });
    let downloaded: Blob | undefined;
    const originalCreateObjectURL = URL.createObjectURL;
    const originalRevokeObjectURL = URL.revokeObjectURL;
    const createObjectURLMock = vi.fn((blob: Blob) => {
      downloaded = blob;
      return "blob:webhook-definition";
    });
    const revokeObjectURLMock = vi.fn();
    Object.defineProperty(URL, "createObjectURL", { configurable: true, value: createObjectURLMock });
    Object.defineProperty(URL, "revokeObjectURL", { configurable: true, value: revokeObjectURLMock });
    const clickMock = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    try {
      render(<App />);
      const button = await screen.findByRole("button", { name: "Run Manager definition JSON 다운로드" });
      expect(button.hasAttribute("disabled")).toBe(false);
      fireEvent.click(button);

      await waitFor(() => expect(exportRunServiceDefinitionMock).toHaveBeenCalledTimes(1));
      await waitFor(() => expect(clickMock).toHaveBeenCalled());
      expect(createObjectURLMock).toHaveBeenCalledWith(expect.any(Blob));
      expect(revokeObjectURLMock).toHaveBeenCalledWith("blob:webhook-definition");
      const parsed = JSON.parse(await downloaded!.text()) as RunDefinitionExport;
      expect(parsed.schemaVersion).toBe(1);
      expect(parsed.services).toHaveLength(1);
      expect(parsed.services[0]).toMatchObject({
        id: "00000000-0000-4000-8000-000000000001",
        command: "exit /b 1",
        enabled: false,
        autoStart: false,
        restartPolicy: "never",
      });
    } finally {
      clickMock.mockRestore();
      Object.defineProperty(URL, "createObjectURL", { configurable: true, value: originalCreateObjectURL });
      Object.defineProperty(URL, "revokeObjectURL", { configurable: true, value: originalRevokeObjectURL });
    }
  });

  it("Run Manager definition 다운로드 오류는 native 내용을 노출하지 않는다", async () => {
    serverStatusMock.mockResolvedValue({ running: true, address: "[::1]:9000" });
    exportRunServiceDefinitionMock.mockRejectedValueOnce(new Error("native profile path /private/raw-token"));
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Run Manager definition JSON 다운로드" }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("Run Manager definition을 다운로드하지 못했습니다. 서버 상태를 확인한 뒤 다시 시도하세요.");
    expect(document.body.textContent).not.toContain("raw-token");
  });

  it("Run Manager definition의 고정된 profile 제한 사유는 안전하게 안내한다", async () => {
    serverStatusMock.mockResolvedValue({ running: true, address: "127.0.0.1:9000" });
    exportRunServiceDefinitionMock.mockRejectedValueOnce(
      new Error("Webhook service profile 개수 제한에 도달했습니다"),
    );
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Run Manager definition JSON 다운로드" }));

    expect((await screen.findByRole("alert")).textContent).toBe(
      "Webhook service profile 개수 제한에 도달했습니다",
    );
  });

  it("backend 원문 오류를 고정된 안전 메시지로 대체한다", async () => {
    setRuleMock.mockRejectedValueOnce(new Error("secret/path=/tmp/private-token"));
    render(<App />);
    await screen.findByText("History (2)");

    fireEvent.click(screen.getByRole("button", { name: "규칙 추가" }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("요청을 처리하지 못했습니다. 입력과 서버 상태를 확인하세요.");
    expect(document.body.textContent).not.toContain("private-token");
  });

  it("우클릭한 요청을 먼저 선택하고 정확한 메뉴와 후속 기능 경계를 표시한다", async () => {
    render(<App />);
    const target = await screen.findByLabelText("GET /health 요청") as HTMLDivElement;

    fireEvent.contextMenu(target, { clientX: 20, clientY: 20 });

    expect(target.getAttribute("aria-current")).toBe("true");
    for (const label of ["마스킹 복사", "원본 복사", "헤더 복사", "API Playground로 변환", "Log Lens에서 보기", "삭제"]) {
      expect(screen.getByRole("menuitem", { name: label })).toBeTruthy();
    }
    expect(screen.getByRole("menuitem", { name: "API Playground로 변환" }).getAttribute("aria-disabled"))
      .toBeNull();
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

  it("unmount 뒤 늦게 도착한 복사 결과는 clipboard side effect를 만들지 않는다", async () => {
    let release!: (value: string) => void;
    copyMaskedHistoryMock.mockReturnValueOnce(new Promise((resolve) => {
      release = resolve;
    }));
    render(<App />);
    const target = await screen.findByLabelText("GET /health 요청") as HTMLDivElement;

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "마스킹 복사" }));
    await waitFor(() => expect(copyMaskedHistoryMock).toHaveBeenCalledWith(2));

    cleanup();
    release("late-secret");
    await Promise.resolve();
    expect(writeTextMock).not.toHaveBeenCalled();
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
    rules = [{ ...initialRule, priority: 37 }];
    render(<App />);
    const target = await screen.findByLabelText("GET /health 규칙") as HTMLDivElement;

    fireEvent.contextMenu(target);
    expect(target.getAttribute("aria-current")).toBe("true");
    expect(screen.getByRole("menuitem", { name: "PowerShell curl.exe 복사" }).getAttribute("aria-disabled"))
      .toBe("true");
    expect(screen.getByRole("menuitem", { name: "POSIX sh curl 복사" }).getAttribute("aria-disabled"))
      .toBe("true");
    fireEvent.click(screen.getByRole("menuitem", { name: "편집" }));
    expect((screen.getByPlaceholderText("method (없으면 전체)") as HTMLInputElement).value).toBe("GET");
    expect((screen.getByLabelText("priority") as HTMLInputElement).value).toBe("37");
    expect(screen.getByRole("button", { name: "규칙 저장" })).toBeTruthy();

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "복제" }));
    await waitFor(() => expect(setRuleMock).toHaveBeenCalledWith(expect.objectContaining({ id: "rule-2", path: "/health", priority: 37 }), false));
  });

  it("실행 중인 rule의 example curl을 마스킹해 복사한다", async () => {
    serverStatusMock.mockResolvedValue({ running: true, address: "127.0.0.1:9000" });
    rules = [{
      ...initialRule,
      headers: [
        ["Authorization", "Bearer rule-secret"],
        ["Content-Type", "application/json"],
      ] as [string, string][],
      body: JSON.stringify({ token: "body-secret", ok: true }),
    }];
    render(<App />);
    const target = await screen.findByLabelText("GET /health 규칙") as HTMLDivElement;

    fireEvent.contextMenu(target);
    const copy = screen.getByRole("menuitem", { name: "POSIX sh curl 복사" });
    expect(copy.getAttribute("aria-disabled")).toBeNull();
    fireEvent.click(copy);

    await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith(expect.stringContaining(
      "curl --globoff --path-as-is --include --request GET 'http://127.0.0.1:9000/health'",
    )));
    const copied = writeTextMock.mock.calls[writeTextMock.mock.calls.length - 1]?.[0] ?? "";
    expect(copied).toContain("Authorization: [REDACTED]");
    expect(copied).not.toContain("rule-secret");
    expect(copied).not.toContain("body-secret");
  });

  it("PowerShell action copies curl.exe and revalidates the running address", async () => {
    serverStatusMock
      .mockResolvedValueOnce({ running: true, address: "0.0.0.0:9000" })
      .mockResolvedValueOnce({ running: true, address: "0.0.0.0:9000" });
    rules = [{ ...initialRule, path: "/events/*", method: "post" }];
    render(<App />);
    const target = await screen.findByLabelText("post /events/* 규칙") as HTMLDivElement;

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "PowerShell curl.exe 복사" }));

    await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith(expect.stringContaining(
      "curl.exe --globoff --path-as-is --include --request POST 'http://127.0.0.1:9000/events/example'",
    )));
    expect(serverStatusMock).toHaveBeenCalledTimes(2);
    expect(writeTextMock.mock.calls[writeTextMock.mock.calls.length - 1]?.[0]).toContain("Concrete trailing-* sample path: /events/example");
  });

  it("replays a selected masked history request only when the local server is running", async () => {
    serverStatusMock.mockResolvedValue({ running: true, address: "127.0.0.1:9000" });
    render(<App />);
    const target = await screen.findByLabelText("POST /hook 요청") as HTMLDivElement;

    fireEvent.contextMenu(target);
    const replay = screen.getByRole("menuitem", { name: "masked 요청 replay" });
    expect(replay.getAttribute("aria-disabled")).toBeNull();
    fireEvent.click(replay);

    await waitFor(() => expect(replayHistoryMock).toHaveBeenCalledWith(1));
    expect(screen.getByRole("status").textContent).toContain("localhost에 replay했습니다");
    expect(screen.getByRole("status").textContent).toContain("status: 200");
    expect(writeTextMock).not.toHaveBeenCalled();
  });

  it("replay errors stay fixed and never expose a source or response detail", async () => {
    serverStatusMock.mockResolvedValue({ running: true, address: "127.0.0.1:9000" });
    replayHistoryMock.mockRejectedValueOnce(new Error("Bearer raw-replay-secret at C:\\private"));
    render(<App />);
    const target = await screen.findByLabelText("POST /hook 요청") as HTMLDivElement;

    fireEvent.click(target);
    fireEvent.click(screen.getByRole("button", { name: "POST /hook masked replay" }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("요청을 처리하지 못했습니다. 입력과 서버 상태를 확인하세요.");
    expect(document.body.textContent).not.toContain("raw-replay-secret");
    expect(document.body.textContent).not.toContain("C:\\private");
  });

  it("edits a bounded response sequence and sends it with the rule", async () => {
    render(<App />);
    await screen.findByText("History (2)");

    fireEvent.click(screen.getByRole("button", { name: /응답 단계 추가/ }));
    fireEvent.change(screen.getByRole("spinbutton", { name: "응답 단계 1 status" }), {
      target: { value: "503" },
    });
    fireEvent.change(screen.getByRole("spinbutton", { name: "응답 단계 1 delay" }), {
      target: { value: "100" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "응답 단계 1 body" }), {
      target: { value: "retry" },
    });
    fireEvent.click(screen.getByRole("button", { name: "규칙 추가" }));

    await waitFor(() => expect(setRuleMock).toHaveBeenCalledWith(expect.objectContaining({
      sequence: [{
        status: 503,
        headers: [],
        body: "retry",
        delayMs: 100,
      }],
    }), false));
  });

  it("resets the selected response sequence without changing the rule", async () => {
    rules = [{
      ...initialRule,
      sequence: [{ status: 500, headers: [], body: "retry", delayMs: 0 }],
    }];
    render(<App />);
    await screen.findByLabelText("GET /health 규칙");
    expect(screen.getByText("2 responses")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "GET /health response sequence 초기화" }));
    await waitFor(() => expect(resetRuleSequenceMock).toHaveBeenCalledWith("rule-1"));
    expect(setRuleMock).not.toHaveBeenCalled();
    expect(screen.getByRole("status").textContent).toContain("첫 응답으로 초기화했습니다");
  });

  it("exposes sequence reset from the rule context menu and preserves focus", async () => {
    rules = [{
      ...initialRule,
      sequence: [{ status: 500, headers: [], body: "retry", delayMs: 0 }],
    }];
    render(<App />);
    const target = await screen.findByLabelText("GET /health 규칙") as HTMLDivElement;
    target.focus();
    fireEvent.keyDown(target, { key: "F10", shiftKey: true });

    fireEvent.click(screen.getByRole("menuitem", { name: "response sequence 초기화" }));
    await waitFor(() => expect(resetRuleSequenceMock).toHaveBeenCalledWith("rule-1"));
    await waitFor(() => expect(document.activeElement).toBe(target));
  });

  it("stale rule selection fails closed without copying", async () => {
    serverStatusMock.mockResolvedValue({ running: true, address: "127.0.0.1:9000" });
    render(<App />);
    const target = await screen.findByLabelText("GET /health 규칙") as HTMLDivElement;

    fireEvent.contextMenu(target);
    rules = [];
    fireEvent.click(screen.getByRole("menuitem", { name: "POSIX sh curl 복사" }));

    expect((await screen.findByRole("alert")).textContent).toContain("선택한 규칙이 더 이상 존재하지 않습니다.");
    expect(writeTextMock).not.toHaveBeenCalled();
  });

  it("rechecks running state before copying when the server stopped after menu open", async () => {
    serverStatusMock
      .mockResolvedValueOnce({ running: true, address: "127.0.0.1:9000" })
      .mockResolvedValueOnce({ running: false, address: null });
    render(<App />);
    const target = await screen.findByLabelText("GET /health 규칙") as HTMLDivElement;

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "POSIX sh curl 복사" }));

    expect((await screen.findByRole("alert")).textContent).toContain("현재 서버가 실행 중이 아니거나 주소가 유효하지 않아 예시 curl을 만들지 못했습니다.");
    expect(writeTextMock).not.toHaveBeenCalled();
  });

  it("keyboard menu restores rule focus after Escape", async () => {
    serverStatusMock.mockResolvedValue({ running: true, address: "127.0.0.1:9000" });
    render(<App />);
    const target = await screen.findByLabelText("GET /health 규칙") as HTMLDivElement;
    target.focus();
    fireEvent.keyDown(target, { key: "F10", shiftKey: true });

    expect(screen.getByRole("menuitem", { name: "PowerShell curl.exe 복사" }).getAttribute("aria-disabled")).toBeNull();
    expect(screen.getByRole("menu")).toBeTruthy();
    fireEvent.keyDown(screen.getByRole("menuitem", { name: "편집" }), { key: "Escape" });
    expect(document.activeElement).toBe(target);
  });

  it("does not start a second clipboard action while the first is busy", async () => {
    serverStatusMock.mockResolvedValue({ running: true, address: "127.0.0.1:9000" });
    let resolveWrite: (() => void) | undefined;
    writeTextMock.mockImplementation(() => new Promise<void>((resolve) => {
      resolveWrite = resolve;
    }));
    render(<App />);
    const target = await screen.findByLabelText("GET /health 규칙") as HTMLDivElement;

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "POSIX sh curl 복사" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledTimes(1));

    fireEvent.contextMenu(target);
    expect(screen.queryByRole("menuitem", { name: "POSIX sh curl 복사" })).toBeNull();
    expect(writeTextMock).toHaveBeenCalledTimes(1);
    resolveWrite?.();
  });

  it("example curl clipboard 실패는 고정 메시지만 표시한다", async () => {
    serverStatusMock.mockResolvedValue({ running: true, address: "127.0.0.1:9000" });
    rules = [{
      ...initialRule,
      headers: [["Authorization", "Bearer rule-secret"]] as [string, string][],
    }];
    writeTextMock.mockRejectedValueOnce(new Error("Bearer backend-secret"));
    render(<App />);
    const target = await screen.findByLabelText("GET /health 규칙") as HTMLDivElement;

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "POSIX sh curl 복사" }));

    expect((await screen.findByRole("alert")).textContent).toContain("예시 curl을 복사하지 못했습니다.");
    expect(document.body.textContent?.includes("backend-secret")).toBe(false);
    expect(document.body.textContent?.includes("rule-secret")).toBe(false);
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

  it("history context action saves one masked fixture and exposes a stable action label", async () => {
    render(<App />);
    const target = await screen.findByLabelText("POST /hook 요청") as HTMLDivElement;

    fireEvent.contextMenu(target);
    const save = screen.getByRole("menuitem", { name: "masked fixture 저장" });
    expect(save.getAttribute("aria-disabled")).toBeNull();
    fireEvent.click(save);

    await waitFor(() => expect(saveFixtureMock).toHaveBeenCalledWith(1));
    await screen.findByText("Fixtures (1)");
    expect(screen.getByLabelText("POST /hook fixture")).toBeTruthy();
    expect(screen.getByText(/원본 header·credential·안전하지 않은 path는 저장하지 않습니다/)).toBeTruthy();
  });

  it("history handoff uses the backend producer and never falls back to clipboard", async () => {
    render(<App />);
    const target = await screen.findByLabelText("POST /hook 요청") as HTMLDivElement;

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "API Playground로 변환" }));

    await waitFor(() => expect(sendHistoryToApiMock).toHaveBeenCalledWith(1));
    expect((await screen.findByRole("status")).textContent).toContain("producer: webhook-lab");
    expect(screen.getByRole("status").textContent).toContain("consumer: api-playground");
    expect(writeTextMock).not.toHaveBeenCalled();
  });

  it("history Log Lens action sends only the opaque history ID", async () => {
    render(<App />);
    const target = await screen.findByLabelText("POST /hook 요청") as HTMLDivElement;

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "Log Lens에서 보기" }));

    await waitFor(() => expect(sendHistoryToLogLensMock).toHaveBeenCalledWith(1));
    expect(sendHistoryToLogLensMock.mock.calls[0]).toEqual([1]);
    expect((await screen.findByRole("status")).textContent).toContain("consumer: log-lens");
    expect(writeTextMock).not.toHaveBeenCalled();
  });

  it("fixture save uses the shared busy guard for double action", async () => {
    let release!: (fixture: CapturedFixture) => void;
    saveFixtureMock.mockReturnValueOnce(new Promise((resolve) => { release = resolve; }));
    render(<App />);
    await screen.findByLabelText("GET /health 요청");
    const visibleSave = screen.getByRole("button", { name: "GET /health masked fixture 저장" });

    fireEvent.click(visibleSave);
    fireEvent.click(visibleSave);
    await waitFor(() => expect(saveFixtureMock).toHaveBeenCalledTimes(1));
    expect(visibleSave.hasAttribute("disabled")).toBe(true);
    expect(screen.getByRole("button", { name: "GET /health masked fixture 저장" }).closest(".app")?.getAttribute("aria-busy"))
      .toBe("true");

    release({
      id: "fixture-2",
      method: "GET",
      url: "/health",
      headers: [],
      body: "",
      receivedAtMs: 1_700_000_001_000,
    });
    await waitFor(() => expect(visibleSave.hasAttribute("disabled")).toBe(false));
  });

  it("fixture action converts a validated fixture to a local response-rule draft", async () => {
    fixtures = [{
      id: "fixture-1",
      method: "POST",
      url: "/hooks/push?token=[REDACTED]",
      headers: [["Authorization", "[REDACTED]"]],
      body: "{\"token\":\"[REDACTED]\"}",
      receivedAtMs: 1_700_000_000_000,
    }];
    fixtureToRuleMock.mockResolvedValueOnce({
      id: "",
      priority: 0,
      method: "POST",
      path: "/hooks/push?token=[REDACTED]",
      status: 200,
      headers: [],
      body: "",
      delayMs: 0,
    });
    render(<App />);
    await screen.findByLabelText("POST /hooks/push?token=[REDACTED] fixture");
    const draft = screen.getByRole("button", { name: "POST /hooks/push?token=[REDACTED] 응답 rule 초안" });
    fireEvent.click(draft);

    await waitFor(() => expect(fixtureToRuleMock).toHaveBeenCalledWith("fixture-1"));
    expect((screen.getByLabelText("method") as HTMLInputElement).value).toBe("POST");
    expect((screen.getByLabelText("path") as HTMLInputElement).value).toBe("/hooks/push?token=[REDACTED]");
    expect((screen.getByLabelText("status") as HTMLInputElement).value).toBe("200");
    expect((screen.getByLabelText("priority") as HTMLInputElement).value).toBe("0");
  });

  it("stored fixture handoff carries only its opaque ID", async () => {
    fixtures = [{
      id: "fixture-1",
      method: "POST",
      url: "/hooks/push?access_token=[REDACTED]",
      headers: [],
      body: '{"event":"push"}',
      receivedAtMs: 1_700_000_000_000,
    }];
    render(<App />);
    const action = await screen.findByRole("button", { name: "POST /hooks/push?access_token=[REDACTED] API Playground로 변환" });
    fireEvent.click(action);

    await waitFor(() => expect(sendFixtureToApiMock).toHaveBeenCalledWith("fixture-1"));
    expect(screen.getByRole("status").textContent).toContain("handoff: fedcba9876543210fedcba9876543210");
    expect(writeTextMock).not.toHaveBeenCalled();
  });

  it("stored fixture Log Lens action sends only the opaque fixture ID", async () => {
    fixtures = [{
      id: "fixture-1",
      method: "POST",
      url: "/hooks/push?access_token=[REDACTED]",
      headers: [],
      body: '{"event":"push"}',
      receivedAtMs: 1_700_000_000_000,
    }];
    render(<App />);
    const action = await screen.findByRole("button", { name: "POST /hooks/push?access_token=[REDACTED] Log Lens에서 보기" });
    fireEvent.click(action);

    await waitFor(() => expect(sendFixtureToLogLensMock).toHaveBeenCalledWith("fixture-1"));
    expect(sendFixtureToLogLensMock.mock.calls[0]).toEqual(["fixture-1"]);
    expect((await screen.findByRole("status")).textContent).toContain("consumer: log-lens");
    expect(writeTextMock).not.toHaveBeenCalled();
  });

  it("fixture 삭제와 전체 삭제는 각각 확인을 거친다", async () => {
    fixtures = [
      { id: "fixture-1", method: "POST", url: "/hook", headers: [], body: "", receivedAtMs: 1_700_000_000_000 },
      { id: "fixture-2", method: "GET", url: "/health", headers: [], body: "", receivedAtMs: 1_700_000_001_000 },
    ];
    render(<App />);
    await screen.findByText("Fixtures (2)");

    fireEvent.click(screen.getByRole("button", { name: "POST /hook fixture 삭제" }));
    expect(deleteFixtureMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(screen.getByRole("button", { name: "POST /hook fixture 삭제" }));
    await waitFor(() => expect(deleteFixtureMock).toHaveBeenCalledWith("fixture-1"));
    await screen.findByText("Fixtures (1)");

    const clear = screen.getByRole("button", { name: "저장된 fixture 모두 삭제" });
    fireEvent.click(clear);
    expect(clearFixturesMock).not.toHaveBeenCalled();
    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(clear);
    await waitFor(() => expect(clearFixturesMock).toHaveBeenCalledTimes(1));
    await screen.findByText("Fixtures (0)");
  });

  it("fixture storage failures stay fixed and do not reflect filesystem or secret details", async () => {
    listFixturesMock.mockRejectedValue(new Error("/tmp/private/fixture-secret.json: Bearer raw-secret"));
    render(<App />);
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("요청을 처리하지 못했습니다. 입력과 서버 상태를 확인하세요.");
    expect(document.body.textContent).not.toContain("private/fixture-secret");
    expect(document.body.textContent).not.toContain("raw-secret");
  });

  it("LAN start requires a second explicit confirmation and sends the allow flag", async () => {
    confirmMock.mockReturnValueOnce(false);
    render(<App />);
    await screen.findByText(/중지/, { selector: ".status" });
    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "시작" }));
    expect(startServerMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(screen.getByRole("button", { name: "시작" }));
    await waitFor(() => expect(startServerMock).toHaveBeenCalledWith("0.0.0.0", 9000, true));
  });
});
