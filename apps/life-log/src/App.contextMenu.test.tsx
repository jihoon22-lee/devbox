import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

const mocks = vi.hoisted(() => ({
  writeText: vi.fn<(value: string) => Promise<void>>(),
}));

vi.mock("./api", () => ({
  autostartStatus: vi.fn().mockResolvedValue({ supported: true, enabled: false, command: null }),
  getAppStats: vi.fn().mockResolvedValue([]),
  getDay: vi.fn().mockImplementation(async (date: string) => ({
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
  startTracking: vi.fn().mockResolvedValue(true),
  stopTracking: vi.fn().mockResolvedValue(undefined),
}));

beforeEach(() => {
  mocks.writeText.mockReset().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: mocks.writeText },
  });
});

afterEach(() => cleanup());

async function renderLoadedApp() {
  render(<App />);
  await screen.findByText("1h 0m");
  return screen.getByLabelText(/\d{4}-\d{2}-\d{2} 선택된 날짜/u) as HTMLInputElement;
}

describe("Life Log date context menu", () => {
  it("선택 날짜 입력에 exact menu를 열고 날짜만 복사한 뒤 focus를 복원한다", async () => {
    const dateInput = await renderLoadedApp();
    const selectedDate = dateInput.value;
    dateInput.focus();

    fireEvent.contextMenu(dateInput, { clientX: 20, clientY: 24 });

    expect(screen.getByRole("menuitem", { name: "날짜 복사" }).getAttribute("aria-disabled")).toBeNull();
    expect(screen.getByRole("menuitem", { name: "Markdown 내보내기" }).getAttribute("aria-disabled")).toBe("true");
    expect(screen.getByRole("menuitem", { name: "JSON 내보내기" }).getAttribute("aria-disabled")).toBe("true");

    fireEvent.click(screen.getByRole("menuitem", { name: "날짜 복사" }));

    await waitFor(() => expect(mocks.writeText).toHaveBeenCalledWith(selectedDate));
    expect(await screen.findByText(`${selectedDate} 날짜를 복사했습니다.`)).toBeTruthy();
    await waitFor(() => expect(document.activeElement).toBe(dateInput));
  });

  it("키보드로 연 exact chart date를 먼저 선택하고 그 날짜를 복사한다", async () => {
    const dateInput = await renderLoadedApp();
    fireEvent.click(screen.getByRole("button", { name: "Week" }));
    const target = await screen.findByRole("button", { name: "2024-01-02 날짜" });
    target.focus();

    fireEvent.keyDown(target, { key: "F10", code: "F10", shiftKey: true });

    await waitFor(() => expect(dateInput.value).toBe("2024-01-02"));
    expect(target.getAttribute("aria-current")).toBe("date");
    fireEvent.click(screen.getByRole("menuitem", { name: "날짜 복사" }));
    await waitFor(() => expect(mocks.writeText).toHaveBeenCalledWith("2024-01-02"));
    await waitFor(() => expect(document.activeElement).toBe(target));
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
});
