import { useCallback, useEffect, useState } from "react";
import { dockerAction, dockerPs, gitStatus, listDistros, openTerminal } from "./api";
import type { ContainerInfo, DistroInfo, GitStatus } from "./types";
import "./App.css";

export default function App() {
  const [distros, setDistros] = useState<DistroInfo[]>([]);
  const [containers, setContainers] = useState<ContainerInfo[]>([]);
  const [projects, setProjects] = useState<GitStatus[]>([]);
  const [selectedDistro, setSelectedDistro] = useState("");
  const [projectPaths, setProjectPaths] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const ds = await listDistros();
      setDistros(ds);
      const distro = ds.find((d) => d.default)?.name ?? ds[0]?.name ?? "Ubuntu";
      setSelectedDistro(distro);
      const [ct, pr] = await Promise.all([dockerPs(distro), gitStatus(projectPaths)]);
      setContainers(ct);
      setProjects(pr);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [projectPaths]);

  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onAction = async (id: string, action: "start" | "stop" | "restart") => {
    setBusy(`${id}:${action}`);
    setError(null);
    try {
      await dockerAction(selectedDistro, id, action);
      setContainers(await dockerPs(selectedDistro));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };

  const running = containers.filter((c) => c.status.startsWith("Up")).length;

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">WSL Dashboard</h1>
        <select value={selectedDistro} onChange={(e) => setSelectedDistro(e.currentTarget.value)}>
          {distros.map((d) => (
            <option key={d.name} value={d.name}>
              {d.name} {d.default ? "(default)" : ""}
            </option>
          ))}
        </select>
        <button className="btn refresh" onClick={() => void refresh()}>
          Refresh
        </button>
      </header>

      {error && <div className="error">{error}</div>}

      <section className="cards">
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
            {d.default && (
              <button className="btn" onClick={() => void openTerminal(d.name)}>
                Open Terminal
              </button>
            )}
          </div>
        ))}
      </section>

      <section className="panel">
        <h2>Docker ({running}/{containers.length} running)</h2>
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
                    <button className="btn" disabled={busy === `${c.id}:start`} onClick={() => void onAction(c.id, "start")}>
                      Start
                    </button>
                  ) : (
                    <>
                      <button className="btn danger" disabled={busy === `${c.id}:stop`} onClick={() => void onAction(c.id, "stop")}>
                        Stop
                      </button>
                      <button className="btn" disabled={busy === `${c.id}:restart`} onClick={() => void onAction(c.id, "restart")}>
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
      </section>

      <section className="panel">
        <h2>Projects</h2>
        <div className="project-add">
          <input
            placeholder="Add project path (C:\projects\devbox)"
            value={projectPaths[0] ?? ""}
            onChange={(e) => setProjectPaths([e.currentTarget.value])}
            onKeyDown={(e) => {
              if (e.key === "Enter") void refresh();
            }}
          />
        </div>
        <table>
          <thead>
            <tr>
              <th>PATH</th>
              <th>BRANCH</th>
              <th>CHANGES</th>
              <th>STATUS</th>
            </tr>
          </thead>
          <tbody>
            {projects.map((p) => (
              <tr key={p.path}>
                <td className="mono">{p.path}</td>
                <td>{p.branch}</td>
                <td>{p.changes}</td>
                <td>
                  <span className={p.clean ? "tag-ok" : "tag-dirty"}>{p.clean ? "clean" : "dirty"}</span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
    </div>
  );
}
