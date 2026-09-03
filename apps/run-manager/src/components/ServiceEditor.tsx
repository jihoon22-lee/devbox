import { useMemo, useState, type FormEvent } from "react";
import { friendlyErrorMessage } from "../api";
import {
  draftFromService,
  EMPTY_SERVICE_DRAFT,
  serviceFieldErrorFromBackend,
  toServiceInput,
  validateServiceDraft,
  type ServiceDraft,
} from "../lib/serviceEditor";
import type { EnvironmentDraft, Job, ServiceFieldErrors, ServiceInput, TargetKind } from "../types";

interface ServiceEditorProps {
  service: Job | null;
  onSave: (input: ServiceInput) => Promise<void>;
  onCancel: () => void;
}

interface ServiceEditorState {
  key: string;
  draft: ServiceDraft;
  errors: ServiceFieldErrors;
}

function emptyEditorState(service: Job | null): ServiceEditorState {
  return {
    key: service?.id ?? "",
    draft: service ? draftFromService(service) : { ...EMPTY_SERVICE_DRAFT, environment: [] },
    errors: {},
  };
}

export default function ServiceEditor({ service, onSave, onCancel }: ServiceEditorProps) {
  const serviceKey = service?.id ?? "";
  // Derived during render rather than reset from an effect, for the same reason as JobEditor: a
  // service-list refresh replaces `service` with an equal-but-new object.
  const [editorState, setEditorState] = useState<ServiceEditorState>(() => emptyEditorState(service));
  const { draft, errors } = useMemo(
    () => (editorState.key === serviceKey ? editorState : emptyEditorState(service)),
    [editorState, service, serviceKey],
  );
  const patchEditorState = (apply: (state: ServiceEditorState) => ServiceEditorState) =>
    setEditorState((previous) => ({
      ...apply(previous.key === serviceKey ? previous : emptyEditorState(service)),
      key: serviceKey,
    }));
  const setDraft = (apply: (value: ServiceDraft) => ServiceDraft) =>
    patchEditorState((state) => ({ ...state, draft: apply(state.draft) }));
  const setErrors = (next: ServiceFieldErrors | ((value: ServiceFieldErrors) => ServiceFieldErrors)) =>
    patchEditorState((state) => ({
      ...state,
      errors: typeof next === "function" ? next(state.errors) : next,
    }));
  const [saving, setSaving] = useState(false);

  const update = <K extends keyof ServiceDraft>(field: K, value: ServiceDraft[K]) => {
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
      environmentAction: "replace",
      environment: current.environment.map((entry) =>
        entry.id === id ? { ...entry, [field]: value } : entry,
      ),
    }));
    setErrors((current) => ({ ...current, env: undefined }));
  };

  const addEnvironment = () => {
    const row: EnvironmentDraft = {
      id: `env-${Math.random().toString(36).slice(2)}`,
      key: "",
      value: "",
      persisted: false,
    };
    setDraft((current) => ({
      ...current,
      environmentAction: "replace",
      environment: [...current.environment.filter((entry) => !entry.persisted), row],
    }));
    setErrors((current) => ({ ...current, env: undefined }));
  };

  const replacePersistedEnvironment = () => {
    setDraft((current) => ({
      ...current,
      environmentAction: "replace",
      environment: [{ id: `env-${Math.random().toString(36).slice(2)}`, key: "", value: "", persisted: false }],
    }));
    setErrors((current) => ({ ...current, env: undefined }));
  };

  const clearPersistedEnvironment = () => {
    setDraft((current) => ({ ...current, environmentAction: "clear", environment: [] }));
    setErrors((current) => ({ ...current, env: undefined }));
  };

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const validation = validateServiceDraft(draft);
    if (Object.keys(validation).length > 0) {
      setErrors(validation);
      return;
    }
    setSaving(true);
    try {
      await onSave(toServiceInput(draft));
    } catch (cause) {
      const raw = cause instanceof Error ? cause.message : typeof cause === "string" ? cause : "";
      const backendErrors = serviceFieldErrorFromBackend(raw);
      setErrors(Object.keys(backendErrors).length > 0
        ? backendErrors
        : { name: friendlyErrorMessage(cause) });
    } finally {
      setSaving(false);
    }
  };

  const title = service ? "서비스 편집" : "새 서비스";

  return (
    <form className="editor-layout" onSubmit={(event) => void handleSubmit(event)} noValidate>
      <div className="editor-main">
        <header className="editor-header">
          <div>
            <span className="eyebrow">서비스 편집기</span>
            <h2>{title}</h2>
            <p className="subtitle">서비스 정의와 재시작·헬스체크 정책을 저장합니다.</p>
          </div>
          <div className="editor-actions">
            <button type="button" className="button-secondary" onClick={onCancel} disabled={saving}>
              취소
            </button>
            <button type="submit" className="button-primary" disabled={saving}>
              {saving ? "저장 중…" : "서비스 저장"}
            </button>
          </div>
        </header>

        <section className="form-card" aria-label="서비스 정의">
          <div className="form-grid">
            <label className="field field-wide">
              <span>이름</span>
              <input
                aria-label="서비스 이름"
                value={draft.name}
                onChange={(event) => update("name", event.target.value)}
                aria-invalid={Boolean(errors.name)}
                autoFocus
              />
              {errors.name ? <small className="field-error" role="alert">{errors.name}</small> : null}
            </label>
            <label className="field field-wide">
              <span>명령</span>
              <textarea
                aria-label="서비스 실행 명령"
                value={draft.command}
                onChange={(event) => update("command", event.target.value)}
                rows={3}
                spellCheck={false}
                aria-invalid={Boolean(errors.command)}
                placeholder="npm start"
              />
              {errors.command ? <small className="field-error" role="alert">{errors.command}</small> : null}
            </label>
            <label className="field field-wide">
              <span>작업 디렉터리 <em>(선택)</em></span>
              <input
                aria-label="서비스 작업 디렉터리"
                value={draft.cwd}
                onChange={(event) => update("cwd", event.target.value)}
                placeholder="C:\\projects\\service 또는 /home/me/service"
              />
            </label>
          </div>

          <fieldset className="form-section">
            <legend>실행 대상</legend>
            <div className="target-options">
              <label className={`target-option ${draft.targetKind === "windows" ? "selected" : ""}`}>
                <input
                  type="radio"
                  name="service-target-kind"
                  value="windows"
                  checked={draft.targetKind === "windows"}
                  onChange={() => updateTarget("windows")}
                />
                <span><strong>Windows</strong><small>호스트에서 실행</small></span>
              </label>
              <label className={`target-option ${draft.targetKind === "wsl" ? "selected" : ""}`}>
                <input
                  type="radio"
                  name="service-target-kind"
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
                  aria-label="서비스 WSL 배포판"
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

          <section className="form-section" aria-labelledby="service-policy-title">
            <div className="section-heading">
              <div>
                <h3 id="service-policy-title">서비스 정책</h3>
                <p className="section-description">실제 시작·중지·재시작은 실행 어댑터 연결 후 제공됩니다.</p>
              </div>
            </div>
            <div className="policy-grid">
              <label className="toggle-field">
                <input
                  type="checkbox"
                  aria-label="데몬 시작 시 자동 시작"
                  checked={draft.autoStart}
                  onChange={(event) => update("autoStart", event.target.checked)}
                />
                <span><strong>자동 시작</strong><small>Run Manager 기동 시 시작 대상으로 표시합니다.</small></span>
              </label>
              <label className="field">
                <span>재시작 정책</span>
                <select
                  aria-label="재시작 정책"
                  value={draft.restartPolicy}
                  onChange={(event) => update("restartPolicy", event.target.value as ServiceDraft["restartPolicy"])}
                >
                  <option value="never">사용 안 함</option>
                  <option value="on-failure">실패 시</option>
                  <option value="always">항상</option>
                </select>
              </label>
            </div>
          </section>

          <section className="form-section" aria-labelledby="health-title">
            <div className="section-heading">
              <div>
                <h3 id="health-title">TCP 헬스체크</h3>
                <p className="section-description">주소와 포트는 로컬 루프백 endpoint만 저장할 수 있습니다.</p>
              </div>
            </div>
            <label className="toggle-field health-toggle">
              <input
                type="checkbox"
                aria-label="TCP 헬스체크 사용"
                checked={draft.healthTcpEnabled}
                onChange={(event) => update("healthTcpEnabled", event.target.checked)}
              />
              <span><strong>TCP probe 사용</strong><small>프로세스 생존과 함께 지정한 로컬 포트를 확인합니다.</small></span>
            </label>
            {draft.healthTcpEnabled ? (
              <div className="service-health-grid">
                <label className="field">
                  <span>로컬 주소</span>
                  <input
                    aria-label="TCP 헬스체크 주소"
                    value={draft.healthTcpAddress}
                    onChange={(event) => update("healthTcpAddress", event.target.value)}
                    placeholder="127.0.0.1"
                    aria-invalid={Boolean(errors.healthTcpAddress)}
                  />
                  {errors.healthTcpAddress ? <small className="field-error" role="alert">{errors.healthTcpAddress}</small> : null}
                </label>
                <label className="field">
                  <span>포트</span>
                  <input
                    aria-label="TCP 헬스체크 포트"
                    inputMode="numeric"
                    value={draft.healthTcpPort}
                    onChange={(event) => update("healthTcpPort", event.target.value)}
                    placeholder="3000"
                    aria-invalid={Boolean(errors.healthTcpPort)}
                  />
                  {errors.healthTcpPort ? <small className="field-error" role="alert">{errors.healthTcpPort}</small> : null}
                </label>
              </div>
            ) : null}
            <div className="adapter-warning" role="note">
              시작 grace는 권장값인 10초로 고정됩니다. 서비스 정의에서는 임의로 변경할 수 없습니다.
            </div>
          </section>

          <section className="form-section" aria-labelledby="service-environment-title">
            <div className="section-heading">
              <div>
                <h3 id="service-environment-title">환경변수</h3>
                <p className="section-description">보호된 환경변수 값은 읽기 API에서 마스킹되며 이 화면에도 평문으로 표시하지 않습니다.</p>
              </div>
            </div>
            <div className="adapter-warning" role="note">
              {service?.envConfigured
                ? "저장된 환경변수는 DPAPI 보호 상태입니다. 기존 값은 마스킹 상태로 유지됩니다."
                : "새 환경변수 값은 Windows 사용자 범위 DPAPI로 암호화되어 저장되며 다시 표시되지 않습니다."}
            </div>
            <div className="section-heading">
              <button type="button" className="button-secondary small" onClick={addEnvironment}>변수 추가</button>
            </div>
            {draft.environment.length === 0 ? <p className="muted">설정된 환경변수가 없습니다.</p> : null}
            <div className="environment-list">
              {draft.environment.map((entry) =>
                entry.persisted ? (
                  <div className="environment-row persisted" key={entry.id}>
                    <span className="masked-key">저장된 환경변수</span>
                    <span className="masked-value" aria-label="마스킹된 환경변수 값">••••••••</span>
                    <span className="muted">DPAPI 보호됨</span>
                    <button
                      type="button"
                      className="button-secondary small"
                      aria-label="기존 환경변수 유지"
                      onClick={() => setDraft((current) => ({ ...current, environmentAction: "keep" }))}
                    >
                      유지
                    </button>
                    <button type="button" className="button-secondary small" aria-label="기존 환경변수 교체" onClick={replacePersistedEnvironment}>
                      교체
                    </button>
                    <button type="button" className="button-secondary small" aria-label="기존 환경변수 삭제" onClick={clearPersistedEnvironment}>
                      삭제
                    </button>
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
                      onClick={() => setDraft((current) => ({
                        ...current,
                        environment: current.environment.filter((item) => item.id !== entry.id),
                      }))}
                    >
                      ×
                    </button>
                  </div>
                ),
              )}
            </div>
            {errors.env ? <small className="field-error" role="alert">{errors.env}</small> : null}
          </section>
        </section>
      </div>
    </form>
  );
}
