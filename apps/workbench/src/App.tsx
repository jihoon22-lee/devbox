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
import {
  draftFromProfile,
  emptyProfileDraft,
  MAX_EXPECTED_PORTS_INPUT_CHARS,
  MAX_PROFILE_NAME_CHARS,
  MAX_PROFILE_PATH_BYTES,
  MAX_SERVICE_ID_CHARS,
  MAX_SERVICES,
  MAX_WSL_DISTRO_CHARS,
  newServiceDraftRow,
  validateProfileDraft,
  type ProfileDraft,
} from "./lib/profileEditor";
import "./App.css";

export default function App() {
  const [profiles, setProfiles] = useState<ProjectProfile[]>([]);
  const [editing, setEditing] = useState<ProfileDraft | null>(null);
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
  const refreshRequest = useRef(0);
  const healthRequest = useRef(0);
  const saveInFlight = useRef(false);
  const [profilesRevision, setProfilesRevision] = useState(0);
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
    const request = ++refreshRequest.current;
    try {
      const [list, activeRun] = await Promise.all([listProfiles(), currentWorkspaceRun()]);
      if (request !== refreshRequest.current) return;
      setProfiles(list);
      setRun(activeRun ? { ...activeRun, steps: [], startedPids: [] } : null);
      setSelectedId((prev) => (prev && list.some((p) => p.id === prev) ? prev : list[0]?.id ?? ""));
      setProfilesRevision((revision) => revision + 1);
    } catch {
      if (request === refreshRequest.current) {
        // A failed read must not leave actionable stale profiles on screen.
        healthRequest.current += 1;
        setProfiles([]);
        setSelectedId("");
        setHealth(null);
        setRun(null);
        setError("프로필 목록을 불러올 수 없습니다.");
      }
    } finally {
      if (request === refreshRequest.current) setProfilesLoaded(true);
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
        const draft = emptyProfileDraft();
        if (action.looksWindows) draft.windowsPath = action.path;
        else draft.wslPath = action.path;
        setEditing(draft);
        setError("연결된 프로필을 찾지 못해 새 프로필 초안을 열었습니다.");
        break;
      }
      case "noop":
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
    const request = ++healthRequest.current;
    if (!selectedId) {
      setHealth(null);
      return;
    }
    setHealth(null);
    void projectHealth(selectedId)
      .then((result) => {
        if (request === healthRequest.current && result.profileId === selectedId) setHealth(result);
      })
      .catch(() => {
        if (request === healthRequest.current) setError("프로젝트 상태를 확인할 수 없습니다.");
      });
  }, [profilesRevision, selectedId]);

  const onSave = async () => {
    if (!editing || saveInFlight.current) return;
    const validation = validateProfileDraft(editing);
    if (!validation.profile) {
      setError("프로필 입력값을 확인한 뒤 저장하세요.");
      return;
    }
    saveInFlight.current = true;
    setBusy(true);
    setError(null);
    try {
      if (validation.profile.id) {
        await updateProfile(validation.profile);
      } else {
        await createProfile(validation.profile);
      }
      setEditing(null);
      await refresh();
    } catch {
      // Backend errors are deliberately not echoed: path strings and future
      // service metadata must not become UI/telemetry output by accident.
      setError("프로필을 저장할 수 없습니다. 입력과 경로를 확인하세요.");
    } finally {
      saveInFlight.current = false;
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
    } catch {
      setError("프로필을 삭제할 수 없습니다.");
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
    } catch {
      setError("Workspace를 시작할 수 없습니다.");
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
    } catch {
      setError("Workspace 실행을 중지할 수 없습니다.");
    } finally {
      setBusy(false);
    }
  };

  const onCopyProfilePath = async (profileId: string) => {
    setError(null);
    try {
      const path = await profileCopyPath(profileId);
      await navigator.clipboard.writeText(path);
    } catch {
      setError("프로필 경로를 클립보드에 복사할 수 없습니다.");
    }
  };

  const onOpenProfileIn = async (profileId: string, appId: string) => {
    setBusy(true);
    setError(null);
    try {
      await openProfileIn(profileId, appId);
    } catch {
      setError("선택한 앱으로 프로필을 열 수 없습니다.");
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
    else if (id === "edit") setEditing(draftFromProfile(profile));
    else if (id === "delete") void onDelete(profile);
    else if (id === "copy-path") void onCopyProfilePath(profile.id);
    else {
      const target = resolvedContextTargets?.find((candidate) => `open-in:${candidate.id}` === id);
      if (target) void onOpenProfileIn(profile.id, target.id);
    }
  };

  const patch = (p: Partial<ProfileDraft>) => setEditing((prev) => (prev ? { ...prev, ...p } : prev));
  const draftValidation = editing ? validateProfileDraft(editing) : null;

  const selectedProfile = profiles.find((profile) => profile.id === selectedId) ?? null;

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">Workbench</h1>
        <button type="button" className="btn" disabled={busy} onClick={() => { setEditing(emptyProfileDraft()); }}>+ 프로필</button>
        <button type="button" className="btn refresh" disabled={busy} onClick={() => void refresh()}>새로고침</button>
      </header>

      {error && <div className="error" role="alert" aria-live="assertive">{error}</div>}

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
              <button type="button" className="profile-name" disabled={busy} onClick={() => setSelectedId(p.id)}>
                {p.name}
              </button>
              <button
                type="button"
                className="mini"
                disabled={busy}
                onClick={() => setEditing(draftFromProfile(p))}
                title="편집"
                aria-label={`${p.name} 프로필 편집`}
              >✏️</button>
              <button
                type="button"
                className="mini"
                disabled={busy || run?.profileId === p.id}
                onClick={() => void onDelete(p)}
                title="삭제"
                aria-label={`${p.name} 프로필 삭제`}
              >
                ✕
              </button>
            </div>
          ))}
          {profiles.length === 0 && <div className="dim">프로필이 없습니다.</div>}
        </aside>

        <main className="content">
          {editing ? (
            <section
              className="panel editor-panel"
              aria-labelledby="profile-editor-title"
              aria-busy={busy}
              onKeyDown={(event) => {
                if (event.key === "Escape" && !event.nativeEvent.isComposing && !busy) {
                  event.preventDefault();
                  setEditing(null);
                }
              }}
            >
              <h2 id="profile-editor-title">{editing.id ? "프로필 편집" : "새 프로필"}</h2>
              <form
                onSubmit={(event) => {
                  event.preventDefault();
                  void onSave();
                }}
              >
                <label className="field" htmlFor="profile-name">
                  <span>이름</span>
                  <input
                    id="profile-name"
                    value={editing.name}
                    maxLength={MAX_PROFILE_NAME_CHARS}
                    autoFocus
                    disabled={busy}
                    aria-invalid={Boolean(draftValidation?.errors.name)}
                    aria-describedby={draftValidation?.errors.name ? "profile-name-error" : undefined}
                    onChange={(e) => patch({ name: e.currentTarget.value })}
                  />
                  {draftValidation?.errors.name && <span id="profile-name-error" className="field-error" role="alert">{draftValidation.errors.name}</span>}
                </label>
                <label className="field" htmlFor="profile-windows-path">
                  <span>Windows 경로</span>
                  <input
                    id="profile-windows-path"
                    value={editing.windowsPath}
                    maxLength={MAX_PROFILE_PATH_BYTES}
                    disabled={busy}
                    aria-invalid={Boolean(draftValidation?.errors.projectPath)}
                    aria-describedby={draftValidation?.errors.projectPath ? "profile-project-path-error" : undefined}
                    onChange={(e) => patch({ windowsPath: e.currentTarget.value })}
                  />
                </label>
                <label className="field" htmlFor="profile-wsl-distro">
                  <span>WSL distro</span>
                  <input
                    id="profile-wsl-distro"
                    value={editing.wslDistro}
                    maxLength={MAX_WSL_DISTRO_CHARS}
                    disabled={busy}
                    aria-invalid={Boolean(draftValidation?.errors.wsl)}
                    aria-describedby={draftValidation?.errors.wsl ? "profile-wsl-error" : undefined}
                    onChange={(e) => patch({ wslDistro: e.currentTarget.value })}
                  />
                </label>
                <label className="field" htmlFor="profile-wsl-path">
                  <span>WSL 경로</span>
                  <input
                    id="profile-wsl-path"
                    value={editing.wslPath}
                    maxLength={MAX_PROFILE_PATH_BYTES}
                    disabled={busy}
                    aria-invalid={Boolean(draftValidation?.errors.projectPath || draftValidation?.errors.wsl)}
                    aria-describedby={draftValidation?.errors.projectPath ? "profile-project-path-error" : draftValidation?.errors.wsl ? "profile-wsl-error" : undefined}
                    onChange={(e) => patch({ wslPath: e.currentTarget.value })}
                  />
                  {draftValidation?.errors.wsl && <span id="profile-wsl-error" className="field-error" role="alert">{draftValidation.errors.wsl}</span>}
                </label>
                <label className="field" htmlFor="profile-git-root">
                  <span>Git root</span>
                  <input
                    id="profile-git-root"
                    value={editing.gitRoot}
                    maxLength={MAX_PROFILE_PATH_BYTES}
                    disabled={busy}
                    aria-invalid={Boolean(draftValidation?.errors.gitRoot)}
                    aria-describedby={draftValidation?.errors.gitRoot ? "profile-git-root-error" : undefined}
                    onChange={(e) => patch({ gitRoot: e.currentTarget.value })}
                  />
                  {draftValidation?.errors.gitRoot && <span id="profile-git-root-error" className="field-error" role="alert">{draftValidation.errors.gitRoot}</span>}
                </label>
                <label className="field" htmlFor="profile-expected-ports">
                  <span>예상 포트 (쉼표)</span>
                  <input
                    id="profile-expected-ports"
                    value={editing.expectedPortsText}
                    maxLength={MAX_EXPECTED_PORTS_INPUT_CHARS}
                    inputMode="numeric"
                    placeholder="예: 3000, 5173"
                    disabled={busy}
                    aria-invalid={Boolean(draftValidation?.errors.expectedPorts)}
                    aria-describedby={draftValidation?.errors.expectedPorts ? "profile-ports-error" : "profile-ports-help"}
                    onChange={(e) => patch({ expectedPortsText: e.currentTarget.value })}
                  />
                  <span id="profile-ports-help" className="field-help">프로필 health 점검과 Start Workspace에서 확인할 로컬 TCP 포트입니다.</span>
                  {draftValidation?.errors.expectedPorts && <span id="profile-ports-error" className="field-error" role="alert">{draftValidation.errors.expectedPorts}</span>}
                </label>
                <fieldset
                  className="editor-section"
                  disabled={busy}
                  aria-describedby={draftValidation?.errors.services ? "profile-services-error" : undefined}
                >
                  <legend>Run Manager 서비스</legend>
                  <p className="field-help">연결할 서비스 ID를 등록합니다. 이 화면은 서비스 자체를 시작·수정하지 않습니다.</p>
                  {editing.serviceRows.map((row, index) => (
                    <div className="editable-list-row" key={row.key}>
                      <label className="sr-only" htmlFor={`service-${row.key}`}>서비스 {index + 1}</label>
                      <input
                        id={`service-${row.key}`}
                        value={row.value}
                        maxLength={MAX_SERVICE_ID_CHARS}
                        placeholder="예: devbox-dev"
                        aria-invalid={Boolean(draftValidation?.errors.serviceRows[row.key])}
                        aria-describedby={draftValidation?.errors.serviceRows[row.key] ? `service-error-${row.key}` : undefined}
                        onChange={(e) => patch({
                          serviceRows: editing.serviceRows.map((candidate) => (
                            candidate.key === row.key ? { ...candidate, value: e.currentTarget.value } : candidate
                          )),
                        })}
                      />
                      <button
                        type="button"
                        className="btn"
                        onClick={() => patch({ serviceRows: editing.serviceRows.filter((candidate) => candidate.key !== row.key) })}
                        aria-label={`서비스 ${index + 1} 삭제`}
                      >
                        삭제
                      </button>
                      {draftValidation?.errors.serviceRows[row.key] && (
                        <span id={`service-error-${row.key}`} className="field-error" role="alert">{draftValidation.errors.serviceRows[row.key]}</span>
                      )}
                    </div>
                  ))}
                  {draftValidation?.errors.services && <span id="profile-services-error" className="field-error" role="alert">{draftValidation.errors.services}</span>}
                  <button
                    type="button"
                    className="btn"
                    disabled={editing.serviceRows.length >= MAX_SERVICES}
                    onClick={() => patch({ serviceRows: [...editing.serviceRows, newServiceDraftRow()] })}
                  >
                    + 서비스 추가
                  </button>
                </fieldset>
                {draftValidation?.errors.projectPath && <div id="profile-project-path-error" className="field-error form-error" role="alert">{draftValidation.errors.projectPath}</div>}
                {draftValidation?.errors.id && <div className="field-error form-error" role="alert">{draftValidation.errors.id}</div>}
                <div className="actions">
                  <button type="submit" className="btn primary" disabled={busy || !draftValidation?.profile}>저장</button>
                  <button type="button" className="btn" disabled={busy} onClick={() => setEditing(null)}>취소</button>
                </div>
              </form>
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
