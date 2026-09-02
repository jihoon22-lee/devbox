import type { ContainerInfo, DashboardDistroSnapshot, DistroInfo } from "../types";
import { compactDockerPorts, dockerDisplayState } from "../lib/dockerDisplay";
import { resourceSummaryLabel, type DashboardFreshness } from "../lib/resourceDisplay";

interface Props {
  distros: DistroInfo[];
  selectedDistro: string;
  onSelectDistro: (name: string) => void;
  onOpenTerminal: (name: string) => void;
  onOpenJournalInLogLens?: (name: string) => void;
  onOpenFileInLogLens?: (name: string) => void;
  containers: ContainerInfo[];
  dockerMissing: boolean;
  busy: string | null;
  logLensBusy?: string | null;
  onAction: (id: string, action: "start" | "stop" | "restart") => void;
  onRefresh: () => void;
  /** Complete resource/session generation shared with broadcast safety. */
  dashboardDistros?: DashboardDistroSnapshot[];
  snapshotState?: DashboardFreshness;
  /** Whether the last good snapshot still backs a state change. Owned by App. */
  snapshotActionable?: boolean;
  /** The toolbar already owns a distro selector; the panel repeats it only when asked. */
  showDistroSelect?: boolean;
}

const DISTRO_STATE_LABELS: Readonly<Record<string, string>> = {
  Running: "실행 중",
  Stopped: "중지됨",
  Installing: "설치 중",
  Uninstalling: "제거 중",
};

function distroStateLabel(state: string): string {
  return DISTRO_STATE_LABELS[state] ?? "상태 알 수 없음";
}

export default function DistroPanel({
  distros,
  selectedDistro,
  onSelectDistro,
  onOpenTerminal,
  onOpenJournalInLogLens,
  onOpenFileInLogLens,
  containers,
  dockerMissing,
  busy,
  logLensBusy = null,
  onAction,
  onRefresh,
  dashboardDistros = [],
  snapshotState = "loading",
  snapshotActionable = false,
  showDistroSelect = true,
}: Props) {
  const running = containers.filter((c) => dockerDisplayState(c.status).running).length;
  // A collection being in flight does not close the Docker controls; an expired or failed
  // one does. App owns that rule so the panel and broadcast never disagree.
  const dockerActionsEnabled = snapshotActionable;
  const snapshotByDistro = new Map(dashboardDistros.map((distro) => [distro.name, distro]));
  const selectedSnapshot = snapshotByDistro.get(selectedDistro);
  const dockerError = selectedSnapshot?.dockerAvailability === "error";
  const dockerNotQueried = selectedSnapshot?.dockerAvailability === "notQueried";
  const snapshotLabel = {
    loading: "조회 중…",
    refreshing: "새로 고치는 중…",
    fresh: "최신 snapshot",
    stale: "오래된 snapshot",
    error: "마지막 정상 snapshot",
  }[snapshotState];
  const anyBusy = busy !== null || logLensBusy !== null;

  return (
    <div className="dash-section">
      <div className="dash-header">
        <span>WSL</span>
        <span className={`snapshot-state snapshot-state-${snapshotState}`} role="status">
          {snapshotLabel}
        </span>
        <button
          className="btn refresh"
          type="button"
          disabled={snapshotState === "loading" || snapshotState === "refreshing" || anyBusy}
          onClick={onRefresh}
        >
          새로고침
        </button>
      </div>

      {showDistroSelect && (
        <select
          aria-label="WSL 배포판 선택"
          disabled={logLensBusy !== null}
          value={selectedDistro}
          onChange={(e) => onSelectDistro(e.currentTarget.value)}
        >
          {distros.map((d) => (
            <option key={d.name} value={d.name}>
              {d.name} {d.default ? "(기본)" : ""}
            </option>
          ))}
        </select>
      )}

      {dockerMissing && (
        <div className="banner">
          WSL에 Docker가 설치되어 있지 않습니다. <code>sudo apt install docker.io</code> 또는 Docker
          Desktop을 설치하세요.
        </div>
      )}
      {dockerError && <div className="banner">선택한 WSL 배포판의 Docker 상태를 읽지 못했습니다. 다음 snapshot에서 다시 시도하세요.</div>}
      {dockerNotQueried && <div className="banner">중지된 WSL 배포판에서는 Docker를 조회하지 않습니다.</div>}

      <div className="cards">
        {distros.map((d) => {
          const snapshot = snapshotByDistro.get(d.name);
          return (
            <div key={d.name} className={`card ${d.default ? "card-default" : ""}`}>
              <div className="card-title">{d.name}</div>
              <div className="card-row">
                <span>버전</span>
                <span>{d.version}</span>
              </div>
              <div className="card-row">
                <span>상태</span>
                <span className={d.state.toLowerCase() === "running" ? "status-on" : "status-off"}>
                  ● {distroStateLabel(d.state)}
                </span>
              </div>
              <div className="card-row">
                <span>활성 터미널</span>
                <span>{snapshot ? snapshot.terminalCount : "—"}</span>
              </div>
              <div
                className="resource-summary"
                role="group"
                aria-label={`${d.name} resource summary`}
              >
                {resourceSummaryLabel(snapshot?.resource)}
              </div>
              <button
                className="btn"
                type="button"
                aria-label={`${d.name} 터미널 열기`}
                disabled={logLensBusy !== null}
                onClick={() => onOpenTerminal(d.name)}
              >
                터미널 열기
              </button>
              {onOpenJournalInLogLens && (
                <button
                  type="button"
                  className="btn"
                  disabled={anyBusy}
                  aria-busy={logLensBusy === `log-lens-journal:${d.name}`}
                  onClick={() => onOpenJournalInLogLens(d.name)}
                >
                  Log Lens에서 저널 열기
                </button>
              )}
              {onOpenFileInLogLens && (
                <button
                  type="button"
                  className="btn"
                  disabled={anyBusy}
                  aria-busy={logLensBusy === `log-lens-file:${d.name}`}
                  onClick={() => onOpenFileInLogLens(d.name)}
                >
                  Log Lens에서 파일 열기
                </button>
              )}
            </div>
          );
        })}
      </div>

      <h2 className="dash-subtitle">Docker ({running}/{containers.length}개 실행 중)</h2>
      {!dockerMissing && !dockerError && !dockerNotQueried && (
        <div className="docker-list" aria-label="Docker 컨테이너">
          {containers.map((c) => {
            const state = dockerDisplayState(c.status);
            const canStart = state.key === "exited" || state.key === "created";
            return (
              <details key={c.id} className="docker-container">
                <summary className="docker-container-summary">
                  <span className="docker-container-name" title={c.name}>
                    {c.name}
                  </span>
                  <span className={`docker-state docker-state-${state.key}`}>{state.label}</span>
                  <span className="docker-port-summary mono" title={c.ports || "포트 없음"}>
                    {compactDockerPorts(c.ports)}
                  </span>
                  <span className="docker-detail-chevron" aria-hidden="true">
                    ▸
                  </span>
                </summary>

                <div className="docker-container-detail">
                  <dl>
                    <div>
                      <dt>컨테이너 ID</dt>
                      <dd className="mono">{c.id}</dd>
                    </div>
                    <div>
                      <dt>이미지</dt>
                      <dd className="mono">{c.image || "(비어 있음)"}</dd>
                    </div>
                    <div>
                      <dt>원본 상태</dt>
                      <dd>{c.status || "(비어 있음)"}</dd>
                    </div>
                    <div>
                      <dt>원본 포트</dt>
                      <dd className="mono">{c.ports || "(비어 있음)"}</dd>
                    </div>
                  </dl>

                  <div className="docker-actions">
                    {canStart ? (
                      <button
                        className="btn"
                        type="button"
                        disabled={anyBusy || !dockerActionsEnabled}
                        onClick={() => onAction(c.id, "start")}
                      >
                        시작
                      </button>
                    ) : (
                      <>
                        <button
                          className="btn danger"
                          type="button"
                          disabled={anyBusy || !dockerActionsEnabled}
                          onClick={() => onAction(c.id, "stop")}
                        >
                          중지
                        </button>
                        <button
                          className="btn"
                          type="button"
                          disabled={anyBusy || !dockerActionsEnabled}
                          onClick={() => onAction(c.id, "restart")}
                        >
                          재시작
                        </button>
                      </>
                    )}
                  </div>
                </div>
              </details>
            );
          })}
          {containers.length === 0 && <div className="docker-empty">컨테이너 없음</div>}
        </div>
      )}
    </div>
  );
}
