import type { ContainerInfo, DistroInfo } from "../types";
import { compactDockerPorts, dockerDisplayState } from "../lib/dockerDisplay";

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
}: Props) {
  const running = containers.filter((c) => dockerDisplayState(c.status).running).length;

  return (
    <div className="dash-section">
      <div className="dash-header">
        <span>WSL</span>
        <button className="btn refresh" onClick={onRefresh}>
          Refresh
        </button>
      </div>

      <select value={selectedDistro} onChange={(e) => onSelectDistro(e.currentTarget.value)}>
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

      <div className="cards">
        {distros.map((d) => (
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
            <button className="btn" onClick={() => onOpenTerminal(d.name)}>
              Open Terminal
            </button>
          </div>
        ))}
      </div>

      <h3 className="dash-subtitle">Docker ({running}/{containers.length} running)</h3>
      {!dockerMissing && (
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
                        disabled={busy === `${c.id}:start`}
                        onClick={() => onAction(c.id, "start")}
                      >
                        Start
                      </button>
                    ) : (
                      <>
                        <button
                          className="btn danger"
                          disabled={busy === `${c.id}:stop`}
                          onClick={() => onAction(c.id, "stop")}
                        >
                          Stop
                        </button>
                        <button
                          className="btn"
                          disabled={busy === `${c.id}:restart`}
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
