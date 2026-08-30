import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { previewCron } from "../api";
import JobEditor from "./JobEditor";
import type { CronPreviewItem, Job, JobInput, WorkspaceTaskState } from "../types";

vi.mock("../api", () => ({
  friendlyErrorMessage: vi.fn((cause: unknown) => cause instanceof Error ? cause.message : String(cause)),
  previewCron: vi.fn(),
}));

const previewCronMock = vi.mocked(previewCron);

function previewItems(): CronPreviewItem[] {
  return Array.from({ length: 5 }, (_, index) => ({
    timestampMillis: 1_000 + index * 60_000,
    datetime: `2026-08-13T0${index + 1}:00:00+09:00`,
    wallTime: `2026-08-13 0${index + 1}:00:00`,
    wallKey: `2026-08-13T0${index + 1}:00:00`,
  }));
}

beforeEach(() => {
  previewCronMock.mockReset().mockResolvedValue(previewItems());
});

afterEach(() => cleanup());

describe("JobEditor", () => {
  it("renders all five next-run preview rows from the shared preview API", async () => {
    const { getByRole, getAllByRole } = render(<JobEditor job={null} onSave={vi.fn()} onCancel={vi.fn()} />);

    await waitFor(() => expect(previewCronMock).toHaveBeenCalled());
    await waitFor(() => expect(getAllByRole("listitem")).toHaveLength(5));
    expect(getByRole("heading", { name: "다음 실행" })).toBeTruthy();
    expect(getAllByRole("listitem")[0].textContent).toContain("2026-08-13 01:00:00");
  });

  it("shows field errors and does not save an invalid cron expression", async () => {
    const onSave = vi.fn<(input: JobInput) => Promise<void>>().mockResolvedValue(undefined);
    const { getByRole, getByLabelText } = render(<JobEditor job={null} onSave={onSave} onCancel={vi.fn()} />);

    fireEvent.change(getByLabelText("작업 이름"), { target: { value: "backup" } });
    fireEvent.change(getByLabelText("실행 명령"), { target: { value: "echo backup" } });
    fireEvent.change(getByLabelText("cron 표현식"), { target: { value: "@reboot" } });
    fireEvent.click(getByRole("button", { name: "작업 저장" }));

    expect(await getByRole("alert")).toBeTruthy();
    expect(onSave).not.toHaveBeenCalled();
  });

  it("requires a WSL distro before saving a WSL-targeted job", async () => {
    const onSave = vi.fn<(input: JobInput) => Promise<void>>().mockResolvedValue(undefined);
    const { getByRole, getByLabelText } = render(<JobEditor job={null} onSave={onSave} onCancel={vi.fn()} />);

    fireEvent.change(getByLabelText("작업 이름"), { target: { value: "backup" } });
    fireEvent.change(getByLabelText("실행 명령"), { target: { value: "echo backup" } });
    fireEvent.click(getByRole("radio", { name: /WSL/ }));
    fireEvent.click(getByRole("button", { name: "작업 저장" }));

    expect(await getByTextSafe(getByRole, "WSL 배포판을 입력하세요.")).toBeTruthy();
    expect(onSave).not.toHaveBeenCalled();
  });

  it("keeps, replaces, or clears a masked environment through explicit actions", async () => {
    const job: Job = {
      id: "job-1",
      kind: "job",
      name: "backup",
      command: "echo backup",
      cwd: null,
      targetKind: "windows",
      targetDistro: null,
      envConfigured: true,
      cronExpr: "0 * * * *",
      enabled: false,
      overlapPolicy: "skip",
      catchUp: false,
      lastEvaluatedAt: null,
      nextQueueSequence: 0,
      restartPolicy: null,
      autoStart: null,
      healthTcpAddress: null,
      healthTcpPort: null,
      healthStartGraceMs: null,
      createdAt: 1,
      updatedAt: 1,
    };
    const onSave = vi.fn<(input: JobInput) => Promise<void>>().mockResolvedValue(undefined);
    const { getByRole, getByLabelText, unmount } = render(
      <JobEditor job={job} onSave={onSave} onCancel={vi.fn()} />,
    );

    fireEvent.click(getByRole("button", { name: "기존 환경변수 교체" }));
    fireEvent.change(getByLabelText("환경변수 이름"), { target: { value: "TOKEN" } });
    fireEvent.change(getByLabelText("환경변수 값"), { target: { value: "rotated" } });
    fireEvent.click(getByRole("button", { name: "작업 저장" }));
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0].environment).toEqual({
      action: "replace",
      values: { TOKEN: "rotated" },
    });

    onSave.mockClear();
    unmount();
    const { getByRole: getByRoleAfterClear } = render(
      <JobEditor job={job} onSave={onSave} onCancel={vi.fn()} />,
    );
    fireEvent.click(getByRoleAfterClear("button", { name: "기존 환경변수 삭제" }));
    fireEvent.click(getByRoleAfterClear("button", { name: "작업 저장" }));
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0].environment).toEqual({ action: "clear" });
  });

  it("locks source-managed fields and limits environment values to declared keys", async () => {
    const job: Job = {
      id: "job-managed",
      kind: "job",
      name: "Build",
      command: "node",
      cwd: "C:\\work\\demo",
      targetKind: "windows",
      targetDistro: null,
      envConfigured: false,
      cronExpr: "0 * * * *",
      enabled: false,
      overlapPolicy: "skip",
      catchUp: false,
      lastEvaluatedAt: null,
      nextQueueSequence: 0,
      restartPolicy: null,
      autoStart: null,
      healthTcpAddress: null,
      healthTcpPort: null,
      healthStartGraceMs: null,
      createdAt: 1,
      updatedAt: 1,
    };
    const workspaceTask: WorkspaceTaskState = {
      jobId: job.id,
      sourceId: "source-1",
      label: "Build",
      taskKind: "process",
      sourceRoot: "C:\\work\\demo",
      revision: "revision-1",
      targetKind: "windows",
      targetDistro: null,
      environmentKeys: ["BUILD_TOKEN"],
      appliedOverride: "windows",
      dependsOn: [],
      dependsOrder: "parallel",
      hasProblemMatcher: false,
      trusted: false,
      shellTrusted: false,
      available: true,
    };
    const onSave = vi.fn<(input: JobInput) => Promise<void>>().mockResolvedValue(undefined);
    const { getByRole, getByLabelText } = render(
      <JobEditor job={job} workspaceTask={workspaceTask} onSave={onSave} onCancel={vi.fn()} />,
    );

    expect(getByLabelText("작업 이름")).toBeDisabled();
    expect(getByLabelText("실행 명령")).toBeDisabled();
    expect(getByLabelText("작업 디렉터리")).toBeDisabled();
    expect(getByRole("checkbox", { name: /활성화/ })).toBeDisabled();

    fireEvent.click(getByRole("button", { name: "변수 추가" }));
    const envKey = getByLabelText("환경변수 이름");
    expect(envKey.tagName).toBe("SELECT");
    expect(Array.from((envKey as HTMLSelectElement).options).map((option) => option.value)).toEqual(["", "BUILD_TOKEN"]);
  });
});

function getByTextSafe(getByRole: ReturnType<typeof render>["getByRole"], text: string): HTMLElement {
  // Keep the assertion helper independent of a global `screen`, which makes
  // this test easier to embed in the app's existing RTL setup.
  const alert = getByRole("alert");
  if (alert.textContent?.includes(text)) return alert;
  throw new Error(`expected alert to contain ${text}`);
}
