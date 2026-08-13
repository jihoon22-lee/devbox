import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { listRuns, tailLog } from "../api";
import type { Job, Run } from "../types";
import RunHistory from "./RunHistory";

vi.mock("../api", () => ({
  listRuns: vi.fn(),
  tailLog: vi.fn(),
}));

const listRunsMock = vi.mocked(listRuns);
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
  blockedByRunId: null,
  startedAt: 2_000,
  endedAt: 4_500,
  exitCode: 0,
  status: "succeeded",
  ownerInstanceId: null,
  attemptToken: null,
  errorMessage: null,
  targetPid: null,
  targetProcessCreatedAt: null,
  targetPgid: null,
  targetSid: null,
  processMarker: null,
  logDir: "logs/runs/run-1",
  logsDeletedAt: null,
  createdAt: 1_500,
};

beforeEach(() => {
  listRunsMock.mockReset().mockResolvedValue([run]);
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
});
