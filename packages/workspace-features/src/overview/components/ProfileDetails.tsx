import { PREFLIGHT_ITEM_LABEL, PREFLIGHT_STATUS_LABEL, RESOURCE_STATE_LABEL } from "../lib/profilePresentation";
import { dependencyHealth } from "../api";
import PackageDependencySummaryPanel from "./PackageDependencySummaryPanel";
import type * as React from "react";

interface Props {
  selectedProfile: import("../api").ProjectProfile;
  busy: boolean;
  run: import("../api").WorkspaceRun | null;
  preflight: import("../api").WorkspacePreflight | null;
  onStart: (profileId: string) => Promise<void>;
  startingProfileId: string | null;
  startCancelRequested: boolean;
  onCancelStart: (profileId: string) => Promise<void>;
  retrying: boolean;
  onRetry: (profile: import("../api").ProjectProfile) => Promise<void>;
  onStop: (profile: import("../api").ProjectProfile) => Promise<void>;
  preflightLoading: boolean;
  preflightTarget: React.RefObject<string | null>;
  onCancelPreflight: () => void;
  onContinueStart: () => Promise<void>;
  health: import("../api").ProjectHealth | null;
  dependencyLoading: boolean;
  dependencyRequest: React.RefObject<number>;
  setDependencyStatus: React.Dispatch<React.SetStateAction<import("../api").WorkspacePreflight | null>>;
  setDependencyLoading: React.Dispatch<React.SetStateAction<boolean>>;
  setError: React.Dispatch<React.SetStateAction<string | null>>;
  dependencyStatus: import("../api").WorkspacePreflight | null;
}

export function ProfileDetails({
  selectedProfile,
  busy,
  run,
  preflight,
  onStart,
  startingProfileId,
  startCancelRequested,
  onCancelStart,
  retrying,
  onRetry,
  onStop,
  preflightLoading,
  preflightTarget,
  onCancelPreflight,
  onContinueStart,
  health,
  dependencyLoading,
  dependencyRequest,
  setDependencyStatus,
  setDependencyLoading,
  setError,
  dependencyStatus,
}: Props) {
  return (
    <section className="panel">
      <h2>{selectedProfile.name}</h2>
      <div className="row-actions">
        <button
          className="btn primary"
          disabled={busy || run !== null || preflight?.profileId === selectedProfile.id}
          onClick={() => void onStart(selectedProfile.id)}
        >
          {startingProfileId === selectedProfile.id ? "Workspace 시작 중…" : "Workspace 시작"}
        </button>
        {startingProfileId === selectedProfile.id && (
          <button
            className="btn danger"
            disabled={!busy || startCancelRequested}
            onClick={() => void onCancelStart(selectedProfile.id)}
          >
            {startCancelRequested ? "취소 요청 중…" : "시작 취소"}
          </button>
        )}
        {run?.profileId === selectedProfile.id && (
          <>
            {run.canRetry === true ? (
              <button className="btn" disabled={busy || retrying} onClick={() => void onRetry(selectedProfile)}>
                {retrying ? "실패 단계 재시도 중…" : "실패 단계부터 다시 시도"}
              </button>
            ) : null}
            <button className="btn danger" disabled={busy} onClick={() => void onStop(selectedProfile)}>
              내가 시작한 작업 중지
            </button>
          </>
        )}
      </div>

      {preflightLoading && preflightTarget.current === selectedProfile.id && (
        <div className="preflight-dialog" role="status" aria-live="polite" aria-busy="true">
          <h3>Workspace 시작 사전 점검 중…</h3>
          <p className="field-help">
            설치된 앱, WSL 경로, 예상 포트와 서비스 dependency를 읽기 전용으로 확인하고 있습니다.
          </p>
          <div className="actions">
            <button type="button" className="btn" onClick={onCancelPreflight}>
              취소
            </button>
          </div>
        </div>
      )}

      {preflight?.profileId === selectedProfile.id && (
        <div
          className={`preflight-dialog ${preflight.ready ? "ready" : "blocked"}`}
          role="dialog"
          aria-modal="true"
          aria-labelledby="workspace-preflight-title"
          aria-describedby="workspace-preflight-description"
          onKeyDown={(event) => {
            if (event.key === "Tab") {
              const focusable = Array.from(
                event.currentTarget.querySelectorAll<HTMLElement>(
                  "button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])",
                ),
              );
              const first = focusable[0];
              const last = focusable[focusable.length - 1];
              if (first && last) {
                if (event.shiftKey && document.activeElement === first) {
                  event.preventDefault();
                  last.focus();
                } else if (!event.shiftKey && document.activeElement === last) {
                  event.preventDefault();
                  first.focus();
                }
              }
            }
            if (event.key === "Escape" && !event.nativeEvent.isComposing && !busy) {
              event.preventDefault();
              onCancelPreflight();
            }
          }}
        >
          <h3 id="workspace-preflight-title">Workspace 시작 사전 점검</h3>
          <p id="workspace-preflight-description" className="field-help">
            실행 전에 읽기 전용으로 확인한 결과입니다. 경고는 기존 resource를 유지한 채 계속할 수 있고, 차단·확인 불가
            항목이 있으면 어떤 앱도 시작하지 않습니다.
          </p>
          <div className="preflight-list" aria-label="Workspace 사전 점검 결과">
            {preflight.items.map((item) => (
              <div className={`preflight-row status-${item.status}`} key={item.key}>
                <div className="preflight-row-heading">
                  <strong>{PREFLIGHT_ITEM_LABEL[item.key] ?? item.key}</strong>
                  <span className="preflight-status">{PREFLIGHT_STATUS_LABEL[item.status]}</span>
                </div>
                <span className="preflight-detail">{item.detail}</span>
                {item.resources.length > 0 && (
                  <ul className="preflight-resources">
                    {item.resources.map((resource) => (
                      <li key={`${resource.kind}:${resource.id}`}>
                        <span>{resource.id} · </span>
                        <span className="resource-state">{RESOURCE_STATE_LABEL[resource.state]}</span>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            ))}
          </div>
          {!preflight.ready && (
            <div className="field-error form-error" role="alert">
              차단된 항목을 해결한 뒤 사전 점검을 다시 실행하세요.
            </div>
          )}
          <div className="actions">
            <button
              type="button"
              className="btn primary"
              autoFocus={preflight.ready}
              disabled={!preflight.ready || busy}
              onClick={() => void onContinueStart()}
            >
              {busy ? "Workspace 시작 중…" : "계속 시작"}
            </button>
            <button
              type="button"
              className="btn"
              autoFocus={!preflight.ready}
              disabled={busy}
              onClick={onCancelPreflight}
            >
              취소
            </button>
          </div>
        </div>
      )}

      <h3 className="subtitle">상태 점검</h3>
      {health?.items.map((item) => (
        <div key={item.name} className={`health-row ${item.ok ? "ok" : "bad"}`}>
          <span className="health-name">{item.name}</span>
          <span className="health-detail">{item.detail}</span>
        </div>
      ))}

      <h3 className="subtitle dependencies-title">의존성</h3>
      <div className="dependency-health-heading">
        <h4 className="dependency-subtitle">환경</h4>
        <button
          type="button"
          className="btn"
          disabled={busy || dependencyLoading}
          onClick={() => {
            dependencyRequest.current += 1;
            setDependencyStatus(null);
            setDependencyLoading(true);
            const request = dependencyRequest.current;
            void dependencyHealth(selectedProfile.id)
              .then((result) => {
                if (request === dependencyRequest.current && result.profileId === selectedProfile.id)
                  setDependencyStatus(result);
              })
              .catch(() => {
                if (request === dependencyRequest.current) setError("의존성 상태를 확인할 수 없습니다.");
              })
              .finally(() => {
                if (request === dependencyRequest.current) setDependencyLoading(false);
              });
          }}
        >
          {dependencyLoading ? "확인 중…" : "의존성 새로고침"}
        </button>
      </div>
      {dependencyLoading && !dependencyStatus ? (
        <div className="dim" role="status">
          앱/배포판/path/port/service dependency 확인 중…
        </div>
      ) : dependencyStatus ? (
        <div className="dependency-health-list" aria-label="Dependency health 결과">
          {dependencyStatus.items.map((item) => (
            <div className={`preflight-row status-${item.status}`} key={item.key}>
              <div className="preflight-row-heading">
                <strong>{PREFLIGHT_ITEM_LABEL[item.key] ?? item.key}</strong>
                <span className="preflight-status">{PREFLIGHT_STATUS_LABEL[item.status]}</span>
              </div>
              <span className="preflight-detail">{item.detail}</span>
              {item.resources.length > 0 && (
                <ul className="preflight-resources">
                  {item.resources.map((resource) => (
                    <li key={`${resource.kind}:${resource.id}`}>
                      <span>{resource.id} · </span>
                      <span className="resource-state">{RESOURCE_STATE_LABEL[resource.state]}</span>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          ))}
        </div>
      ) : (
        <div className="dim">의존성 상태를 확인할 수 없습니다.</div>
      )}

      <PackageDependencySummaryPanel profileId={selectedProfile.id} />

      {run?.profileId === selectedProfile.id && (
        <>
          <h3 className="subtitle">Workspace 시작 결과</h3>
          {run.steps.map((step, i) => (
            <div key={i} className={`health-row ${step.ok ? "ok" : "bad"}`}>
              <span className="health-name">{step.name}</span>
              <span className="health-detail">
                <span>{PREFLIGHT_STATUS_LABEL[step.status]}</span>
                <span aria-hidden="true"> · </span>
                <span>{step.detail}</span>
              </span>
            </div>
          ))}
          {run.resourceProvenance.length > 0 && (
            <>
              <h3 className="subtitle">리소스 소유권</h3>
              {run.resourceProvenance.map((resource) => (
                <div key={`${resource.kind}:${resource.id}`} className="health-row">
                  <span className="health-name">{resource.id}</span>
                  <span className="health-detail">{RESOURCE_STATE_LABEL[resource.state]}</span>
                </div>
              ))}
            </>
          )}
        </>
      )}
    </section>
  );
}
