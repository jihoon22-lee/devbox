import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ImportDialog from "./ImportDialog";
import {
  applyWorkspaceTaskImport,
  cancelWorkspaceTaskImport,
  previewWorkspaceTaskImport,
} from "../api";
import type { WorkspaceTaskPlan } from "../types";

vi.mock("../api", () => ({
  applyImport: vi.fn(async () => 0),
  applyProjectImport: vi.fn(async () => ({ created: 0, skippedConflicts: 0 })),
  applyWorkspaceTaskImport: vi.fn(),
  cancelProjectImport: vi.fn(async () => false),
  cancelWorkspaceTaskImport: vi.fn(async () => true),
  friendlyErrorMessage: vi.fn(() => "요청을 완료하지 못했습니다."),
  importDefinitions: vi.fn(),
  previewProjectImport: vi.fn(),
  previewWorkspaceTaskImport: vi.fn(),
}));

const previewWorkspaceTaskImportMock = vi.mocked(previewWorkspaceTaskImport);
const applyWorkspaceTaskImportMock = vi.mocked(applyWorkspaceTaskImport);
const cancelWorkspaceTaskImportMock = vi.mocked(cancelWorkspaceTaskImport);

const plan: WorkspaceTaskPlan = {
  schemaVersion: 1,
  sourceRoot: "C:\\work\\demo",
  sourcePath: ".vscode/tasks.json",
  projectIdentity: "project-id",
  revision: "revision-1234567890",
  targetKind: "windows",
  targetDistro: null,
  selectedPlatform: "windows",
  items: [
    {
      id: "task-ready",
      sourceIndex: 0,
      label: "Build",
      status: "ready",
      taskKind: "process",
      command: "node",
      args: ["build.js", "--safe"],
      cwd: "C:\\work\\demo",
      environmentKeys: ["BUILD_TOKEN"],
      appliedOverride: "windows",
      dependsOn: [],
      dependsOrder: "parallel",
      hasProblemMatcher: true,
      problemMatcher: {
        regexp: "^(.+):(\\d+): (.+)$",
        file: 1,
        line: 2,
        column: null,
        message: 3,
        severity: null,
      },
      blockedReason: null,
    },
    {
      id: "task-shell",
      sourceIndex: 1,
      label: "Publish shell",
      status: "ready",
      taskKind: "shell",
      command: "npm publish",
      args: [],
      cwd: "C:\\work\\demo",
      environmentKeys: ["PUBLISH_TOKEN"],
      appliedOverride: null,
      dependsOn: ["Build"],
      dependsOrder: "sequence",
      hasProblemMatcher: false,
      problemMatcher: null,
      blockedReason: null,
    },
  ],
};

beforeEach(() => {
  previewWorkspaceTaskImportMock.mockReset().mockResolvedValue(plan);
  applyWorkspaceTaskImportMock.mockReset().mockResolvedValue({
    sourceId: "source-1",
    created: 2,
    updated: 0,
    madeUnavailable: 0,
    skippedConflicts: 1,
  });
  cancelWorkspaceTaskImportMock.mockReset().mockResolvedValue(true);
});

afterEach(() => cleanup());

describe("ImportDialog workspace task mode", () => {
  it("shows a read-only preview, imports ready process and shell tasks, and exposes dependencies", async () => {
    render(<ImportDialog onDone={vi.fn()} onClose={vi.fn()} />);

    fireEvent.click(screen.getByRole("tab", { name: "VS Code tasks" }));
    fireEvent.change(screen.getByLabelText("workspace task 디렉터리"), { target: { value: "C:\\work\\demo" } });
    fireEvent.click(screen.getByRole("button", { name: "tasks.json 미리보기" }));

    await screen.findByText("Build");
    const ready = screen.getByRole("checkbox", { name: "Build 선택" });
    const shell = screen.getByRole("checkbox", { name: "Publish shell 선택" });
    expect(ready).not.toBeDisabled();
    expect(ready).toBeChecked();
    expect(shell).not.toBeDisabled();
    expect(shell).toBeChecked();
    expect(screen.getByText("OS override: windows")).toBeTruthy();
    expect(screen.getByText("dependency: Build · 순차")).toBeTruthy();
    expect(screen.getByText("problem matcher: 지원됨")).toBeTruthy();
    expect(screen.getByText("^(.+):(\\d+): (.+)$")).toBeTruthy();
    expect(screen.getByText("환경 키: BUILD_TOKEN")).toBeTruthy();
    expect(screen.getByText("node")).toBeTruthy();
    expect(screen.getByText("[\"build.js\",\"--safe\"]")).toBeTruthy();
    expect(screen.queryByText("PUBLISH_SECRET_VALUE")).toBeNull();
    expect(screen.getByText(/shell task는 가져온 뒤에도 source 승인과 별도의 셸 실행 승인이 필요합니다/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "선택 task 가져오기 (2)" })).not.toBeDisabled();
  });

  it("keeps selection dependency-safe when a dependency is removed or re-added", async () => {
    render(<ImportDialog onDone={vi.fn()} onClose={vi.fn()} />);

    fireEvent.click(screen.getByRole("tab", { name: "VS Code tasks" }));
    fireEvent.change(screen.getByLabelText("workspace task 디렉터리"), { target: { value: "C:\\work\\demo" } });
    fireEvent.click(screen.getByRole("button", { name: "tasks.json 미리보기" }));
    await screen.findByText("Build");

    const ready = screen.getByRole("checkbox", { name: "Build 선택" });
    const shell = screen.getByRole("checkbox", { name: "Publish shell 선택" });
    fireEvent.click(ready);
    expect(ready).not.toBeChecked();
    expect(shell).not.toBeChecked();
    expect(screen.getByRole("button", { name: "선택 task 가져오기 (0)" })).toBeDisabled();

    fireEvent.click(shell);
    expect(shell).toBeChecked();
    expect(ready).toBeChecked();
    expect(screen.getByRole("button", { name: "선택 task 가져오기 (2)" })).not.toBeDisabled();
  });

  it("applies selected process tasks and explains the explicit trust gate", async () => {
    const onDone = vi.fn();
    render(<ImportDialog onDone={onDone} onClose={vi.fn()} />);

    fireEvent.click(screen.getByRole("tab", { name: "VS Code tasks" }));
    fireEvent.change(screen.getByLabelText("workspace task 디렉터리"), { target: { value: "C:\\work\\demo" } });
    fireEvent.click(screen.getByRole("button", { name: "tasks.json 미리보기" }));
    await screen.findByText("Build");

    fireEvent.click(screen.getByRole("button", { name: "선택 task 가져오기 (2)" }));
    await waitFor(() => expect(applyWorkspaceTaskImportMock).toHaveBeenCalledWith(
      "C:\\work\\demo",
      plan.sourceRoot,
      plan.projectIdentity,
      plan.revision,
      "windows",
      null,
      ["task-ready", "task-shell"],
      expect.any(String),
    ));
    expect(onDone).toHaveBeenCalledWith(2, expect.objectContaining({ created: 2, skippedConflicts: 1 }));
    expect(screen.getByText(/생성 2 · 갱신 0 · 사용 불가 전환 0 · 충돌 건너뜀 1/)).toBeTruthy();
    expect(screen.getByText(/비활성·미신뢰 상태/)).toBeTruthy();
  });
});
