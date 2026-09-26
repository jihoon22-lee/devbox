import { RUNTIME_STATUS_LABEL } from "../lib/profilePresentation";
import {
  MAX_EXPECTED_PORTS_INPUT_CHARS,
  MAX_ENVIRONMENT_SOURCE_BYTES,
  MAX_PROFILE_NAME_CHARS,
  MAX_PROFILE_PATH_BYTES,
  MAX_SERVICE_ID_CHARS,
  MAX_SERVICES,
  MAX_WSL_DISTRO_CHARS,
  newServiceDraftRow,
} from "../lib/profileEditor";
import { formatRuntimeFreshness } from "../lib/runtimeSuggestions";
import type * as React from "react";

interface Props {
  busy: boolean;
  runtimeLoading: boolean;
  runtimeAccepting: boolean;
  environmentLoading: boolean;
  onCancelPreflight: () => void;
  closeEditor: () => void;
  editing: import("../lib/profileEditor").ProfileDraft;
  onSave: () => Promise<void>;
  draftValidation: import("../lib/profileEditor").ProfileDraftValidation | null;
  patch: (p: Partial<import("../lib/profileEditor").ProfileDraft>) => void;
  patchProjectLocation: (p: Partial<import("../lib/profileEditor").ProfileDraft>) => void;
  patchEnvironmentSource: (source: string) => void;
  inspectEnvironment: () => Promise<void>;
  loadRuntimeSuggestions: () => Promise<void>;
  runtimeSuggestions: import("../api").RuntimeSuggestions | null;
  existingRuntimePorts: Set<number>;
  selectedRuntimePorts: Set<number>;
  runtimeActionable: boolean;
  setSelectedRuntimePorts: React.Dispatch<React.SetStateAction<Set<number>>>;
  acceptRuntimePorts: () => Promise<void>;
}

export function ProfileEditor({
  busy,
  runtimeLoading,
  runtimeAccepting,
  environmentLoading,
  onCancelPreflight,
  closeEditor,
  editing,
  onSave,
  draftValidation,
  patch,
  patchProjectLocation,
  patchEnvironmentSource,
  inspectEnvironment,
  loadRuntimeSuggestions,
  runtimeSuggestions,
  existingRuntimePorts,
  selectedRuntimePorts,
  runtimeActionable,
  setSelectedRuntimePorts,
  acceptRuntimePorts,
}: Props) {
  return (
    <section
      className="panel editor-panel"
      aria-labelledby="profile-editor-title"
      aria-busy={busy || runtimeLoading || runtimeAccepting || environmentLoading}
      onKeyDown={(event) => {
        if (event.key === "Escape" && !event.nativeEvent.isComposing && !busy) {
          event.preventDefault();
          onCancelPreflight();
          closeEditor();
        }
      }}
    >
      <h2 id="profile-editor-title">{editing.id ? "프로필 편집" : "새 프로필"}</h2>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void onSave();
        }}
      >
        <label className="field" htmlFor="profile-name">
          <span>이름</span>
          <input
            id="profile-name"
            value={editing.name}
            maxLength={MAX_PROFILE_NAME_CHARS}
            autoFocus
            disabled={busy}
            aria-invalid={Boolean(draftValidation?.errors.name)}
            aria-describedby={draftValidation?.errors.name ? "profile-name-error" : undefined}
            onChange={(e) => patch({ name: e.currentTarget.value })}
          />
          {draftValidation?.errors.name && (
            <span id="profile-name-error" className="field-error" role="alert">
              {draftValidation.errors.name}
            </span>
          )}
        </label>
        <label className="field" htmlFor="profile-windows-path">
          <span>Windows 경로</span>
          <input
            id="profile-windows-path"
            value={editing.windowsPath}
            maxLength={MAX_PROFILE_PATH_BYTES}
            disabled={busy || environmentLoading}
            aria-invalid={Boolean(draftValidation?.errors.projectPath)}
            aria-describedby={draftValidation?.errors.projectPath ? "profile-project-path-error" : undefined}
            onChange={(e) => patchProjectLocation({ windowsPath: e.currentTarget.value })}
          />
        </label>
        <label className="field" htmlFor="profile-wsl-distro">
          <span>WSL 배포판</span>
          <input
            id="profile-wsl-distro"
            value={editing.wslDistro}
            maxLength={MAX_WSL_DISTRO_CHARS}
            disabled={busy || environmentLoading}
            aria-invalid={Boolean(draftValidation?.errors.wsl)}
            aria-describedby={draftValidation?.errors.wsl ? "profile-wsl-error" : undefined}
            onChange={(e) => patchProjectLocation({ wslDistro: e.currentTarget.value })}
          />
        </label>
        <label className="field" htmlFor="profile-wsl-path">
          <span>WSL 경로</span>
          <input
            id="profile-wsl-path"
            value={editing.wslPath}
            maxLength={MAX_PROFILE_PATH_BYTES}
            disabled={busy || environmentLoading}
            aria-invalid={Boolean(draftValidation?.errors.projectPath || draftValidation?.errors.wsl)}
            aria-describedby={
              draftValidation?.errors.projectPath
                ? "profile-project-path-error"
                : draftValidation?.errors.wsl
                  ? "profile-wsl-error"
                  : undefined
            }
            onChange={(e) => patchProjectLocation({ wslPath: e.currentTarget.value })}
          />
          {draftValidation?.errors.wsl && (
            <span id="profile-wsl-error" className="field-error" role="alert">
              {draftValidation.errors.wsl}
            </span>
          )}
        </label>
        <label className="field" htmlFor="profile-git-root">
          <span>Git 루트</span>
          <input
            id="profile-git-root"
            value={editing.gitRoot}
            maxLength={MAX_PROFILE_PATH_BYTES}
            disabled={busy}
            aria-invalid={Boolean(draftValidation?.errors.gitRoot)}
            aria-describedby={draftValidation?.errors.gitRoot ? "profile-git-root-error" : undefined}
            onChange={(e) => patch({ gitRoot: e.currentTarget.value })}
          />
          {draftValidation?.errors.gitRoot && (
            <span id="profile-git-root-error" className="field-error" role="alert">
              {draftValidation.errors.gitRoot}
            </span>
          )}
        </label>
        <fieldset
          className="editor-section environment-editor"
          disabled={busy}
          aria-describedby={
            draftValidation?.errors.environment ? "profile-environment-error" : "profile-environment-help"
          }
        >
          <legend>프로젝트 환경 (.env)</legend>
          <p id="profile-environment-help" className="field-help">
            프로젝트 루트의 .env 파일만 native에서 읽습니다. profile에는 파일 원문 대신 변수
            이름·source·충돌·revision·secret reference만 저장하고, 실행 직전에 다시 확인합니다.
          </p>
          <label className="checkbox-field" htmlFor="profile-environment-enabled">
            <input
              id="profile-environment-enabled"
              type="checkbox"
              checked={editing.environmentEnabled}
              disabled={environmentLoading}
              onChange={(event) => patch({ environmentEnabled: event.currentTarget.checked })}
            />
            <span>Workspace 시작 시 환경 주입 사용</span>
          </label>
          <label className="field" htmlFor="profile-environment-source">
            <span>환경 파일 이름 (프로젝트 상대)</span>
            <input
              id="profile-environment-source"
              value={editing.environmentSource}
              maxLength={MAX_ENVIRONMENT_SOURCE_BYTES}
              placeholder=".env"
              disabled={environmentLoading}
              aria-invalid={Boolean(draftValidation?.errors.environment)}
              aria-describedby={
                draftValidation?.errors.environment ? "profile-environment-error" : "profile-environment-help"
              }
              onChange={(event) => patchEnvironmentSource(event.currentTarget.value)}
            />
          </label>
          <div className="inline-actions">
            <button
              type="button"
              className="btn"
              disabled={
                environmentLoading ||
                !editing.environmentSource.trim() ||
                (!editing.windowsPath.trim() && !editing.wslPath.trim())
              }
              onClick={() => void inspectEnvironment()}
            >
              {environmentLoading ? "환경 파일 확인 중..." : "환경 파일 확인"}
            </button>
            {editing.environmentRevision && !environmentLoading ? (
              <span className="field-help" role="status" aria-live="polite">
                확인된 변수 {editing.environmentVariables.length}개 · 실행 시 변경 여부 재확인
              </span>
            ) : null}
          </div>
          {editing.environmentPreview ? (
            <div className="environment-preview" role="status" aria-live="polite">
              <strong>마스킹된 미리보기</strong>
              {editing.environmentPreview.variables.length === 0 ? (
                <p className="field-help">환경 변수가 없는 빈 파일입니다. 주입할 값이 없습니다.</p>
              ) : (
                <div className="environment-variable-list" aria-label="마스킹된 환경 변수 미리보기">
                  {editing.environmentPreview.variables.map((variable) => (
                    <div className="environment-variable-row" key={`${variable.name}-${variable.source}`}>
                      <span className="environment-variable-name">{variable.name}</span>
                      <span className="environment-variable-value" aria-label="마스킹된 환경 변수 값">
                        {variable.maskedValue || "(비어 있음)"}
                      </span>
                      <span className="environment-variable-source">{variable.source}</span>
                      {variable.secretReference ? (
                        <span className="environment-variable-secret">secret reference</span>
                      ) : null}
                      {variable.conflict !== "none" ? (
                        <span className="environment-variable-conflict">충돌: {variable.conflict}</span>
                      ) : null}
                    </div>
                  ))}
                </div>
              )}
              {editing.environmentPreview.hasConflicts ? (
                <p className="field-error" role="alert">
                  중복 또는 예약된 환경 변수 이름이 있어 주입할 수 없습니다.
                </p>
              ) : null}
            </div>
          ) : editing.environmentVariables.length > 0 ? (
            <p className="field-help">
              저장된 metadata가 있습니다. 원문 없이 다시 확인하면 마스킹된 미리보기를 표시합니다.
            </p>
          ) : null}
          {draftValidation?.errors.environment && (
            <span id="profile-environment-error" className="field-error" role="alert">
              {draftValidation.errors.environment}
            </span>
          )}
        </fieldset>
        <label className="field" htmlFor="profile-expected-ports">
          <span>예상 포트 (쉼표)</span>
          <input
            id="profile-expected-ports"
            value={editing.expectedPortsText}
            maxLength={MAX_EXPECTED_PORTS_INPUT_CHARS}
            inputMode="numeric"
            placeholder="예: 3000, 5173"
            disabled={busy}
            aria-invalid={Boolean(draftValidation?.errors.expectedPorts)}
            aria-describedby={draftValidation?.errors.expectedPorts ? "profile-ports-error" : "profile-ports-help"}
            onChange={(e) => patch({ expectedPortsText: e.currentTarget.value })}
          />
          <span id="profile-ports-help" className="field-help">
            프로필 상태 점검과 Workspace 시작 시 확인할 로컬 TCP 포트입니다.
          </span>
          {draftValidation?.errors.expectedPorts && (
            <span id="profile-ports-error" className="field-error" role="alert">
              {draftValidation.errors.expectedPorts}
            </span>
          )}
        </label>
        <fieldset className="editor-section runtime-suggestions" disabled={busy}>
          <legend>WSL 런타임 포트 제안</legend>
          <p className="field-help">
            WSL Desktop이 마지막으로 발행한 read-only snapshot만 읽습니다. WSL·Docker를 실행하거나 컨테이너를 변경하지
            않으며, 반영한 포트도 저장 전 편집 초안에만 남습니다.
          </p>
          <button
            type="button"
            className="btn"
            disabled={runtimeLoading || runtimeAccepting}
            onClick={() => void loadRuntimeSuggestions()}
          >
            {runtimeLoading ? "제안 읽는 중..." : runtimeSuggestions ? "제안 새로고침" : "제안 불러오기"}
          </button>
          {runtimeSuggestions ? (
            <div className="runtime-suggestion-result">
              <div
                className={`runtime-suggestion-status status-${runtimeSuggestions.status}`}
                role="status"
                aria-live="polite"
              >
                <strong>{RUNTIME_STATUS_LABEL[runtimeSuggestions.status]}</strong>
                {runtimeSuggestions.producerVersion && runtimeSuggestions.freshnessMs !== null ? (
                  <span>
                    {runtimeSuggestions.source} · producer {runtimeSuggestions.producerVersion} ·{" "}
                    {formatRuntimeFreshness(runtimeSuggestions.freshnessMs)}
                  </span>
                ) : (
                  <span>{runtimeSuggestions.source}</span>
                )}
              </div>
              {runtimeSuggestions.ports.length > 0 ? (
                <div className="runtime-port-list" aria-label="WSL runtime 포트 후보">
                  {runtimeSuggestions.ports.map((port) => {
                    const alreadyRegistered = existingRuntimePorts.has(port.published);
                    const selected = selectedRuntimePorts.has(port.published);
                    return (
                      <label className="runtime-port-row" key={port.published}>
                        <input
                          type="checkbox"
                          checked={alreadyRegistered || selected}
                          disabled={alreadyRegistered || !runtimeActionable || runtimeLoading || runtimeAccepting}
                          onChange={(event) => {
                            const checked = event.currentTarget.checked;
                            setSelectedRuntimePorts((previous) => {
                              const next = new Set(previous);
                              if (checked) next.add(port.published);
                              else next.delete(port.published);
                              return next;
                            });
                          }}
                          aria-label={`게시된 포트 ${port.published} 선택`}
                        />
                        <span className="runtime-port-number">host {port.published}</span>
                        {alreadyRegistered ? <span className="runtime-port-existing">이미 등록됨</span> : null}
                        <ul>
                          {port.sources.map((source) => (
                            <li
                              key={`${source.distro}\u0000${source.container}\u0000${source.target}\u0000${source.protocol}`}
                            >
                              {source.distro} · {source.container} ({source.containerState}) · target {source.target}/
                              {source.protocol}
                            </li>
                          ))}
                        </ul>
                      </label>
                    );
                  })}
                </div>
              ) : runtimeActionable ? (
                <p className="field-help">발행된 host 포트 후보가 없습니다.</p>
              ) : null}
              <button
                type="button"
                className="btn primary"
                disabled={!runtimeActionable || runtimeLoading || runtimeAccepting || selectedRuntimePorts.size === 0}
                onClick={() => void acceptRuntimePorts()}
              >
                {runtimeAccepting ? "상태 재확인 중..." : "선택 포트를 초안에 반영"}
              </button>
            </div>
          ) : null}
        </fieldset>
        <fieldset
          className="editor-section"
          disabled={busy}
          aria-describedby={draftValidation?.errors.services ? "profile-services-error" : undefined}
        >
          <legend>Run Manager 서비스</legend>
          <p className="field-help">연결할 서비스 ID를 등록합니다. 이 화면은 서비스 자체를 시작·수정하지 않습니다.</p>
          {editing.serviceRows.map((row, index) => (
            <div className="editable-list-row" key={row.key}>
              <label className="sr-only" htmlFor={`service-${row.key}`}>
                서비스 {index + 1}
              </label>
              <input
                id={`service-${row.key}`}
                value={row.value}
                maxLength={MAX_SERVICE_ID_CHARS}
                placeholder="예: devbox-dev"
                aria-invalid={Boolean(draftValidation?.errors.serviceRows[row.key])}
                aria-describedby={draftValidation?.errors.serviceRows[row.key] ? `service-error-${row.key}` : undefined}
                onChange={(e) =>
                  patch({
                    serviceRows: editing.serviceRows.map((candidate) =>
                      candidate.key === row.key ? { ...candidate, value: e.currentTarget.value } : candidate,
                    ),
                  })
                }
              />
              <button
                type="button"
                className="btn"
                onClick={() =>
                  patch({ serviceRows: editing.serviceRows.filter((candidate) => candidate.key !== row.key) })
                }
                aria-label={`서비스 ${index + 1} 삭제`}
              >
                삭제
              </button>
              {draftValidation?.errors.serviceRows[row.key] && (
                <span id={`service-error-${row.key}`} className="field-error" role="alert">
                  {draftValidation.errors.serviceRows[row.key]}
                </span>
              )}
            </div>
          ))}
          {draftValidation?.errors.services && (
            <span id="profile-services-error" className="field-error" role="alert">
              {draftValidation.errors.services}
            </span>
          )}
          <button
            type="button"
            className="btn"
            disabled={editing.serviceRows.length >= MAX_SERVICES}
            onClick={() => patch({ serviceRows: [...editing.serviceRows, newServiceDraftRow()] })}
          >
            + 서비스 추가
          </button>
        </fieldset>
        {draftValidation?.errors.projectPath && (
          <div id="profile-project-path-error" className="field-error form-error" role="alert">
            {draftValidation.errors.projectPath}
          </div>
        )}
        {draftValidation?.errors.id && (
          <div className="field-error form-error" role="alert">
            {draftValidation.errors.id}
          </div>
        )}
        <div className="actions">
          <button
            type="submit"
            className="btn primary"
            disabled={busy || environmentLoading || !draftValidation?.profile}
          >
            저장
          </button>
          <button
            type="button"
            className="btn"
            disabled={busy}
            onClick={() => {
              onCancelPreflight();
              closeEditor();
            }}
          >
            취소
          </button>
        </div>
      </form>
    </section>
  );
}
