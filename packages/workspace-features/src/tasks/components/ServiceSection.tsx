import { targetLabel, restartLabel, serviceStateLabel } from "../lib/taskPresentation";
import { isKeyboardActivation } from "@devbox/a11y";
import type * as React from "react";

interface Props {
  openServiceCreate: () => void;
  onExportDefs: () => Promise<void>;
  importTriggerRef: React.RefObject<HTMLButtonElement | null>;
  setImportOpen: React.Dispatch<React.SetStateAction<boolean>>;
  loading: boolean;
  services: import("../types").Job[];
  serviceInstances: Record<string, import("../types").ServiceInstance>;
  obsMap: Record<string, import("../api").ServiceObservability | null>;
  selectedServiceId: string | null;
  setSelectedServiceId: React.Dispatch<React.SetStateAction<string | null>>;
  serviceContextTrigger: import("../../../../context-menu/src/useContextMenu").ContextMenuTriggerProps;
  busy: boolean;
  handleServiceRestart: (service: import("../types").Job) => Promise<void>;
  handleServiceStop: (service: import("../types").Job) => Promise<void>;
  handleServiceStart: (service: import("../types").Job) => Promise<void>;
  onToggleObs: (id: string) => Promise<void>;
  obsOpen: Record<string, boolean>;
  openServiceEdit: (service: import("../types").Job) => void;
  handleServiceDelete: (service: import("../types").Job) => Promise<void>;
  fmtUptime: (startedAt: number | null) => string;
}

export function ServiceSection({
  openServiceCreate,
  onExportDefs,
  importTriggerRef,
  setImportOpen,
  loading,
  services,
  serviceInstances,
  obsMap,
  selectedServiceId,
  setSelectedServiceId,
  serviceContextTrigger,
  busy,
  handleServiceRestart,
  handleServiceStop,
  handleServiceStart,
  onToggleObs,
  obsOpen,
  openServiceEdit,
  handleServiceDelete,
  fmtUptime,
}: Props) {
  return (
    <section className="jobs-section" aria-labelledby="services-title">
      <div className="section-toolbar">
        <div>
          <p className="subtitle">서비스 정의와 자동 시작·재시작·로컬 헬스체크 정책을 관리합니다.</p>
          <h3 id="services-title" className="visually-hidden">
            서비스 목록
          </h3>
        </div>
        <button type="button" className="button-primary" onClick={openServiceCreate}>
          + 새 서비스
        </button>
        <button type="button" className="button-secondary" onClick={() => void onExportDefs()}>
          정의 내보내기
        </button>
        <button ref={importTriggerRef} type="button" className="button-secondary" onClick={() => setImportOpen(true)}>
          정의 가져오기
        </button>
      </div>
      {loading ? (
        <div className="empty-card compact">
          <div className="pulse" />
          <p>서비스를 불러오는 중…</p>
        </div>
      ) : null}
      {!loading && services.length === 0 ? (
        <section className="empty-card" aria-labelledby="empty-service-title">
          <div className="pulse" aria-hidden="true" />
          <h3 id="empty-service-title">등록된 서비스가 아직 없습니다</h3>
          <p>계속 실행할 명령을 서비스로 저장하고 자동 시작·재시작 정책을 준비할 수 있습니다.</p>
          <button type="button" className="button-primary" onClick={openServiceCreate}>
            첫 서비스 만들기
          </button>
        </section>
      ) : null}
      {!loading && services.length > 0 ? (
        <div className="job-list service-list">
          {services.map((service) => {
            const instance = serviceInstances[service.id];
            const state = instance?.state ?? null;
            const canStart = state === "stopped";
            const canControl = state !== null && ["starting", "running", "retry_waiting"].includes(state);
            const ready = state === "running" || state === "starting";
            const obs = obsMap[service.id];
            return (
              <article
                className={`job-card service-card ${selectedServiceId === service.id ? "selected" : ""}`}
                key={service.id}
                tabIndex={0}
                aria-current={selectedServiceId === service.id ? "true" : undefined}
                data-service-id={service.id}
                onClick={() => setSelectedServiceId(service.id)}
                onContextMenu={serviceContextTrigger.onContextMenu}
                onKeyDown={(event) => {
                  serviceContextTrigger.onKeyDown?.(event);
                  if (event.defaultPrevented || event.target !== event.currentTarget || !isKeyboardActivation(event))
                    return;
                  event.preventDefault();
                  setSelectedServiceId(service.id);
                }}
              >
                <div className="job-card-main">
                  <div className="job-title-row">
                    <h3>{service.name}</h3>
                    <span className={`job-state ${ready ? "ready" : "disabled"}`}>
                      {state ? serviceStateLabel(state) : "상태 확인 불가"}
                    </span>
                  </div>
                  <code title={service.command}>{service.command}</code>
                  <div className="job-meta">
                    <span>{targetLabel(service)}</span>
                    <span>{restartLabel(service)}</span>
                    <span>{service.autoStart ? "자동 시작" : "수동 시작"}</span>
                    {service.healthTcpAddress && service.healthTcpPort ? (
                      <span>
                        TCP {service.healthTcpAddress}:{service.healthTcpPort}
                      </span>
                    ) : (
                      <span>TCP probe 없음</span>
                    )}
                    {instance && instance.consecutiveFailures > 0 ? (
                      <span>연속 실패 {instance.consecutiveFailures}회</span>
                    ) : null}
                    {service.envConfigured ? <span className="secret-badge">환경변수 보호됨</span> : null}
                  </div>
                </div>
                <div className="job-actions">
                  {canControl ? (
                    <>
                      <button
                        type="button"
                        className="button-secondary"
                        disabled={busy}
                        onClick={() => void handleServiceRestart(service)}
                      >
                        재시작
                      </button>
                      <button
                        type="button"
                        className="button-danger"
                        disabled={busy}
                        onClick={() => void handleServiceStop(service)}
                      >
                        정지
                      </button>
                    </>
                  ) : (
                    <button
                      type="button"
                      className="button-secondary"
                      disabled={busy || !canStart}
                      onClick={() => void handleServiceStart(service)}
                    >
                      시작
                    </button>
                  )}
                  <button type="button" className="button-secondary" onClick={() => void onToggleObs(service.id)}>
                    {obsOpen[service.id] ? "상세 닫기" : "상세"}
                  </button>
                  <button type="button" className="button-secondary" onClick={() => openServiceEdit(service)}>
                    편집
                  </button>
                  <button
                    type="button"
                    className="button-danger"
                    disabled={busy || state !== "stopped"}
                    onClick={() => void handleServiceDelete(service)}
                  >
                    삭제
                  </button>
                </div>
                {obsOpen[service.id] && obs && (
                  <div className="obs-panel">
                    <div className="obs-row">
                      <span className="obs-label">정의</span>
                      <span>
                        {obs.definition.enabled ? "활성" : "비활성"} ·{" "}
                        {obs.definition.autoStart ? "자동 시작" : "수동 시작"}
                      </span>
                    </div>
                    <div className="obs-row">
                      <span className="obs-label">인스턴스 (DB 상태)</span>
                      <span>
                        {obs.instance ? serviceStateLabel(obs.instance.state) : "없음"} · 재시작 {obs.restartCount}회
                      </span>
                    </div>
                    {obs.current && (
                      <div className="obs-row">
                        <span className="obs-label">현재 실행</span>
                        <span>
                          {obs.current.status} · {fmtUptime(obs.current.startedAt)}
                          {obs.currentPid != null && ` · PID ${obs.currentPid} (DB 기록)`}
                        </span>
                      </div>
                    )}
                    {obs.nextRetryAt != null && (
                      <div className="obs-row">
                        <span className="obs-label">다음 재시도</span>
                        <span>{new Date(obs.nextRetryAt).toLocaleTimeString()}</span>
                      </div>
                    )}
                    {obs.recent.length > 0 && (
                      <div className="obs-row">
                        <span className="obs-label">최근 실행</span>
                        <span className="obs-recent">
                          {obs.recent.slice(0, 5).map((r) => (
                            <span key={r.id} className={`obs-run ${r.status === "failed" ? "obs-fail" : ""}`}>
                              {r.status}
                              {r.exitCode != null ? `(${r.exitCode})` : ""}
                            </span>
                          ))}
                        </span>
                      </div>
                    )}
                    <div className="obs-note">
                      인스턴스 상태는 DB 기록 기준입니다. PID는 실제 프로세스 생존과 다를 수 있습니다.
                    </div>
                  </div>
                )}
              </article>
            );
          })}
        </div>
      ) : null}
    </section>
  );
}
