// 정의/task import 다이얼로그.
// 기존 export JSON과 로컬 package.json/Cargo.toml 모두
// preview -> 항목 선택 -> 명시적 적용 순서를 따른다.

import { useEffect, useRef, useState } from "react";
import ChangeSetPreview, { type ChangeSetItem } from "@devbox/diff-view";
import {
  applyImport,
  applyProjectImport,
  cancelProjectImport,
  importDefinitions,
  previewProjectImport,
  type ImportPlan,
  type ProjectImportPlan,
} from "../api";

interface Props {
  onDone: (created: number) => void;
  onClose: () => void;
}

type Preview =
  | { kind: "definitions"; plan: ImportPlan; json: string }
  | { kind: "project"; plan: ProjectImportPlan; path: string };

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
    return "프로젝트 import가 제한 시간 안에 끝나지 않았습니다. 파일을 확인한 뒤 다시 시도하세요.";
  }
  if (value === "project-import-cancelled") {
    return "프로젝트 import를 취소했습니다.";
  }
  if (value.startsWith("project-import-")) {
    return "프로젝트 import를 읽지 못했습니다. 로컬 디렉터리와 파일 크기를 확인하세요.";
  }
  return value || "가져오기를 완료하지 못했습니다.";
}

export default function ImportDialog({ onDone, onClose }: Props) {
  const [mode, setMode] = useState<"definitions" | "project">("definitions");
  const [json, setJson] = useState("");
  const [projectPath, setProjectPath] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [busy, setBusy] = useState(false);
  const [cancelRequested, setCancelRequested] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const previewGeneration = useRef(0);
  const activeOperationId = useRef<string | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const busyRef = useRef(false);
  const cancelPendingRef = useRef<() => Promise<void>>(() => Promise.resolve());
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
    const generation = ++previewGeneration.current;
    activeOperationId.current = null;
    setBusy(true);
    setCancelRequested(false);
    setError(null);
    try {
      const plan = await importDefinitions(json);
      if (generation === previewGeneration.current) setPreview({ kind: "definitions", plan, json });
    } catch (cause) {
      if (generation === previewGeneration.current) setError(errorMessage(cause));
    } finally {
      setBusy(false);
      setCancelRequested(false);
    }
  };

  const previewProject = async () => {
    const generation = ++previewGeneration.current;
    const currentOperationId = operationId("preview");
    activeOperationId.current = currentOperationId;
    setBusy(true);
    setCancelRequested(false);
    setError(null);
    try {
      const plan = await withTimeout(
        previewProjectImport(projectPath, currentOperationId),
        "project-import-timeout",
      );
      if (generation === previewGeneration.current) setPreview({ kind: "project", plan, path: projectPath });
    } catch (cause) {
      if (cause instanceof Error && cause.message === "project-import-timeout") {
        await cancelProjectImport(currentOperationId).catch(() => undefined);
      }
      if (generation === previewGeneration.current) setError(errorMessage(cause));
    } finally {
      if (activeOperationId.current === currentOperationId) {
        activeOperationId.current = null;
      }
      setBusy(false);
      setCancelRequested(false);
    }
  };

  const cancelPending = async () => {
    const currentOperationId = activeOperationId.current;
    if (!busy || cancelRequested || !currentOperationId) return;
    previewGeneration.current += 1;
    setCancelRequested(true);
    setPreview(null);
    setError(null);
    await cancelProjectImport(currentOperationId).catch(() => undefined);
  };

  const discardPreview = () => {
    previewGeneration.current += 1;
    setPreview(null);
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
    if (!preview) return;
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
        if (generation !== previewGeneration.current) return;
        onDone(created);
      } else {
        currentOperationId = operationId("apply");
        activeOperationId.current = currentOperationId;
        const result = await withTimeout(applyProjectImport(
          preview.path,
          preview.plan.sourceRoot,
          preview.plan.revision,
          selectedIds,
          currentOperationId,
        ), "project-import-timeout");
        if (generation !== previewGeneration.current) return;
        onDone(result.created);
      }
    } catch (cause) {
      if (
        currentOperationId &&
        cause instanceof Error &&
        cause.message === "project-import-timeout"
      ) {
        await cancelProjectImport(currentOperationId).catch(() => undefined);
      }
      if (generation === previewGeneration.current) setError(errorMessage(cause));
    } finally {
      if (currentOperationId && activeOperationId.current === currentOperationId) {
        activeOperationId.current = null;
      }
      setBusy(false);
      setCancelRequested(false);
    }
  };

  cancelPendingRef.current = cancelPending;
  const canCancel = busy && mode === "project";

  useEffect(() => () => {
    previewGeneration.current += 1;
    const currentOperationId = activeOperationId.current;
    if (currentOperationId) {
      void cancelProjectImport(currentOperationId).catch(() => undefined);
    }
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
            </div>
            <div
              id="import-content"
              role="tabpanel"
              tabIndex={0}
              aria-labelledby={mode === "definitions" ? "import-tab-definitions" : "import-tab-project"}
            >
              {mode === "definitions" ? (
                <>
                <textarea
                  className="import-textarea"
                  placeholder='export한 JSON을 붙여넣으세요 ({"schemaVersion":1,"jobs":[...],"services":[...]})'
                  value={json}
                  onChange={(event) => setJson(event.currentTarget.value)}
                  spellCheck={false}
                />
                <div className="import-notice">환경변수 값은 가져오지 않습니다. 작업 디렉터리와 활성화 여부를 확인한 뒤 저장합니다.</div>
                <div className="import-actions">
                  <button className="button-primary" disabled={busy || !json.trim()} onClick={() => void previewDefinitions()}>미리보기</button>
                  <button className="button-secondary" disabled={cancelRequested || (busy && !canCancel)} onClick={() => {
                    if (canCancel) void cancelPending();
                    else onClose();
                  }}>{busy ? (canCancel ? "취소 중…" : "처리 중…") : "취소"}</button>
                </div>
                </>
              ) : (
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
                <div className="import-notice">가져온 task는 비활성 draft로 저장되며, cwd와 환경 키를 확인하기 전에는 실행되지 않습니다.</div>
                <div className="import-actions">
                  <button className="button-primary" disabled={busy || !projectPath.trim()} onClick={() => void previewProject()}>로컬 파일 미리보기</button>
                  <button className="button-secondary" disabled={cancelRequested || (busy && !canCancel)} onClick={() => {
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
                <span>{preview.plan.files.map((file) => file.path + " (" + file.bytes + " bytes)").join(" · ")}</span>
                <p>preview revision {preview.plan.revision} · 원본 변경 시 적용이 거부됩니다.</p>
              </div>
            ) : null}
            <ChangeSetPreview
              items={items}
              title="import 계획"
              approveLabel="선택 항목 저장"
              disabled={busy}
              onApprove={(paths) => void apply(paths)}
              onReject={discardPreview}
              onCancel={discardPreview}
            />
            {busy && preview.kind === "project" ? (
              <div className="import-actions">
                <button className="button-secondary" disabled={cancelRequested} onClick={() => void cancelPending()}>
                  {cancelRequested ? "취소 중…" : "import 취소"}
                </button>
              </div>
            ) : null}
          </>
        )}
      </div>
    </div>
  );
}
