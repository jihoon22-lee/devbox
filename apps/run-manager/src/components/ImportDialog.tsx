// 정의/task import 다이얼로그.
// 기존 export JSON과 로컬 package.json/Cargo.toml 모두
// preview -> 항목 선택 -> 명시적 적용 순서를 따른다.

import { useEffect, useRef, useState } from "react";
import ChangeSetPreview, { type ChangeSetItem } from "@devbox/diff-view";
import {
  applyImport,
  applyProjectImport,
  applyWorkspaceTaskImport,
  cancelProjectImport,
  cancelWorkspaceTaskImport,
  friendlyErrorMessage,
  importDefinitions,
  previewProjectImport,
  previewWorkspaceTaskImport,
  type ImportPlan,
  type ProjectImportPlan,
} from "../api";
import type {
  TargetKind,
  WorkspaceTaskApplyResult,
  WorkspaceTaskItem,
  WorkspaceTaskPlan,
} from "../types";

interface Props {
  onDone: (created: number, workspaceResult?: WorkspaceTaskApplyResult) => void;
  onClose: () => void;
}

type Preview =
  | { kind: "definitions"; plan: ImportPlan; json: string }
  | { kind: "project"; plan: ProjectImportPlan; path: string }
  | { kind: "workspace"; plan: WorkspaceTaskPlan; path: string };

const PREVIEW_TIMEOUT_MS = 6_000;

function operationId(prefix: "preview" | "apply"): string {
  const random = globalThis.crypto?.randomUUID?.();
  return `${prefix}-${random ?? `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`}`;
}

function withTimeout<T>(request: Promise<T>, message: string): Promise<T> {
  let timer: number | undefined;
  const timeout = new Promise<T>((_resolve, reject) => {
    timer = window.setTimeout(() => reject(new Error(message)), PREVIEW_TIMEOUT_MS);
  });
  return Promise.race([request, timeout]).finally(() => {
    if (timer !== undefined) window.clearTimeout(timer);
  });
}

function errorMessage(cause: unknown): string {
  const value = cause instanceof Error ? cause.message : String(cause);
  if (value === "import-preview-stale" || value === "project-import-stale") {
    return "원본이 미리보기 이후 변경되었습니다. 다시 미리보기 해주세요.";
  }
  if (value === "project-import-timeout") {
    return "프로젝트 가져오기가 제한 시간 안에 끝나지 않았습니다. 파일을 확인한 뒤 다시 시도하세요.";
  }
  if (value === "project-import-cancelled") {
    return "프로젝트 가져오기를 취소했습니다.";
  }
  if (value.startsWith("project-import-")) {
    return "프로젝트 가져오기를 읽지 못했습니다. 로컬 디렉터리와 파일 크기를 확인하세요.";
  }
  if (value === "workspace-task-import-timeout") {
    return "VS Code task 미리보기가 제한 시간 안에 끝나지 않았습니다. 파일을 확인한 뒤 다시 시도하세요.";
  }
  if (value === "workspace-task-import-cancelled") {
    return "VS Code task 미리보기를 취소했습니다.";
  }
  if (value.startsWith("workspace-task-")) {
    return friendlyErrorMessage(value);
  }
  return "가져오기를 완료하지 못했습니다.";
}

function isSelectableWorkspaceItem(item: WorkspaceTaskItem): boolean {
  return item.status === "ready"
    && (item.taskKind === "process" || item.taskKind === "shell")
    && item.blockedReason == null
    && item.command != null
    && item.cwd != null;
}

/**
 * VS Code dependencies are expressed by label while the native apply
 * contract receives stable item ids. Keep the conversion in the UI so the
 * preview can make the closure visible and never submit a stranded child.
 */
function withWorkspaceDependencyClosure(
  plan: WorkspaceTaskPlan,
  selectedIds: Iterable<string>,
): Set<string> {
  const byId = new Map(plan.items.map((item) => [item.id, item]));
  const byLabel = new Map(plan.items.map((item) => [item.label, item]));
  const next = new Set<string>();
  const visiting = new Set<string>();
  const include = (id: string) => {
    if (next.has(id) || visiting.has(id)) return;
    const item = byId.get(id);
    if (!item || !isSelectableWorkspaceItem(item)) return;
    visiting.add(id);
    next.add(id);
    for (const dependency of item.dependsOn) {
      const dependencyItem = byLabel.get(dependency);
      if (dependencyItem) include(dependencyItem.id);
    }
    visiting.delete(id);
  };
  for (const id of selectedIds) include(id);
  return next;
}

/** Remove selected descendants when their predecessor is deselected. */
function toggleWorkspaceTaskSelection(
  plan: WorkspaceTaskPlan,
  selectedIds: Set<string>,
  id: string,
): Set<string> {
  const item = plan.items.find((candidate) => candidate.id === id);
  if (!item || !isSelectableWorkspaceItem(item)) return new Set(selectedIds);
  if (!selectedIds.has(id)) {
    return withWorkspaceDependencyClosure(plan, [...selectedIds, id]);
  }

  const byId = new Map(plan.items.map((candidate) => [candidate.id, candidate]));
  const next = new Set(selectedIds);
  const removedLabels = new Set<string>([item.label]);
  next.delete(id);
  let changed = true;
  while (changed) {
    changed = false;
    for (const selectedId of [...next]) {
      const selected = byId.get(selectedId);
      if (!selected || !selected.dependsOn.some((dependency) => removedLabels.has(dependency))) continue;
      next.delete(selectedId);
      removedLabels.add(selected.label);
      changed = true;
    }
  }
  return next;
}

function workspaceStatusLabel(item: WorkspaceTaskItem): string {
  if (item.status === "conflict") return "충돌";
  if (item.status === "blocked") return "차단됨";
  return isSelectableWorkspaceItem(item) ? "가져올 수 있음" : "검토 필요";
}

function workspaceReasonLabel(reason: string | null | undefined): string {
  if (!reason) return "";
  const labels: Record<string, string> = {
    "invalid-task": "task 항목이 객체 형식이 아닙니다.",
    "invalid-label": "task label이 없거나 올바르지 않습니다.",
    "duplicate-label": "같은 source에 중복된 task label이 있습니다.",
    "shell-requires-separate-confirmation": "shell task는 별도 위험 확인이 필요해 현재 가져올 수 없습니다.",
    "unsupported-task-type": "현재 process·shell task만 가져올 수 있습니다.",
    "missing-task-type": "task type이 없어 실행 방식을 결정할 수 없습니다.",
    "dependency-graph-too-large": "task dependency 그래프가 허용된 크기를 초과했습니다.",
    "invalid-dependency": "task dependency 선언이 올바르지 않습니다.",
    "invalid-dependency-order": "task dependency 실행 순서가 parallel 또는 sequence가 아닙니다.",
    "dependency-order-without-dependency": "dependency 없이 실행 순서를 지정할 수 없습니다.",
    "dependency-cycle": "task dependency에 순환 참조가 있어 가져올 수 없습니다.",
    "dependency-unavailable": "선행 dependency를 사용할 수 없어 가져올 수 없습니다.",
    "dependencies-require-orchestration": "task 의존 관계는 orchestration 지원 후 가져올 수 있습니다.",
    "background-task-unsupported": "background task는 종료 판정 지원 후 가져올 수 있습니다.",
    "run-options-unsupported": "runOptions가 있는 task는 현재 가져올 수 없습니다.",
    "invalid-command": "실행 command가 없거나 올바르지 않습니다.",
    "invalid-arguments": "args가 문자열 배열 형식이 아닙니다.",
    "arguments-too-large": "argv가 허용된 크기 제한을 넘었습니다.",
    "quoted-argument-unsupported": "VS Code의 quoted argument 객체 형식은 현재 지원하지 않습니다.",
    "invalid-os-override": "선택된 OS override 형식이 올바르지 않습니다.",
    "invalid-options": "task options 형식이 올바르지 않습니다.",
    "custom-shell-unsupported": "사용자 지정 shell options는 현재 지원하지 않습니다.",
    "unsupported-options-field": "지원하지 않는 task options 필드가 있습니다.",
    "unsupported-os-override-field": "지원하지 않는 OS override 필드가 있습니다.",
    "invalid-cwd": "작업 디렉터리가 올바르지 않거나 프로젝트 밖을 가리킵니다.",
    "cwd-outside-project": "작업 디렉터리가 프로젝트 경계 밖을 가리킵니다.",
    "invalid-environment": "환경변수 선언 형식이 올바르지 않습니다.",
    "environment-too-large": "환경변수 키 목록이 허용된 제한을 넘었습니다.",
    "invalid-variable-value": "변수 치환 대상이 문자열 형식이 아닙니다.",
    "invalid-workspace-folder": "workspace 경로를 안전하게 해석할 수 없습니다.",
    "unsupported-variable": "지원하지 않는 변수 참조가 있어 차단되었습니다.",
    "variable-result-too-large": "변수 치환 결과가 허용된 크기 제한을 넘었습니다.",
    "named-problem-matcher-unsupported": "이름으로 지정한 problem matcher는 지원하지 않습니다.",
    "background-problem-matcher-unsupported": "background problem matcher는 지원하지 않습니다.",
    "unsupported-problem-matcher-field": "지원하지 않는 problem matcher 필드가 있습니다.",
    "unsupported-problem-matcher-location": "problem matcher의 파일 위치 설정을 지원하지 않습니다.",
    "invalid-problem-matcher": "problem matcher 형식이 올바르지 않습니다.",
  };
  return labels[reason] ?? reason;
}

function workspaceSourcePath(plan: WorkspaceTaskPlan): string {
  const separator = plan.sourceRoot.includes("\\") ? "\\" : "/";
  return `${plan.sourceRoot.replace(/[\\/]+$/, "")}${separator}${plan.sourcePath.replace(/^[\\/]+/, "")}`;
}

interface WorkspaceTaskPreviewProps {
  plan: WorkspaceTaskPlan;
  selectedIds: Set<string>;
  busy: boolean;
  cancelRequested: boolean;
  result: WorkspaceTaskApplyResult | null;
  onToggle: (id: string) => void;
  onApprove: () => void;
  onDiscard: () => void;
  onCancel: () => void;
}

function WorkspaceTaskPreview({
  plan,
  selectedIds,
  busy,
  cancelRequested,
  result,
  onToggle,
  onApprove,
  onDiscard,
  onCancel,
}: WorkspaceTaskPreviewProps) {
  return (
    <section className="workspace-task-preview" aria-label="VS Code workspace task 가져오기 계획">
      <div className="import-source-summary">
        <strong>읽은 workspace task source</strong>
        <code>{workspaceSourcePath(plan)}</code>
        <span>대상: {plan.targetKind === "wsl" ? `WSL · ${plan.targetDistro ?? "배포판 없음"}` : "Windows"} · 적용 플랫폼: {plan.selectedPlatform}</span>
        <p>revision {plan.revision.slice(0, 12)} · 미리보기는 읽기 전용·오프라인이며 원본 변경 시 적용이 거부됩니다.</p>
      </div>
      <div className="workspace-task-notice" role="note">
        ready process·shell task를 가져올 수 있습니다. 선택한 task의 dependency는 자동으로 함께 선택되며, 선행 task를 해제하면 그에 의존하는 선택 항목도 함께 해제됩니다. shell task는 가져온 뒤에도 source 승인과 별도의 셸 실행 승인이 필요합니다. 지원하지 않는 변수·잘못된 cwd는 차단되며, 환경변수 값은 읽거나 표시하지 않고 키 이름만 보여 줍니다.
      </div>
      <div className="workspace-task-list" role="list" aria-label="workspace task 목록">
        {plan.items.map((item) => {
          const selectable = isSelectableWorkspaceItem(item);
          const checked = selectedIds.has(item.id);
          const reason = workspaceReasonLabel(item.blockedReason)
            || (item.status === "conflict"
              ? "같은 이름·작업 디렉터리의 정의가 이미 있어 충돌했습니다."
              : !selectable && item.taskKind === "shell"
                ? "shell task는 별도 위험 확인이 필요해 현재 가져올 수 없습니다."
                : "현재 가져올 수 없는 task입니다.");
          return (
            <article className={`workspace-task-item ${selectable ? "ready" : "blocked"}`} key={item.id} role="listitem">
              <div className="workspace-task-item-head">
                <label className="workspace-task-select">
                  <input
                    type="checkbox"
                    aria-label={`${item.label} 선택`}
                    checked={checked}
                    disabled={!selectable || busy}
                    onChange={() => onToggle(item.id)}
                  />
                  <strong>{item.label}</strong>
                </label>
                <span className={`workspace-task-status ${selectable ? "ready" : "blocked"}`}>{workspaceStatusLabel(item)}</span>
              </div>
              <div className="workspace-task-meta">
                <span>유형: {item.taskKind ?? "알 수 없음"}</span>
                <span>OS override: {item.appliedOverride ?? "없음"}</span>
                <span>dependency: {item.dependsOn.length > 0 ? `${item.dependsOn.join(", ")} · ${item.dependsOrder === "sequence" ? "순차" : "병렬"}` : "없음"}</span>
                <span>problem matcher: {item.problemMatcher ? "지원됨" : item.hasProblemMatcher ? "지원되지 않음" : "없음"}</span>
                <span>환경 키: {item.environmentKeys.length > 0 ? item.environmentKeys.join(", ") : "없음"}</span>
              </div>
              <dl className="workspace-task-details">
                <div><dt>command</dt><dd><code>{item.command ?? "—"}</code></dd></div>
                <div><dt>argv</dt><dd><code>{JSON.stringify(item.args)}</code></dd></div>
                <div><dt>cwd</dt><dd><code>{item.cwd ?? "—"}</code></dd></div>
                {item.problemMatcher ? (
                  <div><dt>matcher</dt><dd><code>{item.problemMatcher.regexp}</code> · file #{item.problemMatcher.file}, line #{item.problemMatcher.line}, message #{item.problemMatcher.message}</dd></div>
                ) : null}
              </dl>
              {!selectable ? <p className="workspace-task-reason" role="note">차단 사유: {reason}</p> : null}
            </article>
          );
        })}
        {plan.items.length === 0 ? <div className="empty">가져올 task가 없습니다.</div> : null}
      </div>
      {result ? (
        <div className="workspace-task-result" role="status">
          <strong>workspace task 가져오기 완료</strong>
          <span>생성 {result.created} · 갱신 {result.updated} · 사용 불가 전환 {result.madeUnavailable} · 충돌 건너뜀 {result.skippedConflicts}</span>
          <p>가져온 작업은 비활성·미신뢰 상태입니다. 이 source revision을 별도로 승인한 뒤 Jobs 화면에서 활성화해야 하며, 승인은 실행 자체를 시작하지 않습니다. shell task는 source 승인 뒤에도 셸 실행을 별도로 승인해야 합니다.</p>
        </div>
      ) : null}
      <div className="changeset-actions workspace-task-actions">
        <button type="button" className="btn" disabled={busy || selectedIds.size === 0 || Boolean(result)} onClick={onApprove}>
          선택 task 가져오기 ({selectedIds.size})
        </button>
        <button type="button" className="btn" disabled={busy} onClick={onDiscard}>다시 선택</button>
        {busy ? (
          <button type="button" className="btn" disabled={cancelRequested} onClick={onCancel}>{cancelRequested ? "취소 중…" : "가져오기 취소"}</button>
        ) : null}
      </div>
    </section>
  );
}

export default function ImportDialog({ onDone, onClose }: Props) {
  const [mode, setMode] = useState<"definitions" | "project" | "workspace">("definitions");
  const [json, setJson] = useState("");
  const [projectPath, setProjectPath] = useState("");
  const [workspacePath, setWorkspacePath] = useState("");
  const [workspaceTargetKind, setWorkspaceTargetKind] = useState<TargetKind>("windows");
  const [workspaceTargetDistro, setWorkspaceTargetDistro] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [workspaceSelectedIds, setWorkspaceSelectedIds] = useState<Set<string>>(new Set());
  const [workspaceResult, setWorkspaceResult] = useState<WorkspaceTaskApplyResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [cancelRequested, setCancelRequested] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const previewGeneration = useRef(0);
  const activeOperationId = useRef<string | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const busyRef = useRef(false);
  const mountedRef = useRef(true);
  const cancelPendingRef = useRef<() => Promise<void>>(() => Promise.resolve());
  const cancelOperationRef = useRef<(operationId: string) => Promise<boolean>>(cancelProjectImport);
  const onCloseRef = useRef(onClose);
  busyRef.current = busy;
  onCloseRef.current = onClose;

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    const focusableSelector =
      "button:not([disabled]), input:not([disabled]), textarea:not([disabled]), " +
      "[href], [tabindex]:not([tabindex='-1'])";
    const focusable = () =>
      Array.from(dialog.querySelectorAll<HTMLElement>(focusableSelector));
    focusable()[0]?.focus();

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        if (busyRef.current) {
          // Definition JSON parsing/saving is deliberately bounded but has no
          // cancellable native operation. Do not make Escape appear to cancel
          // a batch that may already have committed.
          if (activeOperationId.current) void cancelPendingRef.current();
        }
        else onCloseRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      const controls = focusable();
      if (controls.length === 0) return;
      const first = controls[0];
      const last = controls[controls.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    dialog.addEventListener("keydown", onKeyDown);
    return () => dialog.removeEventListener("keydown", onKeyDown);
  }, []);

  const previewDefinitions = async () => {
    if (!mountedRef.current) return;
    const generation = ++previewGeneration.current;
    activeOperationId.current = null;
    setBusy(true);
    setCancelRequested(false);
    setError(null);
    setWorkspaceResult(null);
    try {
      const plan = await importDefinitions(json);
      if (mountedRef.current && generation === previewGeneration.current) {
        setPreview({ kind: "definitions", plan, json });
      }
    } catch (cause) {
      if (mountedRef.current && generation === previewGeneration.current) {
        setError(errorMessage(cause));
      }
    } finally {
      if (mountedRef.current) {
        setBusy(false);
        setCancelRequested(false);
      }
    }
  };

  const previewProject = async () => {
    if (!mountedRef.current) return;
    const generation = ++previewGeneration.current;
    const currentOperationId = operationId("preview");
    activeOperationId.current = currentOperationId;
    setBusy(true);
    setCancelRequested(false);
    setError(null);
    setWorkspaceResult(null);
    cancelOperationRef.current = cancelProjectImport;
    try {
      const plan = await withTimeout(
        previewProjectImport(projectPath, currentOperationId),
        "project-import-timeout",
      );
      if (mountedRef.current && generation === previewGeneration.current) {
        setPreview({ kind: "project", plan, path: projectPath });
      }
    } catch (cause) {
      if (cause instanceof Error && cause.message === "project-import-timeout") {
        await cancelProjectImport(currentOperationId).catch(() => undefined);
      }
      if (mountedRef.current && generation === previewGeneration.current) {
        setError(errorMessage(cause));
      }
    } finally {
      if (activeOperationId.current === currentOperationId) {
        activeOperationId.current = null;
      }
      if (mountedRef.current) {
        setBusy(false);
        setCancelRequested(false);
      }
    }
  };

  const previewWorkspace = async () => {
    if (!mountedRef.current) return;
    const generation = ++previewGeneration.current;
    const currentOperationId = operationId("preview");
    activeOperationId.current = currentOperationId;
    cancelOperationRef.current = cancelWorkspaceTaskImport;
    setBusy(true);
    setCancelRequested(false);
    setError(null);
    setWorkspaceResult(null);
    try {
      const plan = await withTimeout(
        previewWorkspaceTaskImport(
          workspacePath,
          workspaceTargetKind,
          workspaceTargetKind === "wsl" ? workspaceTargetDistro.trim() || null : null,
          currentOperationId,
        ),
        "workspace-task-import-timeout",
      );
      if (mountedRef.current && generation === previewGeneration.current) {
        setPreview({ kind: "workspace", plan, path: workspacePath });
        setWorkspaceSelectedIds(withWorkspaceDependencyClosure(
          plan,
          plan.items.filter(isSelectableWorkspaceItem).map((item) => item.id),
        ));
      }
    } catch (cause) {
      if (cause instanceof Error && cause.message === "workspace-task-import-timeout") {
        await cancelWorkspaceTaskImport(currentOperationId).catch(() => false);
      }
      if (mountedRef.current && generation === previewGeneration.current) {
        setError(errorMessage(cause));
      }
    } finally {
      if (activeOperationId.current === currentOperationId) activeOperationId.current = null;
      if (mountedRef.current) {
        setBusy(false);
        setCancelRequested(false);
      }
    }
  };

  const cancelPending = async () => {
    const currentOperationId = activeOperationId.current;
    if (!mountedRef.current || !busy || cancelRequested || !currentOperationId) return;
    previewGeneration.current += 1;
    setCancelRequested(true);
    setPreview(null);
    setError(null);
    await cancelOperationRef.current(currentOperationId).catch(() => false);
  };

  const discardPreview = () => {
    if (!mountedRef.current) return;
    previewGeneration.current += 1;
    setPreview(null);
    setWorkspaceSelectedIds(new Set());
    setWorkspaceResult(null);
    setError(null);
  };

  const items: ChangeSetItem[] = preview?.kind === "definitions"
    ? preview.plan.items.map((item) => ({
        path: item.kind + ":" + item.name + " (" + item.id + ")",
        before: item.status === "conflict" ? "(기존 정의 존재)" : "(신규)",
        after: item.detail,
        meta: item.status === "conflict" ? "충돌 — 건너뜀" : "비활성 draft · 확인 필요",
      }))
    : preview?.kind === "project" ? preview.plan.items.map((item) => ({
        path: item.source + ":" + item.sourceName + " (" + item.id + ")",
        before: "(로컬 source)",
        after: item.command + " · cwd: " + item.cwd,
        meta: item.status === "conflict"
          ? "충돌 — 건너뜀"
          : item.environmentKeys.length > 0
          ? "환경 키 " + item.environmentKeys.join(", ") + " · 실행 전 확인 필요"
          : "환경변수 값 미가져옴 · 실행 전 확인 필요",
      })) : [];

  const apply = async (selectedPaths: string[]) => {
    if (!mountedRef.current || !preview) return;
    const generation = previewGeneration.current;
    let currentOperationId: string | null = null;
    setBusy(true);
    setError(null);
    try {
      const selectedIds = preview.plan.items
        .filter((item) => selectedPaths.some((path) => path.endsWith("(" + item.id + ")")))
        .map((item) => item.id);
      if (preview.kind === "definitions") {
        const created = await applyImport(preview.json, selectedIds, preview.plan.revision);
        if (!mountedRef.current || generation !== previewGeneration.current) return;
        onDone(created);
      } else if (preview.kind === "project") {
        currentOperationId = operationId("apply");
        activeOperationId.current = currentOperationId;
        cancelOperationRef.current = cancelProjectImport;
        const result = await withTimeout(applyProjectImport(
          preview.path,
          preview.plan.sourceRoot,
          preview.plan.revision,
          selectedIds,
          currentOperationId,
        ), "project-import-timeout");
        if (!mountedRef.current || generation !== previewGeneration.current) return;
        onDone(result.created);
      } else {
        const workspaceIds = [...withWorkspaceDependencyClosure(preview.plan, workspaceSelectedIds)]
          .filter((id) => selectedIds.includes(id));
        if (workspaceIds.length === 0) {
          setError("가져올 수 있는 task를 하나 이상 선택하세요.");
          return;
        }
        currentOperationId = operationId("apply");
        activeOperationId.current = currentOperationId;
        cancelOperationRef.current = cancelWorkspaceTaskImport;
        const result = await withTimeout(applyWorkspaceTaskImport(
          preview.path,
          preview.plan.sourceRoot,
          preview.plan.projectIdentity,
          preview.plan.revision,
          preview.plan.targetKind,
          preview.plan.targetDistro,
          workspaceIds,
          currentOperationId,
        ), "workspace-task-import-timeout");
        if (!mountedRef.current || generation !== previewGeneration.current) return;
        setWorkspaceResult(result);
        onDone(result.created + result.updated, result);
      }
    } catch (cause) {
      if (
        currentOperationId &&
        cause instanceof Error &&
        (cause.message === "project-import-timeout" || cause.message === "workspace-task-import-timeout")
      ) {
        await cancelOperationRef.current(currentOperationId).catch(() => false);
      }
      if (mountedRef.current && generation === previewGeneration.current) {
        setError(errorMessage(cause));
      }
    } finally {
      if (currentOperationId && activeOperationId.current === currentOperationId) {
        activeOperationId.current = null;
      }
      if (mountedRef.current) {
        setBusy(false);
        setCancelRequested(false);
      }
    }
  };

  cancelPendingRef.current = cancelPending;
  const canCancel = busy && (mode === "project" || mode === "workspace");

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      previewGeneration.current += 1;
      const currentOperationId = activeOperationId.current;
      if (currentOperationId) {
        void cancelOperationRef.current(currentOperationId).catch(() => false);
      }
    };
  }, []);

  return (
    <div className="modal-backdrop" role="presentation">
      <div
        ref={dialogRef}
        className="import-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="import-title"
        aria-busy={busy}
      >
        <h2 id="import-title">정의와 task 가져오기</h2>
        {error && <div className="error" role="alert">{error}</div>}
        {!preview ? (
          <>
            <div className="import-mode-tabs" role="tablist" aria-label="가져오기 유형">
              <button id="import-tab-definitions" type="button" role="tab" aria-controls="import-content" aria-selected={mode === "definitions"} disabled={busy} onClick={() => setMode("definitions")}>
                Run Manager 정의
              </button>
              <button id="import-tab-project" type="button" role="tab" aria-controls="import-content" aria-selected={mode === "project"} disabled={busy} onClick={() => setMode("project")}>
                package/Cargo task
              </button>
              <button id="import-tab-workspace" type="button" role="tab" aria-controls="import-content" aria-selected={mode === "workspace"} disabled={busy} onClick={() => setMode("workspace")}>
                VS Code tasks
              </button>
            </div>
            <div
              id="import-content"
              role="tabpanel"
              tabIndex={0}
              aria-labelledby={mode === "definitions" ? "import-tab-definitions" : mode === "project" ? "import-tab-project" : "import-tab-workspace"}
            >
              {mode === "definitions" ? (
                <>
                <textarea
                  className="import-textarea"
                  placeholder='내보낸 JSON을 붙여넣으세요 ({"schemaVersion":1,"jobs":[...],"services":[...]})'
                  value={json}
                  onChange={(event) => setJson(event.currentTarget.value)}
                  spellCheck={false}
                />
                <div className="import-notice">환경변수 값은 가져오지 않습니다. 작업 디렉터리와 활성화 여부를 확인한 뒤 저장합니다.</div>
                <div className="import-actions">
                  <button type="button" className="button-primary" disabled={busy || !json.trim()} onClick={() => void previewDefinitions()}>미리보기</button>
                  <button type="button" className="button-secondary" disabled={cancelRequested || (busy && !canCancel)} onClick={() => {
                    if (canCancel) void cancelPending();
                    else onClose();
                  }}>{busy ? (canCancel ? "취소 중…" : "처리 중…") : "취소"}</button>
                </div>
                </>
              ) : mode === "project" ? (
                <>
                <label className="field">
                  <span>프로젝트 디렉터리</span>
                  <input
                    aria-label="프로젝트 디렉터리"
                    type="text"
                    value={projectPath}
                    onChange={(event) => setProjectPath(event.currentTarget.value)}
                    placeholder="C:\\work\\project 또는 /mnt/e/work/project"
                  />
                  <small className="field-help">package.json과 Cargo.toml만 읽습니다. npm/Cargo 실행·네트워크·.env 읽기는 없습니다.</small>
                </label>
                <div className="import-notice">가져온 task는 비활성 초안으로 저장되며, cwd와 환경 키를 확인하기 전에는 실행되지 않습니다.</div>
                <div className="import-actions">
                  <button type="button" className="button-primary" disabled={busy || !projectPath.trim()} onClick={() => void previewProject()}>로컬 파일 미리보기</button>
                  <button type="button" className="button-secondary" disabled={cancelRequested || (busy && !canCancel)} onClick={() => {
                    if (canCancel) void cancelPending();
                    else onClose();
                  }}>{busy ? (canCancel ? "취소 중…" : "처리 중…") : "취소"}</button>
                </div>
                </>
              ) : (
                <>
                <label className="field">
                  <span>workspace task 디렉터리</span>
                  <input
                    aria-label="workspace task 디렉터리"
                    type="text"
                    value={workspacePath}
                    onChange={(event) => setWorkspacePath(event.currentTarget.value)}
                    placeholder="C:\\work\\project 또는 /mnt/e/work/project"
                  />
                  <small className="field-help">프로젝트 바로 아래 .vscode/tasks.json 하나만 읽습니다. 미리보기는 읽기 전용이며 task·셸·네트워크를 실행하지 않습니다.</small>
                </label>
                <fieldset className="import-target-controls">
                  <legend>실행 대상</legend>
                  <div className="target-options">
                    <label className={`target-option ${workspaceTargetKind === "windows" ? "selected" : ""}`}>
                      <input
                        type="radio"
                        name="workspace-target-kind"
                        value="windows"
                        checked={workspaceTargetKind === "windows"}
                        onChange={() => setWorkspaceTargetKind("windows")}
                      />
                      <span><strong>Windows</strong><small>호스트 기준 override</small></span>
                    </label>
                    <label className={`target-option ${workspaceTargetKind === "wsl" ? "selected" : ""}`}>
                      <input
                        type="radio"
                        name="workspace-target-kind"
                        value="wsl"
                        checked={workspaceTargetKind === "wsl"}
                        onChange={() => setWorkspaceTargetKind("wsl")}
                      />
                      <span><strong>WSL</strong><small>Linux override + 배포판</small></span>
                    </label>
                  </div>
                  {workspaceTargetKind === "wsl" ? (
                    <label className="field target-distro-field">
                      <span>WSL 배포판</span>
                      <input
                        aria-label="workspace task WSL 배포판"
                        value={workspaceTargetDistro}
                        onChange={(event) => setWorkspaceTargetDistro(event.currentTarget.value)}
                        placeholder="Ubuntu"
                      />
                    </label>
                  ) : null}
                </fieldset>
                <div className="import-notice">환경변수 값은 읽거나 가져오지 않습니다. 미리보기에는 선언된 키 이름만 표시되며, 가져온 작업은 비활성·미신뢰 초안으로 저장됩니다.</div>
                <div className="import-actions">
                  <button type="button" className="button-primary" disabled={busy || !workspacePath.trim() || (workspaceTargetKind === "wsl" && !workspaceTargetDistro.trim())} onClick={() => void previewWorkspace()}>tasks.json 미리보기</button>
                  <button type="button" className="button-secondary" disabled={cancelRequested || (busy && !canCancel)} onClick={() => {
                    if (canCancel) void cancelPending();
                    else onClose();
                  }}>{busy ? (canCancel ? "취소 중…" : "처리 중…") : "취소"}</button>
                </div>
                </>
              )}
            </div>
          </>
        ) : (
          <>
            {preview.kind === "project" ? (
              <div className="import-source-summary">
                <strong>읽은 프로젝트</strong>
                <code>{preview.plan.sourceRoot}</code>
                <span>{preview.plan.files.map((file) => file.path + " (" + file.bytes + "바이트)").join(" · ")}</span>
                <p>미리보기 revision {preview.plan.revision} · 원본 변경 시 적용이 거부됩니다.</p>
              </div>
            ) : null}
            {preview.kind === "workspace" ? (
              <WorkspaceTaskPreview
                plan={preview.plan}
                selectedIds={workspaceSelectedIds}
                busy={busy}
                cancelRequested={cancelRequested}
                result={workspaceResult}
                onToggle={(id) => setWorkspaceSelectedIds((current) => {
                  return toggleWorkspaceTaskSelection(preview.plan, current, id);
                })}
                onApprove={() => void apply([...workspaceSelectedIds].map((id) => `workspace:${id} (${id})`))}
                onDiscard={discardPreview}
                onCancel={() => void cancelPending()}
              />
            ) : (
              <>
                <ChangeSetPreview
                  items={items}
                  title="가져오기 계획"
                  approveLabel="선택 항목 저장"
                  disabled={busy}
                  onApprove={(paths) => void apply(paths)}
                  onReject={discardPreview}
                  onCancel={discardPreview}
                />
                {busy && preview.kind === "project" ? (
                  <div className="import-actions">
                    <button type="button" className="button-secondary" disabled={cancelRequested} onClick={() => void cancelPending()}>
                      {cancelRequested ? "취소 중…" : "가져오기 취소"}
                    </button>
                  </div>
                ) : null}
              </>
            )}
          </>
        )}
      </div>
    </div>
  );
}
