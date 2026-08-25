import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  createProfile,
  currentWorkspaceRun,
  deleteProfile,
  listProfiles,
  onOpenRequest,
  openProfileIn,
  profileCopyPath,
  projectHealth,
  profileOpenTargets,
  startWorkspace,
  stopWorkspace,
  takePendingOpen,
  updateProfile,
  type OpenRequest,
  type ProjectHealth,
  type ProjectProfile,
  type WorkspaceRun,
  type WorkbenchOpenTarget,
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
  const [contextProfile, setContextProfile] = useState<ProjectProfile | null>(null);
  const [contextTargets, setContextTargets] = useState<{
    profileId: string;
    targets: WorkbenchOpenTarget[];
  } | null>(null);
  const contextTargetRequest = useRef(0);
  // Flips true once the first listProfiles() resolves (success or failure).
  // Gates applink handling (below) so a `path` target is matched against the
  // real profile list instead of racing the empty initial state.
  const [profilesLoaded, setProfilesLoaded] = useState(false);

  const prepareProfileContext = useCallback((target: HTMLElement) => {
    const id = target.dataset.profileId;
    const profile = profiles.find((candidate) => candidate.id === id);
    if (!profile) return;
    setSelectedId(profile.id);
    setContextProfile(profile);
    setContextTargets(null);
    const request = ++contextTargetRequest.current;
    void profileOpenTargets(profile.id)
      .then((targets) => {
        if (request === contextTargetRequest.current) {
          setContextTargets({ profileId: profile.id, targets });
        }
      })
      .catch(() => {
        if (request === contextTargetRequest.current) {
          setContextTargets({ profileId: profile.id, targets: [] });
          setError("다른 앱으로 열기 대상을 확인하지 못했습니다");
        }
      });
  }, [profiles]);
  const profileContextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => prepareProfileContext(target),
  });

  const refresh = useCallback(async () => {
    try {
      const [list, activeRun] = await Promise.all([listProfiles(), currentWorkspaceRun()]);
      setProfiles(list);
      setRun(activeRun ? { ...activeRun, steps: [], startedPids: [] } : null);
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

  useEffect(() => {
    const id = contextProfile?.id;
    if (!id) return;
    const current = profiles.find((profile) => profile.id === id) ?? null;
    if (current) setContextProfile(current);
    else {
      contextTargetRequest.current += 1;
      profileContextMenu.close();
      setContextProfile(null);
      setContextTargets(null);
    }
  }, [contextProfile?.id, profileContextMenu.close, profiles]);

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

    const consumePendingOpen = () => {
      void takePendingOpen()
        .then((request) => {
          if (!disposed && request) handleOpenRequestRef.current(request);
        })
        .catch(() => undefined);
    };
    let coldStartConsumed = false;
    const consumeColdStart = () => {
      if (disposed || coldStartConsumed) return;
      coldStartConsumed = true;
      consumePendingOpen();
    };

    void onOpenRequest(() => consumePendingOpen())
      .then((stop) => {
        if (disposed) stop();
        else {
          unlisten = stop;
          consumeColdStart();
        }
      })
      .catch(() => {
        consumeColdStart();
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

  const onDelete = async (profile: ProjectProfile) => {
    if (run?.profileId === profile.id) {
      setError("실행 중인 프로필은 먼저 Workbench가 시작한 리소스를 중지하세요.");
      return;
    }
    if (!window.confirm(
      `'${profile.name}' 프로필을 삭제할까요? 저장된 프로필 정의만 삭제하며 프로젝트 파일과 이미 실행 중이던 외부 리소스는 변경하지 않습니다.`,
    )) return;
    setBusy(true);
    setError(null);
    try {
      await deleteProfile(profile.id);
      setSelectedId("");
      setHealth(null);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const onStart = async (profileId: string) => {
    if (!profileId) return;
    if (run) {
      setError("현재 Workspace 실행을 먼저 중지하세요.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      setRun(await startWorkspace(profileId));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const onStop = async (profile: ProjectProfile) => {
    if (!run || run.profileId !== profile.id) {
      setError("선택한 프로필에서 Workbench가 시작한 실행을 찾을 수 없습니다.");
      return;
    }
    if (!window.confirm(
      `'${profile.name}'에서 Workbench가 시작한 리소스만 중지할까요? 시작 전부터 실행 중이던 리소스는 유지됩니다.`,
    )) return;
    setBusy(true);
    setError(null);
    try {
      const n = await stopWorkspace(run.runId, profile.id);
      setRun(null);
      if (n > 0) setError(`Workbench가 시작한 프로세스 ${n}개를 종료했습니다.`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const onCopyProfilePath = async (profileId: string) => {
    setError(null);
    try {
      const path = await profileCopyPath(profileId);
      await navigator.clipboard.writeText(path);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onOpenProfileIn = async (profileId: string, appId: string) => {
    setBusy(true);
    setError(null);
    try {
      await openProfileIn(profileId, appId);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const resolvedContextTargets = contextProfile && contextTargets?.profileId === contextProfile.id
    ? contextTargets.targets
    : null;
  const contextRun = contextProfile && run?.profileId === contextProfile.id ? run : null;
  const contextHasPath = Boolean(
    contextProfile?.windowsPath?.trim() || contextProfile?.wsl?.path.trim(),
  );
  const profileContextItems = useMemo<readonly ContextMenuEntry[]>(() => {
    if (!contextProfile) return [];
    const openTargetItems: ContextMenuEntry[] = (resolvedContextTargets ?? []).map((target) => ({
      type: "item",
      id: `open-in:${target.id}`,
      label: target.displayName,
    }));
    return [
      {
        type: "item",
        id: "start",
        label: "Start Workspace",
        disabled: busy || run !== null,
      },
      {
        type: "item",
        id: "stop",
        label: "Stop What I Started",
        disabled: busy || contextRun === null,
        danger: true,
      },
      { type: "separator", id: "lifecycle-separator" },
      { type: "item", id: "edit", label: "프로필 편집", disabled: busy },
      {
        type: "item",
        id: "delete",
        label: "삭제",
        disabled: busy || contextRun !== null,
        danger: true,
      },
      { type: "separator", id: "path-separator" },
      {
        type: "item",
        id: "copy-path",
        label: "경로 복사",
        disabled: busy || !contextHasPath,
      },
      {
        type: "submenu",
        id: "open-in",
        label: "다른 앱으로 열기",
        disabled: busy || resolvedContextTargets === null || openTargetItems.length === 0,
        items: openTargetItems,
      },
    ];
  }, [busy, contextHasPath, contextProfile, contextRun, resolvedContextTargets, run]);

  const onProfileContextSelect = (id: string) => {
    const profile = contextProfile;
    if (!profile) return;
    if (id === "start") void onStart(profile.id);
    else if (id === "stop") void onStop(profile);
    else if (id === "edit") setEditing(profile);
    else if (id === "delete") void onDelete(profile);
    else if (id === "copy-path") void onCopyProfilePath(profile.id);
    else {
      const target = resolvedContextTargets?.find((candidate) => `open-in:${candidate.id}` === id);
      if (target) void onOpenProfileIn(profile.id, target.id);
    }
  };

  const patch = (p: Partial<ProjectProfile>) => setEditing((prev) => (prev ? { ...prev, ...p } : prev));

  const selectedProfile = profiles.find((profile) => profile.id === selectedId) ?? null;

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
            <div
              key={p.id}
              className={`profile-row ${p.id === selectedId ? "active" : ""}`}
              tabIndex={0}
              aria-current={p.id === selectedId ? "true" : undefined}
              data-profile-id={p.id}
              onClick={() => setSelectedId(p.id)}
              {...profileContextMenu.triggerProps}
            >
              <button className="profile-name" onClick={() => setSelectedId(p.id)}>
                {p.name}
              </button>
              <button className="mini" disabled={busy} onClick={() => setEditing(p)} title="편집">✏️</button>
              <button
                className="mini"
                disabled={busy || run?.profileId === p.id}
                onClick={() => void onDelete(p)}
                title="삭제"
              >
                ✕
              </button>
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
          ) : selectedProfile ? (
            <section className="panel">
              <h2>{selectedProfile.name}</h2>
              <div className="row-actions">
                <button
                  className="btn primary"
                  disabled={busy || run !== null}
                  onClick={() => void onStart(selectedProfile.id)}
                >
                  Start Workspace
                </button>
                {run?.profileId === selectedProfile.id && (
                  <button className="btn danger" disabled={busy} onClick={() => void onStop(selectedProfile)}>
                    Stop What I Started
                  </button>
                )}
              </div>

              <h3 className="subtitle">Health</h3>
              {health?.items.map((item) => (
                <div key={item.name} className={`health-row ${item.ok ? "ok" : "bad"}`}>
                  <span className="health-name">{item.name}</span>
                  <span className="health-detail">{item.detail}</span>
                </div>
              ))}

              {run?.profileId === selectedProfile.id && (
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
      <ContextMenu
        open={profileContextMenu.open}
        anchor={profileContextMenu.anchor}
        restoreFocusTo={profileContextMenu.restoreFocusTo}
        items={profileContextItems}
        onSelect={onProfileContextSelect}
        onClose={profileContextMenu.close}
        ariaLabel="프로필 메뉴"
      />
    </div>
  );
}
