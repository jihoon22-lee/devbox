import type { ContainerInfo, DashboardDistroSnapshot, DistroInfo } from "../types";
import { compactDockerPorts, dockerDisplayState } from "../lib/dockerDisplay";
import { resourceSummaryLabel, type DashboardFreshness } from "../lib/resourceDisplay";

interface Props {
  distros: DistroInfo[];
  selectedDistro: string;
  onSelectDistro: (name: string) => void;
  onOpenTerminal: (name: string) => void;
  containers: ContainerInfo[];
  dockerMissing: boolean;
  busy: string | null;
  onAction: (id: string, action: "start" | "stop" | "restart") => void;
  onRefresh: () => void;
  /** Complete resource/session generation shared with broadcast safety. */
  dashboardDistros?: DashboardDistroSnapshot[];
  snapshotState?: DashboardFreshness;
}

export default function DistroPanel({
  distros,
  selectedDistro,
  onSelectDistro,
  onOpenTerminal,
  containers,
  dockerMissing,
  busy,
  onAction,
  onRefresh,
  dashboardDistros = [],
  snapshotState = "loading",
}: Props) {
  const running = containers.filter((c) => dockerDisplayState(c.status).running).length;
  const snapshotFresh = snapshotState === "fresh";
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
          disabled={snapshotState === "loading" || snapshotState === "refreshing" || busy !== null}
          onClick={onRefresh}
        >
          Refresh
        </button>
      </div>

      <select
        aria-label="WSL distro 선택"
        value={selectedDistro}
        onChange={(e) => onSelectDistro(e.currentTarget.value)}
      >
        {distros.map((d) => (
          <option key={d.name} value={d.name}>
            {d.name} {d.default ? "(default)" : ""}
          </option>
        ))}
      </select>

      {dockerMissing && (
        <div className="banner">
          WSL에 Docker가 설치되어 있지 않습니다. <code>sudo apt install docker.io</code> 또는 Docker
          Desktop을 설치하세요.
        </div>
      )}
      {dockerError && <div className="banner">선택한 WSL distro의 Docker 상태를 읽지 못했습니다. 다음 snapshot에서 다시 시도하세요.</div>}
      {dockerNotQueried && <div className="banner">중지된 WSL distro에서는 Docker를 조회하지 않습니다.</div>}

      <div className="cards">
        {distros.map((d) => {
          const snapshot = snapshotByDistro.get(d.name);
          return (
            <div key={d.name} className={`card ${d.default ? "card-default" : ""}`}>
              <div className="card-title">{d.name}</div>
              <div className="card-row">
                <span>Version</span>
                <span>{d.version}</span>
              </div>
              <div className="card-row">
                <span>Status</span>
                <span className={d.state.toLowerCase() === "running" ? "status-on" : "status-off"}>
                  ● {d.state}
                </span>
              </div>
              <div className="card-row">
                <span>Active terminals</span>
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
                onClick={() => onOpenTerminal(d.name)}
              >
                Open Terminal
              </button>
            </div>
          );
        })}
      </div>

      <h3 className="dash-subtitle">Docker ({running}/{containers.length} running)</h3>
      {!dockerMissing && !dockerError && !dockerNotQueried && (
        <div className="docker-list" aria-label="Docker containers">
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
                  <span className="docker-port-summary mono" title={c.ports || "No ports"}>
                    {compactDockerPorts(c.ports)}
                  </span>
                  <span className="docker-detail-chevron" aria-hidden="true">
                    ▸
                  </span>
                </summary>

                <div className="docker-container-detail">
                  <dl>
                    <div>
                      <dt>Container ID</dt>
                      <dd className="mono">{c.id}</dd>
                    </div>
                    <div>
                      <dt>Image</dt>
                      <dd className="mono">{c.image || "(empty)"}</dd>
                    </div>
                    <div>
                      <dt>Original status</dt>
                      <dd>{c.status || "(empty)"}</dd>
                    </div>
                    <div>
                      <dt>Original ports</dt>
                      <dd className="mono">{c.ports || "(empty)"}</dd>
                    </div>
                  </dl>

                  <div className="docker-actions">
                    {canStart ? (
                      <button
                        className="btn"
                        type="button"
                        disabled={busy === `${c.id}:start` || !snapshotFresh}
                        onClick={() => onAction(c.id, "start")}
                      >
                        Start
                      </button>
                    ) : (
                      <>
                        <button
                          className="btn danger"
                          type="button"
                          disabled={busy === `${c.id}:stop` || !snapshotFresh}
                          onClick={() => onAction(c.id, "stop")}
                        >
                          Stop
                        </button>
                        <button
                          className="btn"
                          type="button"
                          disabled={busy === `${c.id}:restart` || !snapshotFresh}
                          onClick={() => onAction(c.id, "restart")}
                        >
                          Restart
                        </button>
                      </>
                    )}
                  </div>
                </div>
              </details>
            );
          })}
          {containers.length === 0 && <div className="docker-empty">No containers</div>}
        </div>
      )}
    </div>
  );
}
