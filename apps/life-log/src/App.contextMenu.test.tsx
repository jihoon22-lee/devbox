import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App, { toDateStr } from "./App";
import type { DigestInput, DigestResponse } from "./api";

const mocks = vi.hoisted(() => ({
  native: false,
  writeText: vi.fn<(value: string) => Promise<void>>(),
  exportLifeLog: vi.fn(),
  cancelDigest: vi.fn().mockResolvedValue(false),
  getDigest: vi.fn(),
  getDay: vi.fn(),
  saveLifeLog: vi.fn(),
  sendDigestToKnowledge: vi.fn(),
}));

vi.mock("./lib/isTauri", () => ({ isTauri: () => mocks.native }));

vi.mock("./api", () => ({
  autostartStatus: vi.fn().mockResolvedValue({ supported: true, enabled: false, command: null }),
  cancelDigest: mocks.cancelDigest,
  exportLifeLog: mocks.exportLifeLog,
  getDigest: mocks.getDigest,
  getAppStats: vi.fn().mockResolvedValue([]),
  getDay: mocks.getDay.mockImplementation(async (date: string) => ({
    date,
    pc_usage_ms: 3_600_000,
    app_totals: [],
    git: { projects: [], total_commits: 0 },
  })),
  getIdleThreshold: vi.fn().mockResolvedValue(300_000),
  getPrivacyRules: vi.fn().mockResolvedValue({
    excludedProcesses: [],
    excludedTitlePatterns: [],
    redactTitlePatterns: [],
    maskAllTitles: false,
  }),
  getProjects: vi.fn().mockResolvedValue([]),
  getRange: vi.fn().mockResolvedValue({
    label: "fixture range",
    pc_usage_ms: 5_400_000,
    app_totals: [],
    git: { projects: [], total_commits: 0 },
    daily: [
      { day_ms: new Date(2024, 0, 1).getTime(), pc_usage_ms: 1_800_000 },
      { day_ms: new Date(2024, 0, 2).getTime(), pc_usage_ms: 3_600_000 },
    ],
  }),
  getTimeline: vi.fn().mockResolvedValue([]),
  integrationSources: vi.fn().mockResolvedValue([]),
  isTracking: vi.fn().mockResolvedValue(false),
  projectAttribution: vi.fn().mockResolvedValue({
    attributed: [],
    unattributed: { projectId: "unattributed", sessions: 0, durationMs: 0 },
    profileCount: 0,
  }),
  redactExisting: vi.fn().mockResolvedValue(0),
  setAutostart: vi.fn(),
  setIdleThreshold: vi.fn(),
  setPrivacyRules: vi.fn(),
  setProjects: vi.fn(),
  saveLifeLog: mocks.saveLifeLog,
  sendDigestToKnowledge: mocks.sendDigestToKnowledge,
  startTracking: vi.fn().mockResolvedValue(true),
  stopTracking: vi.fn().mockResolvedValue(undefined),
}));

function digestFixture(input: DigestInput): DigestResponse {
  return {
    origin: "browser-preview",
    document: {
      schemaVersion: 1,
      period: input.period,
      range: {
        startDate: input.startDate,
        endDate: input.endDate,
        timezone: input.timezone,
        startMs: input.dayStart,
        endMs: input.dayEnd,
        dayBoundaries: input.dayBoundaries,
      },
      filter: input.filter,
      rules: {
        sessionWindow: "session window",
        sessionDuration: "session duration",
        dailyBuckets: "daily buckets",
        appFilter: "app filter",
        appTotals: "app totals",
        gitCommits: "git commits",
        snapshotScope: "snapshot scope",
        privacy: "privacy",
        externalProcessing: "external processing",
      },
      headline: `${input.period} fixture`,
      summary: {
        pcUsageMs: 0,
        sessionCount: 0,
        activeDays: 0,
        totalDays: input.dayBoundaries.length,
        averageDailyUsageMs: 0,
        topApp: null,
        gitCommits: 0,
      },
      daily: input.dayBoundaries.map((boundary) => ({
        date: boundary.date,
        startMs: boundary.startMs,
        endMs: boundary.endMs,
        pcUsageMs: 0,
        sessionCount: 0,
        gitCommits: 0,
        topApp: null,
        hasActivity: false,
      })),
      appTotals: [],
      git: { projects: [], totalCommits: 0, errorCodes: ["browser_preview_only"] },
      sources: ["life-log", "git", "run-manager", "knowledge-base"].map((id) => ({
        id,
        available: false,
        schemaVersion: null,
        snapshotVersion: null,
        producerVersion: null,
        generatedAt: null,
        freshnessMs: null,
        view: null,
        scope: "browser-preview-only",
        errorCode: "browser_preview_only",
      })),
    },
    markdown: "# Life Log local digest\n",
  };
}

beforeEach(() => {
  mocks.native = false;
  mocks.writeText.mockReset().mockResolvedValue(undefined);
  mocks.exportLifeLog.mockReset().mockResolvedValue({
    origin: "browser-preview",
    format: "markdown",
    extension: "md",
    mimeType: "text/markdown;charset=utf-8",
    byteLength: 10,
    content: "# fixture\n",
  });
  mocks.getDigest.mockReset().mockImplementation((input: DigestInput) => Promise.resolve(digestFixture(input)));
  mocks.sendDigestToKnowledge.mockReset().mockResolvedValue({
    id: "0123456789abcdef0123456789abcdef",
    kind: "knowledge-draft/v1",
    expiresAtMs: Date.now() + 600_000,
  });
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: mocks.writeText },
  });
  Object.defineProperty(URL, "createObjectURL", {
    configurable: true,
    value: vi.fn(() => "blob:life-log-fixture"),
  });
  Object.defineProperty(URL, "revokeObjectURL", {
    configurable: true,
    value: vi.fn(),
  });
});

afterEach(() => cleanup());

async function renderLoadedApp() {
  render(<App />);
  await screen.findByRole("heading", { name: "Daily local digest" });
  await waitFor(() => expect(screen.queryByText("Loading...")).toBeNull());
  return screen.getByLabelText(/\d{4}-\d{2}-\d{2} 선택된 날짜/u) as HTMLInputElement;
}

describe("Life Log daily digest", () => {
  it("exposes day source/rule provenance and explicit copy/download actions", async () => {
    await renderLoadedApp();

    expect(await screen.findByRole("heading", { name: "Daily local digest" })).toBeTruthy();
    expect(mocks.getDay).not.toHaveBeenCalled();
    expect(screen.getByText(/Browser preview only · native local data unavailable/u)).toBeTruthy();
    expect(screen.getByLabelText("Application filter")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Copy digest" }));
    await waitFor(() => expect(mocks.writeText).toHaveBeenCalledWith("# Life Log local digest\n"));

    fireEvent.click(screen.getByText("Sources and aggregation rules"));
    expect(screen.getByText("life-log")).toBeTruthy();
    expect(screen.getByText("sessionWindow")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Download preview" }));
    await waitFor(() => expect(URL.createObjectURL).toHaveBeenCalled());
  });

  it("clears stale digest state before a newer navigation request completes", async () => {
    let firstInput: DigestInput | null = null;
    let resolveFirst!: (response: DigestResponse) => void;
    let resolveSecond!: () => void;
    const first = new Promise<DigestResponse>((resolve) => { resolveFirst = resolve; });
    const second = new Promise<void>((resolve) => { resolveSecond = resolve; });
    mocks.getDigest
      .mockReset()
      .mockImplementationOnce((input: DigestInput) => {
        firstInput = input;
        return first;
      })
      .mockImplementationOnce((input: DigestInput) => second.then(() => digestFixture(input)));

    render(<App />);
    await waitFor(() => expect(mocks.getDigest).toHaveBeenCalledTimes(1));
    const dateInput = screen.getByLabelText(/\d{4}-\d{2}-\d{2} 선택된 날짜/u) as HTMLInputElement;
    const initialDate = dateInput.value;
    fireEvent.click(screen.getByRole("button", { name: "다음 날짜" }));
    await waitFor(() => expect(mocks.getDigest).toHaveBeenCalledTimes(2));
    expect(dateInput.value).not.toBe(initialDate);
    expect(screen.queryByRole("heading", { name: "Daily local digest" })).toBeNull();

    resolveFirst(digestFixture(firstInput!));
    await Promise.resolve();
    expect(screen.queryByText("day fixture")).toBeNull();

    resolveSecond();
    await screen.findByRole("heading", { name: "Daily local digest" });
    expect(dateInput.value).not.toBe(initialDate);
  });

  it("keeps Knowledge handoff disabled in browser preview without creating an IPC side effect", async () => {
    await renderLoadedApp();

    const handoff = screen.getByRole("button", { name: "Send to Knowledge" });
    expect((handoff as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(handoff);
    expect(mocks.sendDigestToKnowledge).not.toHaveBeenCalled();
  });

  it("sends the current native digest once and reports preview-before-save", async () => {
    mocks.native = true;
    await renderLoadedApp();

    const handoff = screen.getByRole("button", { name: "Send to Knowledge" });
    expect((handoff as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(handoff);
    fireEvent.click(handoff);

    await waitFor(() => expect(mocks.sendDigestToKnowledge).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("Knowledge draft를 미리보기로 보냈습니다. 저장 전 내용을 확인하세요.")).toBeTruthy();
    expect(mocks.sendDigestToKnowledge).toHaveBeenCalledWith(expect.objectContaining({
      period: "day",
    }));
  });
});

describe("Life Log date context menu", () => {
  it("선택 날짜 입력에 exact menu를 열고 날짜만 복사한 뒤 focus를 복원한다", async () => {
    const dateInput = await renderLoadedApp();
    const selectedDate = dateInput.value;
    dateInput.focus();

    fireEvent.contextMenu(dateInput, { clientX: 20, clientY: 24 });

    expect(screen.getByRole("menuitem", { name: "날짜 복사" }).getAttribute("aria-disabled")).toBeNull();
    expect(screen.getByRole("menuitem", { name: "Markdown 내보내기" }).getAttribute("aria-disabled")).toBeNull();
    expect(screen.getByRole("menuitem", { name: "JSON 내보내기" }).getAttribute("aria-disabled")).toBeNull();
    expect(screen.getByRole("menuitem", { name: "CSV 내보내기" }).getAttribute("aria-disabled")).toBeNull();

    fireEvent.click(screen.getByRole("menuitem", { name: "날짜 복사" }));

    await waitFor(() => expect(mocks.writeText).toHaveBeenCalledWith(selectedDate));
    expect(await screen.findByText(`${selectedDate} 날짜를 복사했습니다.`)).toBeTruthy();
    await waitFor(() => expect(document.activeElement).toBe(dateInput));
  });

  it("명시적으로 선택한 날짜의 Markdown export만 browser download fixture로 만든다", async () => {
    const dateInput = await renderLoadedApp();
    fireEvent.contextMenu(dateInput);
    fireEvent.click(screen.getByRole("menuitem", { name: "Markdown 내보내기" }));

    await waitFor(() => expect(mocks.exportLifeLog).toHaveBeenCalledWith(expect.objectContaining({
      startDate: dateInput.value,
      endDate: dateInput.value,
      format: "markdown",
    })));
    expect(await screen.findByText(/MARKDOWN export를 브라우저 미리보기로 다운로드했습니다/u)).toBeTruthy();
  });

  it("키보드로 연 exact chart date를 먼저 선택하고 그 날짜를 복사한다", async () => {
    const dateInput = await renderLoadedApp();
    const chartDate = new Date(`${dateInput.value}T00:00:00`);
    chartDate.setDate(chartDate.getDate() + 1);
    const chartDateKey = toDateStr(chartDate);
    fireEvent.click(screen.getByRole("button", { name: "Week" }));
    const target = await screen.findByRole("button", { name: `${chartDateKey} 날짜` });
    target.focus();

    fireEvent.keyDown(target, { key: "F10", code: "F10", shiftKey: true });

    await waitFor(() => expect(dateInput.value).toBe(chartDateKey));
    await waitFor(() => expect(screen.queryByText("Loading...")).toBeNull());
    const currentTarget = await screen.findByRole("button", { name: `${chartDateKey} 날짜` });
    expect(currentTarget.getAttribute("aria-current")).toBe("date");
    fireEvent.click(screen.getByRole("menuitem", { name: "날짜 복사" }));
    await waitFor(() => expect(mocks.writeText).toHaveBeenCalledWith(chartDateKey));
    await waitFor(() => expect(document.activeElement).toBe(currentTarget));
  });

  it("invalid target은 이전 context date로 retarget하지 않는다", async () => {
    const dateInput = await renderLoadedApp();
    fireEvent.contextMenu(dateInput);
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });

    dateInput.dataset.date = "credential-raw";
    fireEvent.contextMenu(dateInput);

    const copy = screen.getByRole("menuitem", { name: "날짜 복사" });
    expect(copy.getAttribute("aria-disabled")).toBe("true");
    fireEvent.click(copy);
    expect(mocks.writeText).not.toHaveBeenCalled();
  });

  it("clipboard 실패는 raw 오류를 반향하지 않고 고정 안내만 표시한다", async () => {
    const raw = "C:\\secret\\life-log credential-raw";
    mocks.writeText.mockRejectedValueOnce(new Error(raw));
    const dateInput = await renderLoadedApp();

    fireEvent.keyDown(dateInput, { key: "ContextMenu", code: "ContextMenu" });
    fireEvent.click(screen.getByRole("menuitem", { name: "날짜 복사" }));

    expect(await screen.findByText("날짜를 클립보드에 복사하지 못했습니다.")).toBeTruthy();
    expect(document.body.textContent).not.toContain(raw);
  });

  it("export 실패는 native/path 오류를 반향하지 않고 고정 안내만 표시한다", async () => {
    const raw = "C:\\secret\\life-log credential-raw";
    mocks.exportLifeLog.mockRejectedValueOnce(new Error(raw));
    const dateInput = await renderLoadedApp();

    fireEvent.contextMenu(dateInput);
    fireEvent.click(screen.getByRole("menuitem", { name: "Markdown 내보내기" }));

    expect(await screen.findByText("MARKDOWN export 미리보기를 다운로드하지 못했습니다.")).toBeTruthy();
    expect(document.body.textContent).not.toContain(raw);
  });

  it("range preview modal은 initial focus, Escape close, accessible labelling을 제공한다", async () => {
    await renderLoadedApp();
    const open = screen.getByRole("button", { name: "Export preview" });
    open.focus();
    fireEvent.click(open);

    const dialog = await screen.findByRole("dialog", { name: "Life Log export" });
    expect(dialog.getAttribute("aria-describedby")).toBe("life-log-export-description");
    await waitFor(() => expect(document.activeElement).toBe(screen.getByLabelText("시작 날짜")));

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Life Log export" })).toBeNull());
    expect(document.activeElement).toBe(open);
  });

  it("range preview modal은 Tab을 양 끝에서 순환시킨다", async () => {
    await renderLoadedApp();
    fireEvent.click(screen.getByRole("button", { name: "Export preview" }));

    const dialog = await screen.findByRole("dialog", { name: "Life Log export" });
    const first = screen.getByLabelText("시작 날짜");
    const last = screen.getByRole("button", { name: "미리보기 다운로드" });
    await waitFor(() => expect(document.activeElement).toBe(first));

    last.focus();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(document.activeElement).toBe(first);

    fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(last);
    expect(dialog.getAttribute("aria-busy")).toBe("false");
  });

  it("busy export는 double action을 막고 모든 modal action을 비활성화한다", async () => {
    let resolveExport!: (value: unknown) => void;
    const pending = new Promise((resolve) => {
      resolveExport = resolve;
    });
    mocks.exportLifeLog.mockReturnValueOnce(pending);
    await renderLoadedApp();
    fireEvent.click(screen.getByRole("button", { name: "Export preview" }));

    const dialog = await screen.findByRole("dialog", { name: "Life Log export" });
    const cancel = screen.getByRole("button", { name: "취소" });
    const submit = screen.getByRole("button", { name: "미리보기 다운로드" });
    fireEvent.click(submit);
    await waitFor(() => expect(dialog.getAttribute("aria-busy")).toBe("true"));
    expect(cancel.hasAttribute("disabled")).toBe(true);
    expect(submit.hasAttribute("disabled")).toBe(true);
    expect((screen.getByLabelText("시작 날짜") as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByLabelText("종료 날짜") as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByLabelText("형식") as HTMLSelectElement).disabled).toBe(true);

    fireEvent.click(submit);
    expect(mocks.exportLifeLog).toHaveBeenCalledTimes(1);
    resolveExport({
      origin: "browser-preview",
      format: "markdown",
      extension: "md",
      mimeType: "text/markdown;charset=utf-8",
      byteLength: 10,
      content: "# fixture\n",
    });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Life Log export" })).toBeNull());
  });

  it("unmount가 pending export의 stale completion을 무효화한다", async () => {
    let resolveExport!: (value: unknown) => void;
    const pending = new Promise((resolve) => {
      resolveExport = resolve;
    });
    mocks.exportLifeLog.mockReturnValueOnce(pending);
    const { unmount } = render(<App />);
    await screen.findByRole("heading", { name: "Daily local digest" });
    await waitFor(() => expect(screen.queryByText("Loading...")).toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "Export preview" }));
    fireEvent.click(await screen.findByRole("button", { name: "미리보기 다운로드" }));
    await waitFor(() => expect(screen.getByRole("dialog", { name: "Life Log export" }).getAttribute("aria-busy")).toBe("true"));

    unmount();
    resolveExport({
      origin: "browser-preview",
      format: "markdown",
      extension: "md",
      mimeType: "text/markdown;charset=utf-8",
      byteLength: 10,
      content: "# fixture\n",
    });
    await Promise.resolve();
    await Promise.resolve();
    expect(document.body.textContent).not.toContain("fixture");
  });
});
