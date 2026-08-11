import { useCallback, useEffect, useState } from "react";
import { dockerAction, dockerPs, gitStatus, listDistros, openTerminal } from "./api";
import type { ContainerInfo, DistroInfo, GitStatus } from "./types";
import "./App.css";

const LS_KEY = "wsld-projects";

function loadSavedPaths(): string[] {
  try {
    return JSON.parse(localStorage.getItem(LS_KEY) ?? "[]");
  } catch {
    return [];
  }
}

export default function App() {
  const [distros, setDistros] = useState<DistroInfo[]>([]);
  const [containers, setContainers] = useState<ContainerInfo[]>([]);
  const [projects, setProjects] = useState<GitStatus[]>([]);
  const [selectedDistro, setSelectedDistro] = useState("");
  const [savedPaths, setSavedPaths] = useState<string[]>(loadSavedPaths);
  const [newPath, setNewPath] = useState("");
  const [dockerMissing, setDockerMissing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const ds = await listDistros();
      setDistros(ds);
      const distro = ds.find((d) => d.default)?.name ?? ds[0]?.name ?? "Ubuntu";
      setSelectedDistro(distro);

      // docker는 별도로 처리: 미설치 시 안내 배너만
      try {
        setContainers(await dockerPs(distro));
        setDockerMissing(false);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        if (/command not found|docker/i.test(msg)) {
          setContainers([]);
          setDockerMissing(true);
        } else {
          setError(msg);
        }
      }

      setProjects(await gitStatus(savedPaths));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [savedPaths]);

  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const persist = (paths: string[]) => {
    setSavedPaths(paths);
    localStorage.setItem(LS_KEY, JSON.stringify(paths));
  };

  const addPath = () => {
    const p = newPath.trim();
    if (!p) return;
    const next = savedPaths.includes(p) ? savedPaths : [...savedPaths, p];
    persist(next);
    setNewPath("");
  };

  const removePath = (p: string) => {
    persist(savedPaths.filter((x) => x !== p));
  };

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

      {dockerMissing && (
        <div className="banner">
          WSL에 Docker가 설치되어 있지 않습니다. Docker 컨테이너 관리 기능을 사용하려면 WSL에서{" "}
          <code>sudo apt install docker.io</code> 또는 Docker Desktop을 설치하세요.
        </div>
      )}

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
        )}
      </section>

      <section className="panel">
        <h2>Projects</h2>
        <div className="project-add">
          <input
            placeholder="Add project path (C:\projects\devbox)"
            value={newPath}
            onChange={(e) => setNewPath(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") addPath();
            }}
          />
          <button className="btn" onClick={addPath}>
            Add
          </button>
        </div>
        <table>
          <thead>
            <tr>
              <th>PATH</th>
              <th>BRANCH</th>
              <th>CHANGES</th>
              <th>STATUS</th>
              <th />
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
                <td>
                  <button className="mini" title="Remove" onClick={() => removePath(p.path)}>
                    ✕
                  </button>
                </td>
              </tr>
            ))}
            {projects.length === 0 && (
              <tr>
                <td colSpan={5} className="empty">
                  No projects added
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </section>
    </div>
  );
}
