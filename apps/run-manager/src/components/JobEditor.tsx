import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { previewCron } from "../api";
import {
  draftFromJob,
  EMPTY_JOB_DRAFT,
  fieldErrorFromBackend,
  toJobInput,
  validateJobDraft,
  type JobDraft,
} from "../lib/jobEditor";
import type { CronPreviewItem, EnvironmentDraft, Job, JobFieldErrors, JobInput, TargetKind } from "../types";
import CronBuilder from "./CronBuilder";
import NextRunsPreview from "./NextRunsPreview";

interface JobEditorProps {
  job: Job | null;
  onSave: (input: JobInput) => Promise<void>;
  onCancel: () => void;
}

function newEnvironmentRow(): EnvironmentDraft {
  return { id: `env-${Math.random().toString(36).slice(2)}`, key: "", value: "", persisted: false };
}

export default function JobEditor({ job, onSave, onCancel }: JobEditorProps) {
  const [draft, setDraft] = useState<JobDraft>(() => (job ? draftFromJob(job) : { ...EMPTY_JOB_DRAFT }));
  const [errors, setErrors] = useState<JobFieldErrors>({});
  const [saving, setSaving] = useState(false);
  const [previewItems, setPreviewItems] = useState<CronPreviewItem[]>([]);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const previewRequest = useRef(0);

  useEffect(() => {
    setDraft(job ? draftFromJob(job) : { ...EMPTY_JOB_DRAFT, environment: [] });
    setErrors({});
  }, [job]);

  useEffect(() => {
    const requestId = ++previewRequest.current;
    const expression = draft.cronExpr.trim();
    if (!expression) {
      setPreviewItems([]);
      setPreviewError(null);
      setPreviewLoading(false);
      return;
    }
    setPreviewLoading(true);
    const timer = window.setTimeout(() => {
      void previewCron(expression)
        .then((items) => {
          if (requestId !== previewRequest.current) return;
          setPreviewItems(items.slice(0, 5));
          setPreviewError(null);
        })
        .catch((cause: unknown) => {
          if (requestId !== previewRequest.current) return;
          setPreviewItems([]);
          setPreviewError(cause instanceof Error ? cause.message : String(cause));
        })
        .finally(() => {
          if (requestId === previewRequest.current) setPreviewLoading(false);
        });
    }, 180);
    return () => window.clearTimeout(timer);
  }, [draft.cronExpr]);

  const title = useMemo(() => (job ? "작업 편집" : "새 작업"), [job]);
  const update = <K extends keyof JobDraft>(field: K, value: JobDraft[K]) => {
    setDraft((current) => ({ ...current, [field]: value }));
    setErrors((current) => ({ ...current, [field]: undefined }));
  };

  const updateTarget = (targetKind: TargetKind) => {
    update("targetKind", targetKind);
    if (targetKind === "windows") update("targetDistro", "");
  };

  const updateEnvironment = (id: string, field: "key" | "value", value: string) => {
    setDraft((current) => ({
      ...current,
      environment: current.environment.map((entry) => (entry.id === id ? { ...entry, [field]: value } : entry)),
    }));
    setErrors((current) => ({ ...current, env: undefined }));
  };

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const validation = validateJobDraft(draft);
    if (Object.keys(validation).length > 0) {
      setErrors(validation);
      return;
    }
    setSaving(true);
    try {
      await onSave(toJobInput(draft));
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      const backendErrors = fieldErrorFromBackend(message);
      setErrors(Object.keys(backendErrors).length > 0 ? backendErrors : { name: message });
    } finally {
      setSaving(false);
    }
  };

  return (
    <form className="editor-layout" onSubmit={(event) => void handleSubmit(event)} noValidate>
      <div className="editor-main">
        <header className="editor-header">
          <div>
            <span className="eyebrow">JOB EDITOR</span>
            <h2>{title}</h2>
            <p className="subtitle">예약 실행에 필요한 정의를 저장합니다.</p>
          </div>
          <div className="editor-actions">
            <button type="button" className="button-secondary" onClick={onCancel} disabled={saving}>
              취소
            </button>
            <button type="submit" className="button-primary" disabled={saving}>
              {saving ? "저장 중…" : "작업 저장"}
            </button>
          </div>
        </header>

        <section className="form-card" aria-label="작업 정의">
          <div className="form-grid">
            <label className="field field-wide">
              <span>이름</span>
              <input
                aria-label="작업 이름"
                value={draft.name}
                onChange={(event) => update("name", event.target.value)}
                aria-invalid={Boolean(errors.name)}
                aria-describedby={errors.name ? "name-error" : undefined}
                autoFocus
              />
              {errors.name ? <small id="name-error" className="field-error">{errors.name}</small> : null}
            </label>
            <label className="field field-wide">
              <span>명령</span>
              <textarea
                aria-label="실행 명령"
                value={draft.command}
                onChange={(event) => update("command", event.target.value)}
                rows={3}
                spellCheck={false}
                aria-invalid={Boolean(errors.command)}
                aria-describedby={errors.command ? "command-error" : undefined}
                placeholder="python main.py"
              />
              {errors.command ? <small id="command-error" className="field-error">{errors.command}</small> : null}
            </label>
            <label className="field field-wide">
              <span>작업 디렉터리 <em>(선택)</em></span>
              <input
                aria-label="작업 디렉터리"
                value={draft.cwd}
                onChange={(event) => update("cwd", event.target.value)}
                placeholder="C:\\projects\\demo 또는 /home/me/demo"
              />
            </label>
          </div>

          <fieldset className="form-section">
            <legend>실행 대상</legend>
            <div className="target-options">
              <label className={`target-option ${draft.targetKind === "windows" ? "selected" : ""}`}>
                <input
                  type="radio"
                  name="target-kind"
                  value="windows"
                  checked={draft.targetKind === "windows"}
                  onChange={() => updateTarget("windows")}
                />
                <span><strong>Windows</strong><small>호스트에서 실행</small></span>
              </label>
              <label className={`target-option ${draft.targetKind === "wsl" ? "selected" : ""}`}>
                <input
                  type="radio"
                  name="target-kind"
                  value="wsl"
                  checked={draft.targetKind === "wsl"}
                  onChange={() => updateTarget("wsl")}
                />
                <span><strong>WSL</strong><small>지정한 배포판에서 실행</small></span>
              </label>
            </div>
            {draft.targetKind === "wsl" ? (
              <label className="field target-distro-field">
                <span>WSL 배포판</span>
                <input
                  aria-label="WSL 배포판"
                  value={draft.targetDistro}
                  onChange={(event) => update("targetDistro", event.target.value)}
                  placeholder="Ubuntu"
                  aria-invalid={Boolean(errors.targetDistro)}
                />
                {errors.targetDistro ? <small className="field-error" role="alert">{errors.targetDistro}</small> : null}
              </label>
            ) : null}
            {draft.targetKind === "windows" && errors.targetDistro ? (
              <small className="field-error" role="alert">{errors.targetDistro}</small>
            ) : null}
          </fieldset>

          <section className="form-section" aria-labelledby="environment-title">
            <div className="section-heading">
              <div>
                <h3 id="environment-title">환경변수</h3>
                <p className="section-description">값은 명령줄·로그에 포함하지 않고 보호된 저장소에만 보관합니다.</p>
              </div>
              <button
                type="button"
                className="button-secondary small"
                onClick={() => setDraft((current) => ({ ...current, environment: [...current.environment, newEnvironmentRow()] }))}
              >
                변수 추가
              </button>
            </div>
            <div className="adapter-warning" role="note">
              Windows DPAPI 어댑터가 연결되기 전에는 새 환경변수 값을 저장할 수 없습니다. 기존 값은 마스킹 상태로 유지됩니다.
            </div>
            {draft.environment.length === 0 ? <p className="muted">설정된 환경변수가 없습니다.</p> : null}
            <div className="environment-list">
              {draft.environment.map((entry) =>
                entry.persisted ? (
                  <div className="environment-row persisted" key={entry.id}>
                    <span className="masked-key">저장된 환경변수</span>
                    <span className="masked-value" aria-label="마스킹된 환경변수 값">••••••••</span>
                    <span className="muted">DPAPI 보호됨</span>
                  </div>
                ) : (
                  <div className="environment-row" key={entry.id}>
                    <input
                      aria-label="환경변수 이름"
                      value={entry.key}
                      onChange={(event) => updateEnvironment(entry.id, "key", event.target.value)}
                      placeholder="NAME"
                    />
                    <input
                      aria-label="환경변수 값"
                      type="password"
                      value={entry.value}
                      onChange={(event) => updateEnvironment(entry.id, "value", event.target.value)}
                      placeholder="값 (저장 전용)"
                    />
                    <button
                      type="button"
                      className="icon-button"
                      aria-label="환경변수 삭제"
                      onClick={() => setDraft((current) => ({ ...current, environment: current.environment.filter((item) => item.id !== entry.id) }))}
                    >
                      ×
                    </button>
                  </div>
                ),
              )}
            </div>
            {errors.env ? <small className="field-error" role="alert">{errors.env}</small> : null}
          </section>

          <CronBuilder value={draft.cronExpr} onChange={(value) => update("cronExpr", value)} error={errors.cronExpr} />

          <section className="form-section" aria-labelledby="policy-title">
            <legend id="policy-title">실행 정책</legend>
            <div className="policy-grid">
              <label className="toggle-field">
                <input type="checkbox" checked={draft.enabled} onChange={(event) => update("enabled", event.target.checked)} />
                <span><strong>활성화</strong><small>스케줄러가 이 작업을 평가합니다.</small></span>
              </label>
              <label className="toggle-field">
                <input type="checkbox" checked={draft.catchUp} onChange={(event) => update("catchUp", event.target.checked)} />
                <span><strong>놓친 실행 따라잡기</strong><small>중단 중 마지막 실행 한 번만 재개합니다.</small></span>
              </label>
              <label className="field">
                <span>중복 실행</span>
                <select
                  aria-label="중복 실행 정책"
                  value={draft.overlapPolicy}
                  onChange={(event) => update("overlapPolicy", event.target.value as JobInput["overlapPolicy"])}
                >
                  <option value="skip">건너뛰기</option>
                  <option value="queue">대기열에 추가</option>
                  <option value="kill-previous">이전 실행 종료 후 실행</option>
                </select>
              </label>
            </div>
          </section>
        </section>
      </div>
      <NextRunsPreview items={previewItems} loading={previewLoading} error={previewError} />
    </form>
  );
}
