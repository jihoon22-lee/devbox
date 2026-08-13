import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { listActiveRuns, listRuns, runJobNow, stopActiveRun, tailLog } from "../api";
import type { Job, Run } from "../types";
import RunHistory from "./RunHistory";

vi.mock("../api", () => ({
  listRuns: vi.fn(),
  listActiveRuns: vi.fn(),
  runJobNow: vi.fn(),
  stopActiveRun: vi.fn(),
  tailLog: vi.fn(),
}));

const listRunsMock = vi.mocked(listRuns);
const listActiveRunsMock = vi.mocked(listActiveRuns);
const runJobNowMock = vi.mocked(runJobNow);
const stopActiveRunMock = vi.mocked(stopActiveRun);
const tailLogMock = vi.mocked(tailLog);

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
  stopActiveRunMock.mockReset().mockResolvedValue({ ...run, status: "cancelled" });
  tailLogMock.mockReset().mockResolvedValue({
    data: Array.from(new TextEncoder().encode("finished\n")),
    retainedStartOffset: "0",
    nextCursor: "9",
    truncated: false,
  });
});

afterEach(() => cleanup());

describe("RunHistory", () => {
  it("loads bounded history and tails stdout with a decimal cursor", async () => {
    const view = render(<RunHistory jobs={[job]} />);

    await waitFor(() => expect(listRunsMock).toHaveBeenCalledWith(job.id, expect.objectContaining({ limit: 50 })));
    await waitFor(() => expect(tailLogMock).toHaveBeenCalledWith(run.id, "stdout", null));
    expect((await view.findByLabelText("stdout 로그")).textContent).toContain("finished");

    fireEvent.click(view.getByRole("button", { name: "stderr" }));
    await waitFor(() => expect(tailLogMock).toHaveBeenCalledWith(run.id, "stderr", null));
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
    await waitFor(() => expect(stopActiveRunMock).toHaveBeenCalledWith(job.id));
  });
});
