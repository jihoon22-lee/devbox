import type { ContainerInfo, DistroInfo } from "../types";

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
  const running = containers.filter((c) => c.status.startsWith("Up")).length;

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
              <span className="status-on">● Running</span>
            </div>
            <button className="btn" onClick={() => onOpenTerminal(d.name)}>
              Open Terminal
            </button>
          </div>
        ))}
      </div>

      <h3 className="dash-subtitle">Docker ({running}/{containers.length} running)</h3>
      {!dockerMissing && (
        <table>
          <thead>
            <tr>
              <th>NAME</th>
              <th>IMAGE</th>
              <th>STATUS</th>
              <th>PORTS</th>
              <th>ACTION</th>
            </tr>
          </thead>
          <tbody>
            {containers.map((c) => (
              <tr key={c.id}>
                <td>{c.name}</td>
                <td className="dim">{c.image}</td>
                <td>{c.status}</td>
                <td className="mono dim">{c.ports || "-"}</td>
                <td className="actions">
                  {c.status.startsWith("Exited") ? (
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
                </td>
              </tr>
            ))}
            {containers.length === 0 && (
              <tr>
                <td colSpan={5} className="empty">
                  No containers
                </td>
              </tr>
            )}
          </tbody>
        </table>
      )}
    </div>
  );
}
