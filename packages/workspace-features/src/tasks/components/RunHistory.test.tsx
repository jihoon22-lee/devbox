import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { listActiveRuns, listRuns, openRunLogInLogLens, runJobNow, searchRunLogs, stopActiveRun, tailLog } from "../api";
import type { Job, LogSearchResponse, Run } from "../types";
import RunHistory, { collectRunLog } from "./RunHistory";

vi.mock("../api", () => ({
  friendlyErrorMessage: vi.fn(() => "요청을 완료하지 못했습니다."),
  listRuns: vi.fn(),
  listActiveRuns: vi.fn(),
  openRunLogInLogLens: vi.fn(),
  runJobNow: vi.fn(),
  searchRunLogs: vi.fn(),
  stopActiveRun: vi.fn(),
  tailLog: vi.fn(),
}));

const listRunsMock = vi.mocked(listRuns);
const listActiveRunsMock = vi.mocked(listActiveRuns);
const runJobNowMock = vi.mocked(runJobNow);
const openRunLogInLogLensMock = vi.mocked(openRunLogInLogLens);
const searchRunLogsMock = vi.mocked(searchRunLogs);
const stopActiveRunMock = vi.mocked(stopActiveRun);
const tailLogMock = vi.mocked(tailLog);
const confirmMock = vi.fn<(message?: string) => boolean>();

const job: Job = {
  id: "job-1",
  kind: "job",
  name: "백업",
  command: "backup",
  cwd: null,
  targetKind: "windows",
  targetDistro: null,
  envConfigured: false,
  cronExpr: "0 * * * *",
  enabled: true,
  overlapPolicy: "skip",
  catchUp: false,
  lastEvaluatedAt: null,
  nextQueueSequence: 0,
  restartPolicy: null,
  autoStart: null,
  healthTcpAddress: null,
  healthTcpPort: null,
  healthStartGraceMs: null,
  createdAt: 1_000,
  updatedAt: 1_000,
};

const run: Run = {
  id: "run-1",
  jobId: job.id,
  scheduledAt: null,
  occurrenceWallKey: null,
  queueSequence: 0,
  startedAt: 2_000,
  endedAt: 4_500,
  exitCode: 0,
  status: "succeeded",
  logsAvailable: true,
  failureCode: null,
  createdAt: 1_500,
};

beforeEach(() => {
  listRunsMock.mockReset().mockResolvedValue([run]);
  listActiveRunsMock.mockReset().mockResolvedValue([]);
  runJobNowMock.mockReset().mockResolvedValue({ ...run, id: "run-now", status: "running", endedAt: null, exitCode: null });
  openRunLogInLogLensMock.mockReset().mockResolvedValue(undefined);
  searchRunLogsMock.mockReset().mockResolvedValue({
    matches: [],
    scannedLines: 1,
    scannedBytes: 9,
    truncated: false,
    sources: [],
  });
  stopActiveRunMock.mockReset().mockResolvedValue({ ...run, status: "cancelled" });
  tailLogMock.mockReset().mockResolvedValue({
    data: Array.from(new TextEncoder().encode("finished\n")),
    retainedStartOffset: "0",
    nextCursor: "9",
    truncated: false,
  });
  confirmMock.mockReset().mockReturnValue(false);
  Object.defineProperty(window, "confirm", {
    configurable: true,
    value: confirmMock,
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("RunHistory", () => {
  it("requires an explicit confirmation before publishing the selected stream to Log Lens", async () => {
    confirmMock.mockReturnValueOnce(false);
    const view = render(<RunHistory jobs={[job]} />);
    await waitFor(() => expect(view.getByLabelText("stdout 로그")).toBeInTheDocument());
    fireEvent.click(view.getByRole("button", { name: "Log Lens" }));
    expect(openRunLogInLogLensMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(view.getByRole("button", { name: "Log Lens" }));
    await waitFor(() => expect(openRunLogInLogLensMock).toHaveBeenCalledWith("run-1", "stdout"));
    expect(confirmMock.mock.calls[1]?.[0]).toContain("로그 원문·경로·명령·환경변수");
  });

  it("locks run and stream context while a Log Lens handoff is pending", async () => {
    const second = { ...run, id: "run-2", status: "failed" as const, exitCode: 1 };
    listRunsMock.mockResolvedValue([run, second]);
    let resolveHandoff: (() => void) | undefined;
    openRunLogInLogLensMock.mockImplementationOnce(
      () => new Promise<void>((resolve) => {
        resolveHandoff = resolve;
      }),
    );
    confirmMock.mockReturnValue(true);
    const view = render(<RunHistory jobs={[job]} />);
    await waitFor(() => expect(view.getByLabelText("stdout 로그")).toBeInTheDocument());

    fireEvent.click(view.getByRole("button", { name: "Log Lens" }));
    await waitFor(() => expect(openRunLogInLogLensMock).toHaveBeenCalledWith("run-1", "stdout"));

    const secondRow = view.container.querySelector<HTMLButtonElement>('[data-run-id="run-2"]');
    const stderrButton = view.getByRole("button", { name: "stderr" });
    expect(secondRow).not.toBeNull();
    expect(secondRow).toBeDisabled();
    expect(stderrButton).toBeDisabled();
    fireEvent.click(secondRow as HTMLButtonElement);
    fireEvent.click(stderrButton);
    expect(view.getByLabelText("stdout 로그")).toBeInTheDocument();

    await act(async () => resolveHandoff?.());
    await waitFor(() => expect(view.getByRole("button", { name: "Log Lens" })).toBeEnabled());
    expect(secondRow).toBeEnabled();
    expect(stderrButton).toBeEnabled();
  });

  it("restores the Log Lens action after IPC failure without exposing the raw error", async () => {
    openRunLogInLogLensMock.mockRejectedValueOnce(new Error("native path /secret/run.log"));
    confirmMock.mockReturnValue(true);
    const view = render(<RunHistory jobs={[job]} />);
    await waitFor(() => expect(view.getByLabelText("stdout 로그")).toBeInTheDocument());

    fireEvent.click(view.getByRole("button", { name: "Log Lens" }));
    const alert = await view.findByRole("alert");
    expect(alert).toHaveTextContent("Log Lens handoff를 시작하지 못했습니다.");
    expect(alert).not.toHaveTextContent("/secret/run.log");
    await waitFor(() => expect(view.getByRole("button", { name: "Log Lens" })).toBeEnabled());
  });

  it("loads bounded history and tails stdout with a decimal cursor", async () => {
    const view = render(<RunHistory jobs={[job]} />);

    await waitFor(() => expect(listRunsMock).toHaveBeenCalledWith(job.id, expect.objectContaining({ limit: 50 })));
    await waitFor(() => expect(tailLogMock).toHaveBeenCalledWith(run.id, "stdout", null));
    expect((await view.findByLabelText("stdout 로그")).textContent).toContain("finished");
    expect(view.getByRole("button", { name: "stdout" }).getAttribute("aria-pressed")).toBe("true");

    fireEvent.click(view.getByRole("button", { name: "stderr" }));
    await waitFor(() => expect(tailLogMock).toHaveBeenCalledWith(run.id, "stderr", null));
    expect(view.getByRole("button", { name: "stderr" }).getAttribute("aria-pressed")).toBe("true");
  });

  it("does not silently drop an invalid non-empty duration filter", async () => {
    const view = render(<RunHistory jobs={[job]} />);
    await waitFor(() => expect(listRunsMock).toHaveBeenCalled());
    listRunsMock.mockClear();

    fireEvent.change(view.getByLabelText("최소 실행 시간(초)"), {
      target: { value: "-1" },
    });

    await waitFor(() => expect(listRunsMock).toHaveBeenCalledWith(
      job.id,
      expect.objectContaining({ minDurationMs: -1_000 }),
    ));
  });

  it("shows the retained-range warning and does not expose numeric cursors", async () => {
    tailLogMock.mockResolvedValueOnce({
      data: Array.from(new TextEncoder().encode("tail")),
      retainedStartOffset: "90071992547409930",
      nextCursor: "90071992547409934",
      truncated: true,
    });
    const view = render(<RunHistory jobs={[job]} />);

    expect(await view.findByText(/이전 로그는 생략했습니다/)).toBeTruthy();
    expect(tailLogMock.mock.calls[0]?.[2]).toBeNull();
  });

  it("exposes manual run and active-stop controls", async () => {
    const active = { ...run, id: "run-now", status: "running" as const, endedAt: null, exitCode: null };
    const cancelled = { ...active, status: "cancelled" as const, endedAt: 5_000 };
    listRunsMock.mockReset().mockResolvedValueOnce([run]).mockResolvedValueOnce([active]).mockResolvedValue([cancelled]);
    runJobNowMock.mockResolvedValue(active);
    listActiveRunsMock.mockReset().mockResolvedValue([active]);
    stopActiveRunMock.mockImplementation(async () => {
      listRunsMock.mockResolvedValueOnce([cancelled]);
      return cancelled;
    });
    const view = render(<RunHistory jobs={[job]} />);
    await waitFor(() => expect(listRunsMock).toHaveBeenCalled());

    fireEvent.click(view.getByRole("button", { name: "지금 실행" }));
    await waitFor(() => expect(runJobNowMock).toHaveBeenCalledWith(job.id));
    fireEvent.click(view.getByRole("button", { name: "활성 실행 중지" }));
    expect(confirmMock).toHaveBeenCalledWith("'백업' 작업의 활성 실행을 중지할까요?");
    expect(stopActiveRunMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(view.getByRole("button", { name: "활성 실행 중지" }));
    await waitFor(() => expect(stopActiveRunMock).toHaveBeenCalledWith(job.id));
  });

  it("selects the right-clicked history row and exposes every history action", async () => {
    const second = { ...run, id: "run-2", status: "failed" as const, exitCode: 1 };
    listRunsMock.mockResolvedValue([run, second]);
    const view = render(<RunHistory jobs={[job]} />);
    const row = await waitFor(() => {
      const element = document.querySelector<HTMLButtonElement>('[data-run-id="run-2"]');
      if (!element) throw new Error("history row was not rendered");
      return element;
    });

    fireEvent.contextMenu(row, { clientX: 20, clientY: 30 });

    expect(row.getAttribute("aria-pressed")).toBe("true");
    expect(view.getByRole("menu", { name: "실행 이력 메뉴" })).toBeTruthy();
    for (const label of ["로그 보기", "재실행", "로그 저장"]) {
      expect(view.getByRole("menuitem", { name: label })).toBeTruthy();
    }
  });

  it("opens the history menu from Shift+F10, reruns the exact row, and restores focus", async () => {
    const view = render(<RunHistory jobs={[job]} />);
    const row = await waitFor(() => {
      const element = document.querySelector<HTMLButtonElement>('[data-run-id="run-1"]');
      if (!element) throw new Error("history row was not rendered");
      return element;
    });
    row.focus();

    fireEvent.keyDown(row, { key: "F10", code: "F10", shiftKey: true });
    fireEvent.click(view.getByRole("menuitem", { name: "재실행" }));

    await waitFor(() => expect(runJobNowMock).toHaveBeenCalledWith(job.id));
    await waitFor(() => expect(document.activeElement).toBe(row));
  });

  it("saves the selected stream with an opaque bounded filename", async () => {
    const createObjectUrl = vi.fn(() => "blob:run-log");
    const revokeObjectUrl = vi.fn();
    Object.defineProperty(URL, "createObjectURL", { configurable: true, value: createObjectUrl });
    Object.defineProperty(URL, "revokeObjectURL", { configurable: true, value: revokeObjectUrl });
    const downloads: string[] = [];
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (
      this: HTMLAnchorElement,
    ) {
      downloads.push(this.download);
    });
    const view = render(<RunHistory jobs={[job]} />);
    await view.findByLabelText("stdout 로그");
    tailLogMock.mockReset()
      .mockResolvedValueOnce({
        data: Array.from(new TextEncoder().encode("saved\n")),
        retainedStartOffset: "0",
        nextCursor: "6",
        truncated: false,
      })
      .mockResolvedValueOnce({
        data: [],
        retainedStartOffset: "0",
        nextCursor: "6",
        truncated: false,
      });
    const row = document.querySelector<HTMLButtonElement>('[data-run-id="run-1"]');
    if (!row) throw new Error("history row was not rendered");

    fireEvent.contextMenu(row);
    fireEvent.click(view.getByRole("menuitem", { name: "로그 저장" }));

    await waitFor(() => expect(createObjectUrl).toHaveBeenCalledTimes(1));
    expect(downloads).toEqual(["run-run-1-stdout.log"]);
    expect(revokeObjectUrl).toHaveBeenCalledWith("blob:run-log");
    expect(tailLogMock).toHaveBeenCalledWith(run.id, "stdout", null, 256 * 1024);
    click.mockRestore();
  });

  it("collects multiple decimal-cursor chunks without converting offsets to numbers", async () => {
    tailLogMock.mockReset()
      .mockResolvedValueOnce({
        data: [97, 98],
        retainedStartOffset: "90071992547409930",
        nextCursor: "90071992547409932",
        truncated: false,
      })
      .mockResolvedValueOnce({
        data: [99, 100],
        retainedStartOffset: "90071992547409930",
        nextCursor: "90071992547409934",
        truncated: false,
      })
      .mockResolvedValueOnce({
        data: [],
        retainedStartOffset: "90071992547409930",
        nextCursor: "90071992547409934",
        truncated: false,
      });

    const collected = await collectRunLog(run.id, "stdout", tailLogMock);

    expect(new TextDecoder().decode(collected.bytes)).toBe("abcd");
    expect(collected.truncated).toBe(false);
    expect(tailLogMock.mock.calls.map((call) => call[2])).toEqual([
      null,
      "90071992547409932",
      "90071992547409934",
    ]);
  });

  it("stops a malformed non-advancing cursor instead of looping", async () => {
    tailLogMock.mockReset()
      .mockResolvedValueOnce({
        data: [97],
        retainedStartOffset: "0",
        nextCursor: "1",
        truncated: false,
      })
      .mockResolvedValue({
        data: [98],
        retainedStartOffset: "0",
        nextCursor: "1",
        truncated: false,
      });

    const collected = await collectRunLog(run.id, "stdout", tailLogMock);

    expect(new TextDecoder().decode(collected.bytes)).toBe("ab");
    expect(collected.truncated).toBe(true);
    expect(tailLogMock).toHaveBeenCalledTimes(2);
  });

  it("searches literal logs with source/level filters and navigates by stream and line", async () => {
    const response: LogSearchResponse = {
      matches: [
        {
          sourceId: "run-manager:run-1:stderr",
          stream: "stderr",
          lineNumber: 1,
          level: "error",
          timestampMillis: null,
        },
      ],
      scannedLines: 2,
      scannedBytes: 24,
      truncated: false,
      sources: [{ kind: "log-source/v1", sourceId: "run-manager:run-1:stderr", runId: "run-1", stream: "stderr" }],
    };
    searchRunLogsMock.mockResolvedValueOnce(response);
    const view = render(<RunHistory jobs={[job]} />);
    const query = await view.findByRole("searchbox", { name: "로그 검색어" });
    fireEvent.change(query, { target: { value: "failure" } });
    fireEvent.change(view.getByRole("combobox", { name: "로그 검색 소스" }), { target: { value: "stderr" } });
    fireEvent.change(view.getByRole("combobox", { name: "로그 검색 레벨" }), { target: { value: "error" } });
    fireEvent.submit(view.getByRole("form", { name: "로그 검색" }));

    await waitFor(() => expect(searchRunLogsMock).toHaveBeenCalledWith("run-1", {
      query: "failure",
      mode: "literal",
      source: "stderr",
      level: "error",
      startAt: null,
      endAt: null,
    }));
    expect(await view.findByText(/1 \/ 1개 결과/)).toBeTruthy();
    fireEvent.click(view.getByRole("button", { name: "다음 검색 결과" }));
    expect(view.getByRole("button", { name: "stderr" }).className).toContain("active");
  });

  it("does not submit a composing Enter and suppresses raw regex errors", async () => {
    searchRunLogsMock.mockRejectedValueOnce(new Error("secret /path/raw"));
    const view = render(<RunHistory jobs={[job]} />);
    const query = await view.findByRole("searchbox", { name: "로그 검색어" });
    fireEvent.change(query, { target: { value: "[" } });
    fireEvent.change(view.getByRole("combobox", { name: "로그 검색 방식" }), { target: { value: "regex" } });
    fireEvent.keyDown(query, { key: "Enter", keyCode: 229 });
    expect(searchRunLogsMock).not.toHaveBeenCalled();
    fireEvent.submit(view.getByRole("form", { name: "로그 검색" }));

    const alert = await view.findByRole("alert");
    expect(alert.textContent).toContain("로그 검색을 완료하지 못했습니다.");
    expect(alert.textContent).not.toContain("secret");
    expect(alert.textContent).not.toContain("/path/raw");
  });

  it("submits from keyboard and clears metadata without retaining a log copy", async () => {
    searchRunLogsMock.mockResolvedValueOnce({
      matches: [{
        sourceId: "run-manager:run-1:stdout",
        stream: "stdout",
        lineNumber: 1,
        level: null,
        timestampMillis: null,
      }],
      scannedLines: 1,
      scannedBytes: 4,
      truncated: false,
      sources: [{ kind: "log-source/v1", sourceId: "run-manager:run-1:stdout", runId: "run-1", stream: "stdout" }],
    });
    const view = render(<RunHistory jobs={[job]} />);
    const query = await view.findByRole("searchbox", { name: "로그 검색어" });
    fireEvent.change(query, { target: { value: "hit" } });
    fireEvent.keyDown(query, { key: "Enter", code: "Enter", keyCode: 13 });
    await waitFor(() => expect(searchRunLogsMock).toHaveBeenCalledTimes(1));
    expect(await view.findByText(/1 \/ 1개 결과/)).toBeTruthy();

    fireEvent.click(view.getByRole("button", { name: "지우기" }));
    expect((query as HTMLInputElement).value).toBe("");
    expect(view.queryByText("1 / 1개 결과")).toBeNull();
  });

  it("guards duplicate searches while busy and ignores a stale result after unmount", async () => {
    let resolve: (value: LogSearchResponse) => void = () => undefined;
    searchRunLogsMock.mockReturnValueOnce(new Promise<LogSearchResponse>((done) => {
      resolve = (value) => done(value);
    }));
    const view = render(<RunHistory jobs={[job]} />);
    const query = await view.findByRole("searchbox", { name: "로그 검색어" });
    fireEvent.change(query, { target: { value: "hit" } });
    const form = view.getByRole("form", { name: "로그 검색" });
    fireEvent.submit(form);
    fireEvent.submit(form);
    expect(searchRunLogsMock).toHaveBeenCalledTimes(1);
    view.unmount();
    resolve({ matches: [], scannedLines: 1, scannedBytes: 3, truncated: false, sources: [] });
    await Promise.resolve();
  });
});
