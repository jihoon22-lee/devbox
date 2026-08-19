import { useCallback, useEffect, useRef, useState } from "react";
import {
  createProfile,
  deleteProfile,
  listProfiles,
  onOpenRequest,
  projectHealth,
  startWorkspace,
  stopWorkspace,
  takePendingOpen,
  updateProfile,
  type OpenRequest,
  type ProjectHealth,
  type ProjectProfile,
  type WorkspaceRun,
} from "./api";
import { routeOpenRequest } from "./lib/applink";
import "./App.css";

function emptyProfile(): ProjectProfile {
  return {
    id: "",
    name: "",
    windowsPath: null,
    wsl: null,
    gitRoot: null,
    expectedPorts: [],
    runManagerServiceIds: [],
  };
}

export default function App() {
  const [profiles, setProfiles] = useState<ProjectProfile[]>([]);
  const [editing, setEditing] = useState<ProjectProfile | null>(null);
  const [selectedId, setSelectedId] = useState<string>("");
  const [health, setHealth] = useState<ProjectHealth | null>(null);
  const [run, setRun] = useState<WorkspaceRun | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // Flips true once the first listProfiles() resolves (success or failure).
  // Gates applink handling (below) so a `path` target is matched against the
  // real profile list instead of racing the empty initial state.
  const [profilesLoaded, setProfilesLoaded] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const list = await listProfiles();
      setProfiles(list);
      setSelectedId((prev) => (prev && list.some((p) => p.id === prev) ? prev : list[0]?.id ?? ""));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setProfilesLoaded(true);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Inbound cross-app open requests (§1.4, §3). Redefined every render so it
  // always closes over the latest `profiles` — the devbox://open listener
  // below is set up once and lives for the app's lifetime, so without this a
  // relaunch long after mount would match against a stale profile list.
  const handleOpenRequest = (request: OpenRequest) => {
    const action = routeOpenRequest(request, profiles);
    switch (action.kind) {
      case "selectProfile":
        setSelectedId(action.profileId);
        setEditing(null);
        break;
      case "draftProfile": {
        // No matching profile — surface it via the create-profile draft form
        // (this app's existing affordance) instead of silently doing nothing.
        const draft = emptyProfile();
        if (action.looksWindows) draft.windowsPath = action.path;
        else draft.wsl = { distro: "", path: action.path };
        setEditing(draft);
        setError(`연결된 프로필을 찾지 못해 새 프로필 초안을 열었습니다: ${action.path}`);
        break;
      }
      case "noop":
        console.info(`applink: ${action.reason}`);
        break;
    }
  };
  const handleOpenRequestRef = useRef(handleOpenRequest);
  handleOpenRequestRef.current = handleOpenRequest;

  // Cold start pulls take_pending_open once; a relaunch of this same running
  // instance arrives as the devbox://open event. Both converge on
  // handleOpenRequest so the two paths behave identically. Gated on
  // profilesLoaded so the match against `profiles` is against real data.
  useEffect(() => {
    if (!profilesLoaded) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void takePendingOpen()
      .then((request) => {
        if (!disposed && request) handleOpenRequestRef.current(request);
      })
      .catch(() => undefined);

    void onOpenRequest((request) => handleOpenRequestRef.current(request)).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [profilesLoaded]);

  useEffect(() => {
    if (!selectedId) return;
    void projectHealth(selectedId).then(setHealth).catch(() => undefined);
  }, [selectedId]);

  const onSave = async () => {
    if (!editing) return;
    setBusy(true);
    setError(null);
    try {
      if (editing.id) {
        await updateProfile(editing);
      } else {
        await createProfile(editing);
      }
      setEditing(null);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const onDelete = async (id: string) => {
    setBusy(true);
    setError(null);
    try {
      await deleteProfile(id);
      setSelectedId("");
      setHealth(null);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const onStart = async () => {
    if (!selectedId) return;
    setBusy(true);
    setError(null);
    try {
      setRun(await startWorkspace(selectedId));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const onStop = async () => {
    if (!run) return;
    setBusy(true);
    setError(null);
    try {
      const n = await stopWorkspace(run.runId);
      setRun(null);
      if (n > 0) setError(`Workbench가 시작한 프로세스 ${n}개를 종료했습니다.`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const patch = (p: Partial<ProjectProfile>) => setEditing((prev) => (prev ? { ...prev, ...p } : prev));

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">Workbench</h1>
        <button className="btn" onClick={() => { setEditing(emptyProfile()); }}>+ 프로필</button>
        <button className="btn refresh" onClick={() => void refresh()}>새로고침</button>
      </header>

      {error && <div className="error">{error}</div>}

      <div className="main">
        <aside className="sidebar">
          <h2 className="group-title">프로젝트</h2>
          {profiles.map((p) => (
            <div key={p.id} className={`profile-row ${p.id === selectedId ? "active" : ""}`}>
              <button className="profile-name" onClick={() => setSelectedId(p.id)}>
                {p.name}
              </button>
              <button className="mini" onClick={() => setEditing(p)} title="편집">✏️</button>
              <button className="mini" onClick={() => void onDelete(p.id)} title="삭제">✕</button>
            </div>
          ))}
          {profiles.length === 0 && <div className="dim">프로필이 없습니다.</div>}
        </aside>

        <main className="content">
          {editing ? (
            <section className="panel">
              <h2>{editing.id ? "프로필 편집" : "새 프로필"}</h2>
              <label className="field">
                <span>이름</span>
                <input value={editing.name} onChange={(e) => patch({ name: e.currentTarget.value })} />
              </label>
              <label className="field">
                <span>Windows 경로</span>
                <input value={editing.windowsPath ?? ""} onChange={(e) => patch({ windowsPath: e.currentTarget.value || null })} />
              </label>
              <label className="field">
                <span>WSL distro</span>
                <input value={editing.wsl?.distro ?? ""} onChange={(e) => patch({ wsl: { distro: e.currentTarget.value, path: editing.wsl?.path ?? "" } })} />
              </label>
              <label className="field">
                <span>WSL 경로</span>
                <input value={editing.wsl?.path ?? ""} onChange={(e) => patch({ wsl: { distro: editing.wsl?.distro ?? "", path: e.currentTarget.value } })} />
              </label>
              <label className="field">
                <span>Git root</span>
                <input value={editing.gitRoot ?? ""} onChange={(e) => patch({ gitRoot: e.currentTarget.value || null })} />
              </label>
              <label className="field">
                <span>예상 포트 (쉼표)</span>
                <input
                  value={editing.expectedPorts.join(", ")}
                  onChange={(e) =>
                    patch({
                      expectedPorts: e.currentTarget.value.split(",").map((s) => Number(s.trim())).filter((n) => Number.isFinite(n) && n > 0),
                    })
                  }
                />
              </label>
              <div className="actions">
                <button className="btn primary" disabled={busy || !editing.name.trim()} onClick={() => void onSave()}>저장</button>
                <button className="btn" onClick={() => setEditing(null)}>취소</button>
              </div>
            </section>
          ) : selectedId ? (
            <section className="panel">
              <h2>{profiles.find((p) => p.id === selectedId)?.name ?? selectedId}</h2>
              <div className="row-actions">
                <button className="btn primary" disabled={busy} onClick={() => void onStart()}>Start Workspace</button>
                {run && (
                  <button className="btn danger" disabled={busy} onClick={() => void onStop()}>Stop What I Started</button>
                )}
              </div>

              <h3 className="subtitle">Health</h3>
              {health?.items.map((item) => (
                <div key={item.name} className={`health-row ${item.ok ? "ok" : "bad"}`}>
                  <span className="health-name">{item.name}</span>
                  <span className="health-detail">{item.detail}</span>
                </div>
              ))}

              {run && (
                <>
                  <h3 className="subtitle">Start Workspace 결과</h3>
                  {run.steps.map((step, i) => (
                    <div key={i} className={`health-row ${step.ok ? "ok" : "bad"}`}>
                      <span className="health-name">{step.name}</span>
                      <span className="health-detail">{step.detail}</span>
                    </div>
                  ))}
                </>
              )}
            </section>
          ) : (
            <div className="empty">왼쪽에서 프로필을 선택하세요.</div>
          )}
        </main>
      </div>
    </div>
  );
}
