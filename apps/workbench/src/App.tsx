import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  createProfile,
  cancelStartWorkspace,
  cancelProjectEnvironment,
  cancelProjectHealth,
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
  previewProjectEnvironment,
  workspacePreflight,
  wslRuntimeSuggestions,
  type OpenRequest,
  type PreflightItem,
  type ProjectEnvironmentPreview,
  type ProjectHealth,
  type ProjectProfile,
  type ResourceProvenance,
  type WorkspaceRun,
  type WorkspacePreflight,
  type WorkbenchOpenTarget,
  type RuntimeSuggestions,
} from "./api";
import { routeOpenRequest } from "./lib/applink";
import {
  draftFromProfile,
  emptyProfileDraft,
  MAX_EXPECTED_PORTS_INPUT_CHARS,
  MAX_ENVIRONMENT_SOURCE_BYTES,
  MAX_PROFILE_NAME_CHARS,
  MAX_PROFILE_PATH_BYTES,
  MAX_SERVICE_ID_CHARS,
  MAX_SERVICES,
  MAX_WSL_DISTRO_CHARS,
  newServiceDraftRow,
  parseExpectedPorts,
  validateProfileDraft,
  type ProfileDraft,
} from "./lib/profileEditor";
import { formatRuntimeFreshness, mergeSuggestedPorts } from "./lib/runtimeSuggestions";
import "./App.css";

const RUNTIME_STATUS_LABEL: Record<RuntimeSuggestions["status"], string> = {
  fresh: "최신 snapshot",
  stale: "오래된 snapshot — 반영 시 추가 확인 필요",
  expired: "만료된 snapshot — 반영 불가",
  missing: "WSL Desktop snapshot 없음",
  corrupt: "WSL Desktop snapshot을 안전하게 읽을 수 없음",
};

const PREFLIGHT_ITEM_LABEL: Record<string, string> = {
  "required-apps": "필수 앱",
  "wsl-distro": "WSL distro",
  "working-directory": "working directory",
  ports: "예상 port",
  "service-dependencies": "service dependency",
};

const PREFLIGHT_STATUS_LABEL: Record<PreflightItem["status"], string> = {
  pass: "통과",
  warning: "경고",
  failure: "차단",
  unavailable: "확인 불가",
};

const RESOURCE_STATE_LABEL: Record<ResourceProvenance["state"], string> = {
  available: "사용 가능",
  existing: "이미 실행 중",
  workbenchStarted: "Workbench가 시작",
  notRunning: "실행 전",
  missing: "없음",
  conflict: "충돌",
  unsafe: "안전하지 않음",
  unavailable: "확인 불가",
};

export default function App() {
  const [profiles, setProfiles] = useState<ProjectProfile[]>([]);
  const [editing, setEditing] = useState<ProfileDraft | null>(null);
  const [selectedId, setSelectedId] = useState<string>("");
  const [health, setHealth] = useState<ProjectHealth | null>(null);
  const [run, setRun] = useState<WorkspaceRun | null>(null);
  const [preflight, setPreflight] = useState<WorkspacePreflight | null>(null);
  const [preflightLoading, setPreflightLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [runtimeSuggestions, setRuntimeSuggestions] = useState<RuntimeSuggestions | null>(null);
  const [selectedRuntimePorts, setSelectedRuntimePorts] = useState<Set<number>>(new Set());
  const [runtimeLoading, setRuntimeLoading] = useState(false);
  const [runtimeAccepting, setRuntimeAccepting] = useState(false);
  const [environmentLoading, setEnvironmentLoading] = useState(false);
  const [startingProfileId, setStartingProfileId] = useState<string | null>(null);
  const [startCancelRequested, setStartCancelRequested] = useState(false);
  const startCancelRequestedRef = useRef(false);
  const [contextProfile, setContextProfile] = useState<ProjectProfile | null>(null);
  const [contextTargets, setContextTargets] = useState<{
    profileId: string;
    targets: WorkbenchOpenTarget[];
  } | null>(null);
  const contextTargetRequest = useRef(0);
  const refreshRequest = useRef(0);
  const healthRequest = useRef(0);
  const healthProfileId = useRef<string | null>(null);
  const saveInFlight = useRef(false);
  const preflightRequest = useRef(0);
  const preflightTarget = useRef<string | null>(null);
  const preflightFocusReturn = useRef<HTMLElement | null>(null);
  const preflightStartInFlight = useRef(false);
  const runtimeRequest = useRef(0);
  const environmentRequest = useRef(0);
  const editingRef = useRef(editing);
  editingRef.current = editing;
  const [profilesRevision, setProfilesRevision] = useState(0);
  // Flips true once the first listProfiles() resolves (success or failure).
  // Gates applink handling (below) so a `path` target is matched against the
  // real profile list instead of racing the empty initial state.
  const [profilesLoaded, setProfilesLoaded] = useState(false);

  const prepareProfileContext = useCallback((target: HTMLElement) => {
    if (busy && !preflightLoading) return;
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
  }, [busy, preflightLoading, profiles]);
  const profileContextMenu = useContextMenu({
    disabled: busy && !preflightLoading,
    onBeforeOpen: (_reason, target) => prepareProfileContext(target),
  });

  const refresh = useCallback(async () => {
    const hadPreflightOperation = preflightTarget.current !== null;
    preflightRequest.current += 1;
    preflightTarget.current = null;
    preflightFocusReturn.current = null;
    setPreflight(null);
    setPreflightLoading(false);
    if (hadPreflightOperation) setBusy(false);
    const request = ++refreshRequest.current;
    try {
      const [list, activeRun] = await Promise.all([listProfiles(), currentWorkspaceRun()]);
      if (request !== refreshRequest.current) return;
      setProfiles(list);
      setRun(activeRun ? { ...activeRun, steps: [], startedPids: [], resourceProvenance: [] } : null);
      setSelectedId((prev) => (prev && list.some((p) => p.id === prev) ? prev : list[0]?.id ?? ""));
      setProfilesRevision((revision) => revision + 1);
    } catch {
      if (request === refreshRequest.current) {
        // A failed read must not leave actionable stale profiles on screen.
        healthRequest.current += 1;
        const previousProfileId = healthProfileId.current;
        healthProfileId.current = null;
        if (previousProfileId) void cancelProjectHealth(previousProfileId).catch(() => undefined);
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
    runtimeRequest.current += 1;
    setRuntimeSuggestions(null);
    setSelectedRuntimePorts(new Set());
    setRuntimeLoading(false);
    setRuntimeAccepting(false);
    return () => {
      runtimeRequest.current += 1;
    };
  }, [editing?.id]);

  useEffect(() => {
    environmentRequest.current += 1;
    setEnvironmentLoading(false);
    return () => {
      environmentRequest.current += 1;
    };
  }, [editing?.id]);

  useEffect(() => {
    const target = preflightTarget.current;
    const mismatch = (
      (preflightLoading && target !== null && target !== selectedId)
      || (preflight !== null && preflight.profileId !== selectedId)
    );
    if (!mismatch) return;
    // Once Continue has crossed into the backend start operation, preserve the
    // target selection until that operation settles. A profile click must not
    // make a successful start disappear or clear busy under its promise.
    if (busy && !preflightLoading && target !== null) {
      setSelectedId(target);
      return;
    }
    preflightRequest.current += 1;
    preflightTarget.current = null;
    preflightFocusReturn.current = null;
    setPreflight(null);
    setPreflightLoading(false);
    if (busy && preflightLoading) setBusy(false);
  }, [busy, preflight, preflightLoading, selectedId]);

  useEffect(() => () => {
    preflightRequest.current += 1;
    preflightTarget.current = null;
  }, []);

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
    if ((busy && !preflightLoading) || preflightStartInFlight.current) return;
    const action = routeOpenRequest(request, profiles);
    switch (action.kind) {
      case "selectProfile": {
        const hadPreflightOperation = preflightTarget.current !== null;
        preflightRequest.current += 1;
        preflightTarget.current = null;
        setPreflight(null);
        setPreflightLoading(false);
        preflightFocusReturn.current = null;
        if (hadPreflightOperation) setBusy(false);
        setSelectedId(action.profileId);
        closeEditor();
        break;
      }
      case "draftProfile": {
        // No matching profile — surface it via the create-profile draft form
        // (this app's existing affordance) instead of silently doing nothing.
        const hadPreflightOperation = preflightTarget.current !== null;
        preflightRequest.current += 1;
        preflightTarget.current = null;
        setPreflight(null);
        setPreflightLoading(false);
        preflightFocusReturn.current = null;
        if (hadPreflightOperation) setBusy(false);
        const draft = emptyProfileDraft();
        if (action.looksWindows) draft.windowsPath = action.path;
        else draft.wslPath = action.path;
        openEditor(draft);
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
    const previousProfileId = healthProfileId.current;
    healthProfileId.current = selectedId || null;
    if (previousProfileId && previousProfileId !== selectedId) {
      void cancelProjectHealth(previousProfileId).catch(() => undefined);
    }
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

  const loadRuntimeSuggestions = async () => {
    if (!editing || runtimeLoading || runtimeAccepting) return;
    const request = ++runtimeRequest.current;
    setRuntimeLoading(true);
    setError(null);
    try {
      const result = await wslRuntimeSuggestions();
      if (request !== runtimeRequest.current || !editingRef.current) return;
      setRuntimeSuggestions(result);
      setSelectedRuntimePorts(new Set());
    } catch {
      if (request === runtimeRequest.current) {
        setRuntimeSuggestions(null);
        setSelectedRuntimePorts(new Set());
        setError("WSL runtime 제안을 읽을 수 없습니다.");
      }
    } finally {
      if (request === runtimeRequest.current) setRuntimeLoading(false);
    }
  };

  const acceptRuntimePorts = async () => {
    if (!editing || runtimeLoading || runtimeAccepting || selectedRuntimePorts.size === 0) return;
    const selected = Array.from(selectedRuntimePorts).sort((left, right) => left - right);
    const request = ++runtimeRequest.current;
    setRuntimeAccepting(true);
    setError(null);
    try {
      // Re-read immediately before acceptance. Preview never grants authority
      // to a snapshot that has since expired or changed.
      const latest = await wslRuntimeSuggestions();
      if (request !== runtimeRequest.current) return;
      setRuntimeSuggestions(latest);
      const available = new Set(latest.ports.map((port) => port.published));
      if (latest.status === "expired") {
        setError("WSL runtime 제안이 만료되었습니다. WSL Desktop에서 상태를 갱신하세요.");
        return;
      }
      if (latest.status === "missing" || latest.status === "corrupt") {
        setSelectedRuntimePorts(new Set());
        setError("현재 반영할 수 있는 WSL runtime 제안이 없습니다.");
        return;
      }
      if (selected.some((port) => !available.has(port))) {
        setSelectedRuntimePorts(new Set(selected.filter((port) => available.has(port))));
        setError("WSL runtime 상태가 변경되었습니다. 제안을 다시 확인하세요.");
        return;
      }
      if (latest.status === "stale" && !window.confirm(
        `WSL runtime snapshot이 오래되었습니다. 선택한 포트 ${selected.length}개를 편집 초안에만 반영할까요? 프로필은 저장 버튼을 누르기 전까지 변경되지 않습니다.`,
      )) {
        return;
      }

      const currentDraft = editingRef.current;
      if (!currentDraft) return;
      const merged = mergeSuggestedPorts(currentDraft.expectedPortsText, selected);
      if (merged.nextText === null) {
        setError(merged.error ?? "WSL runtime 포트를 편집 초안에 반영하지 못했습니다.");
        return;
      }
      setEditing((previous) => (
        previous === currentDraft ? { ...previous, expectedPortsText: merged.nextText! } : previous
      ));
      setSelectedRuntimePorts(new Set());
    } catch {
      if (request === runtimeRequest.current) {
        setError("WSL runtime 상태를 다시 확인하지 못해 반영을 중단했습니다.");
      }
    } finally {
      if (request === runtimeRequest.current) setRuntimeAccepting(false);
    }
  };

  const onSave = async () => {
    if (!editing || saveInFlight.current || environmentLoading) return;
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
      closeEditor();
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

  const restorePreflightFocus = () => {
    preflightFocusReturn.current?.focus({ preventScroll: true });
    preflightFocusReturn.current = null;
  };

  const onStart = async (profileId: string) => {
    if (!profileId || busy || preflightLoading || preflightStartInFlight.current) return;
    if (run) {
      setError("현재 Workspace 실행을 먼저 중지하세요.");
      return;
    }
    const focused = profileContextMenu.restoreFocusTo ?? document.activeElement;
    preflightFocusReturn.current = focused instanceof HTMLElement ? focused : null;
    const request = ++preflightRequest.current;
    preflightTarget.current = profileId;
    setPreflight(null);
    setPreflightLoading(true);
    setBusy(true);
    setError(null);
    try {
      const result = await workspacePreflight(profileId);
      if (request !== preflightRequest.current) return;
      if (result.profileId !== profileId) {
        preflightTarget.current = null;
        setPreflight(null);
        setError("Workspace 사전 점검 결과가 현재 프로필과 일치하지 않습니다.");
        restorePreflightFocus();
        return;
      }
      setPreflight(result);
      if (!result.ready) setError("Workspace 사전 점검에서 시작을 차단했습니다.");
    } catch {
      if (request === preflightRequest.current) {
        preflightTarget.current = null;
        setPreflight(null);
        setError("Workspace 사전 점검을 수행할 수 없습니다.");
        restorePreflightFocus();
      }
    } finally {
      if (request === preflightRequest.current) {
        setPreflightLoading(false);
        setBusy(false);
      }
    }
  };

  const onContinueStart = async () => {
    const candidate = preflight;
    if (!candidate || !candidate.ready || busy || run || preflightStartInFlight.current) return;
    const request = preflightRequest.current;
    preflightStartInFlight.current = true;
    setStartingProfileId(candidate.profileId);
    setStartCancelRequested(false);
    startCancelRequestedRef.current = false;
    setBusy(true);
    setError(null);
    try {
      const nextRun = await startWorkspace(candidate.profileId);
      if (request !== preflightRequest.current) return;
      setRun(nextRun);
      preflightTarget.current = null;
      setPreflight(null);
      restorePreflightFocus();
    } catch {
      if (request === preflightRequest.current) {
        preflightTarget.current = null;
        setPreflight(null);
        setError(startCancelRequestedRef.current
          ? "Workspace 시작을 취소했습니다."
          : "Workspace 시작 전 상태가 변경되었습니다. 사전 점검을 다시 실행하세요.");
        restorePreflightFocus();
      }
    } finally {
      preflightStartInFlight.current = false;
      setStartingProfileId(null);
      setStartCancelRequested(false);
      startCancelRequestedRef.current = false;
      if (request === preflightRequest.current) setBusy(false);
    }
  };

  const onCancelPreflight = () => {
    if (busy) return;
    preflightRequest.current += 1;
    preflightTarget.current = null;
    setPreflight(null);
    setPreflightLoading(false);
    setError(null);
    restorePreflightFocus();
  };

  const onCancelStart = async (profileId: string) => {
    if (startingProfileId !== profileId || !busy) return;
    setStartCancelRequested(true);
    startCancelRequestedRef.current = true;
    try {
      await cancelStartWorkspace(profileId);
    } catch {
      setStartCancelRequested(false);
      startCancelRequestedRef.current = false;
      setError("Workspace 시작을 취소할 수 없습니다.");
    }
  };

  const inspectEnvironment = async () => {
    const draft = editingRef.current;
    if (!draft || busy || environmentLoading) return;
    const source = draft.environmentSource.trim();
    if (!source || (!draft.windowsPath.trim() && !draft.wslPath.trim())) {
      setError("프로젝트 경로와 환경 파일을 입력한 뒤 확인하세요.");
      return;
    }
    const request = ++environmentRequest.current;
    setEnvironmentLoading(true);
    setError(null);
    try {
      const preview: ProjectEnvironmentPreview = await previewProjectEnvironment({
        windowsPath: draft.windowsPath.trim() || null,
        wsl: draft.wslDistro.trim() && draft.wslPath.trim()
          ? { distro: draft.wslDistro.trim(), path: draft.wslPath.trim() }
          : null,
        source,
      });
      if (request !== environmentRequest.current || editingRef.current !== draft) return;
      const metadata = preview.variables.map((variable) => ({
        name: variable.name,
        source: variable.source,
        conflict: variable.conflict,
        secretReference: variable.secretReference,
      }));
      setEditing((previous) => previous === draft ? {
        ...previous,
        environmentSource: preview.source,
        environmentRevision: preview.revision,
        environmentVariables: metadata,
        environmentPreview: preview,
      } : previous);
    } catch {
      if (request === environmentRequest.current) {
        setError("환경 파일을 확인할 수 없습니다. 프로젝트 경로와 파일을 확인하세요.");
      }
    } finally {
      if (request === environmentRequest.current) setEnvironmentLoading(false);
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
  const contextPreflight = contextProfile && preflight?.profileId === contextProfile.id ? preflight : null;
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
        disabled: busy || environmentLoading || run !== null || contextPreflight !== null,
      },
      {
        type: "item",
        id: "stop",
        label: "Stop What I Started",
        disabled: busy || environmentLoading || contextRun === null,
        danger: true,
      },
      { type: "separator", id: "lifecycle-separator" },
      { type: "item", id: "edit", label: "프로필 편집", disabled: busy || environmentLoading || contextPreflight !== null },
      {
        type: "item",
        id: "delete",
        label: "삭제",
        disabled: busy || environmentLoading || contextRun !== null || contextPreflight !== null,
        danger: true,
      },
      { type: "separator", id: "path-separator" },
      {
        type: "item",
        id: "copy-path",
        label: "경로 복사",
        disabled: busy || environmentLoading || contextPreflight !== null || !contextHasPath,
      },
      {
        type: "submenu",
        id: "open-in",
        label: "다른 앱으로 열기",
        disabled: busy || environmentLoading || contextPreflight !== null || resolvedContextTargets === null || openTargetItems.length === 0,
        items: openTargetItems,
      },
    ];
  }, [busy, contextHasPath, contextPreflight, contextProfile, contextRun, environmentLoading, resolvedContextTargets, run]);

  const onProfileContextSelect = (id: string) => {
    const profile = contextProfile;
    if (!profile) return;
    if (id === "start") void onStart(profile.id);
    else if (id === "stop") void onStop(profile);
    else if (id === "edit") {
      onCancelPreflight();
      openEditor(draftFromProfile(profile));
    }
    else if (id === "delete") void onDelete(profile);
    else if (id === "copy-path") void onCopyProfilePath(profile.id);
    else {
      const target = resolvedContextTargets?.find((candidate) => `open-in:${candidate.id}` === id);
      if (target) void onOpenProfileIn(profile.id, target.id);
    }
  };

  const patch = (p: Partial<ProfileDraft>) => setEditing((prev) => (prev ? { ...prev, ...p } : prev));
  const patchProjectLocation = (p: Partial<ProfileDraft>) => {
    environmentRequest.current += 1;
    // Capture the old request ID synchronously. The native cancel may resolve
    // after a new preview has claimed its slot, but the backend exact-key
    // check then cannot cancel that newer request.
    void cancelProjectEnvironment().catch(() => undefined);
    setEnvironmentLoading(false);
    setEditing((prev) => (prev ? {
      ...prev,
      ...p,
      environmentRevision: "",
      environmentVariables: [],
      environmentPreview: null,
    } : prev));
  };
  const patchEnvironmentSource = (source: string) => patchProjectLocation({ environmentSource: source });
  const closeEditor = () => {
    environmentRequest.current += 1;
    void cancelProjectEnvironment().catch(() => undefined);
    setEnvironmentLoading(false);
    setEditing(null);
  };
  const openEditor = (draft: ProfileDraft) => {
    environmentRequest.current += 1;
    void cancelProjectEnvironment().catch(() => undefined);
    setEnvironmentLoading(false);
    setEditing(draft);
  };
  const draftValidation = editing ? validateProfileDraft(editing) : null;
  const existingRuntimePorts = new Set(
    editing ? parseExpectedPorts(editing.expectedPortsText).ports : [],
  );
  const runtimeActionable = runtimeSuggestions?.status === "fresh"
    || runtimeSuggestions?.status === "stale";

  const selectedProfile = profiles.find((profile) => profile.id === selectedId) ?? null;

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">Workbench</h1>
        <button type="button" className="btn" disabled={busy || environmentLoading} onClick={() => { onCancelPreflight(); openEditor(emptyProfileDraft()); }}>+ 프로필</button>
        <button type="button" className="btn refresh" disabled={busy || environmentLoading} onClick={() => void refresh()}>새로고침</button>
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
              onClick={() => { if (!busy || preflightLoading) setSelectedId(p.id); }}
              {...profileContextMenu.triggerProps}
            >
              <button type="button" className="profile-name" disabled={busy || environmentLoading} onClick={() => { if (!busy || preflightLoading) setSelectedId(p.id); }}>
                {p.name}
              </button>
              <button
                type="button"
                className="mini"
                disabled={busy || environmentLoading}
                onClick={() => { onCancelPreflight(); openEditor(draftFromProfile(p)); }}
                title="편집"
                aria-label={`${p.name} 프로필 편집`}
              >✏️</button>
              <button
                type="button"
                className="mini"
                disabled={busy || environmentLoading || preflight !== null || run?.profileId === p.id}
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
              aria-busy={busy || runtimeLoading || runtimeAccepting || environmentLoading}
              onKeyDown={(event) => {
                if (event.key === "Escape" && !event.nativeEvent.isComposing && !busy) {
                  event.preventDefault();
                  onCancelPreflight();
                  closeEditor();
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
                    disabled={busy || environmentLoading}
                    aria-invalid={Boolean(draftValidation?.errors.projectPath)}
                    aria-describedby={draftValidation?.errors.projectPath ? "profile-project-path-error" : undefined}
                    onChange={(e) => patchProjectLocation({ windowsPath: e.currentTarget.value })}
                  />
                </label>
                <label className="field" htmlFor="profile-wsl-distro">
                  <span>WSL distro</span>
                  <input
                    id="profile-wsl-distro"
                    value={editing.wslDistro}
                    maxLength={MAX_WSL_DISTRO_CHARS}
                    disabled={busy || environmentLoading}
                    aria-invalid={Boolean(draftValidation?.errors.wsl)}
                    aria-describedby={draftValidation?.errors.wsl ? "profile-wsl-error" : undefined}
                    onChange={(e) => patchProjectLocation({ wslDistro: e.currentTarget.value })}
                  />
                </label>
                <label className="field" htmlFor="profile-wsl-path">
                  <span>WSL 경로</span>
                  <input
                    id="profile-wsl-path"
                    value={editing.wslPath}
                    maxLength={MAX_PROFILE_PATH_BYTES}
                    disabled={busy || environmentLoading}
                    aria-invalid={Boolean(draftValidation?.errors.projectPath || draftValidation?.errors.wsl)}
                    aria-describedby={draftValidation?.errors.projectPath ? "profile-project-path-error" : draftValidation?.errors.wsl ? "profile-wsl-error" : undefined}
                    onChange={(e) => patchProjectLocation({ wslPath: e.currentTarget.value })}
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
                <fieldset
                  className="editor-section environment-editor"
                  disabled={busy}
                  aria-describedby={draftValidation?.errors.environment ? "profile-environment-error" : "profile-environment-help"}
                >
                  <legend>프로젝트 환경 (.env)</legend>
                  <p id="profile-environment-help" className="field-help">
                    프로젝트 루트의 .env 파일만 native에서 읽습니다. profile에는 파일 원문 대신
                    변수 이름·source·충돌·revision·secret reference만 저장하고, 실행 직전에 다시 확인합니다.
                  </p>
                  <label className="checkbox-field" htmlFor="profile-environment-enabled">
                    <input
                      id="profile-environment-enabled"
                      type="checkbox"
                      checked={editing.environmentEnabled}
                      disabled={environmentLoading}
                      onChange={(event) => patch({ environmentEnabled: event.currentTarget.checked })}
                    />
                    <span>Start Workspace에서 환경 주입 사용</span>
                  </label>
                  <label className="field" htmlFor="profile-environment-source">
                    <span>환경 파일 이름 (프로젝트 상대)</span>
                    <input
                      id="profile-environment-source"
                      value={editing.environmentSource}
                      maxLength={MAX_ENVIRONMENT_SOURCE_BYTES}
                      placeholder=".env"
                      disabled={environmentLoading}
                      aria-invalid={Boolean(draftValidation?.errors.environment)}
                      aria-describedby={draftValidation?.errors.environment ? "profile-environment-error" : "profile-environment-help"}
                      onChange={(event) => patchEnvironmentSource(event.currentTarget.value)}
                    />
                  </label>
                  <div className="inline-actions">
                    <button
                      type="button"
                      className="btn"
                      disabled={environmentLoading || !editing.environmentSource.trim() || (!editing.windowsPath.trim() && !editing.wslPath.trim())}
                      onClick={() => void inspectEnvironment()}
                    >
                      {environmentLoading ? "환경 파일 확인 중..." : "환경 파일 확인"}
                    </button>
                    {editing.environmentRevision && !environmentLoading ? (
                      <span className="field-help" role="status" aria-live="polite">
                        확인된 변수 {editing.environmentVariables.length}개 · 실행 시 변경 여부 재확인
                      </span>
                    ) : null}
                  </div>
                  {editing.environmentPreview ? (
                    <div className="environment-preview" role="status" aria-live="polite">
                      <strong>마스킹된 미리보기</strong>
                      {editing.environmentPreview.variables.length === 0 ? (
                        <p className="field-help">환경 변수가 없는 빈 파일입니다. 주입할 값이 없습니다.</p>
                      ) : (
                        <div className="environment-variable-list" aria-label="마스킹된 환경 변수 미리보기">
                          {editing.environmentPreview.variables.map((variable) => (
                            <div className="environment-variable-row" key={`${variable.name}-${variable.source}`}>
                              <span className="environment-variable-name">{variable.name}</span>
                              <span className="environment-variable-value" aria-label="마스킹된 환경 변수 값">{variable.maskedValue || "(empty)"}</span>
                              <span className="environment-variable-source">{variable.source}</span>
                              {variable.secretReference ? <span className="environment-variable-secret">secret reference</span> : null}
                              {variable.conflict !== "none" ? <span className="environment-variable-conflict">충돌: {variable.conflict}</span> : null}
                            </div>
                          ))}
                        </div>
                      )}
                      {editing.environmentPreview.hasConflicts ? (
                        <p className="field-error" role="alert">중복 또는 예약된 환경 변수 이름이 있어 주입할 수 없습니다.</p>
                      ) : null}
                    </div>
                  ) : editing.environmentVariables.length > 0 ? (
                    <p className="field-help">저장된 metadata가 있습니다. 원문 없이 다시 확인하면 마스킹된 미리보기를 표시합니다.</p>
                  ) : null}
                  {draftValidation?.errors.environment && (
                    <span id="profile-environment-error" className="field-error" role="alert">{draftValidation.errors.environment}</span>
                  )}
                </fieldset>
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
                <fieldset className="editor-section runtime-suggestions" disabled={busy}>
                  <legend>WSL runtime 포트 제안</legend>
                  <p className="field-help">
                    WSL Desktop이 마지막으로 발행한 read-only snapshot만 읽습니다. WSL·Docker를
                    실행하거나 컨테이너를 변경하지 않으며, 반영한 포트도 저장 전 편집 초안에만 남습니다.
                  </p>
                  <button
                    type="button"
                    className="btn"
                    disabled={runtimeLoading || runtimeAccepting}
                    onClick={() => void loadRuntimeSuggestions()}
                  >
                    {runtimeLoading ? "제안 읽는 중..." : runtimeSuggestions ? "제안 새로고침" : "제안 불러오기"}
                  </button>
                  {runtimeSuggestions ? (
                    <div className="runtime-suggestion-result">
                      <div className={`runtime-suggestion-status status-${runtimeSuggestions.status}`} role="status" aria-live="polite">
                        <strong>{RUNTIME_STATUS_LABEL[runtimeSuggestions.status]}</strong>
                        {runtimeSuggestions.producerVersion && runtimeSuggestions.freshnessMs !== null ? (
                          <span>
                            {runtimeSuggestions.source} · producer {runtimeSuggestions.producerVersion} · {formatRuntimeFreshness(runtimeSuggestions.freshnessMs)}
                          </span>
                        ) : (
                          <span>{runtimeSuggestions.source}</span>
                        )}
                      </div>
                      {runtimeSuggestions.ports.length > 0 ? (
                        <div className="runtime-port-list" aria-label="WSL runtime 포트 후보">
                          {runtimeSuggestions.ports.map((port) => {
                            const alreadyRegistered = existingRuntimePorts.has(port.published);
                            const selected = selectedRuntimePorts.has(port.published);
                            return (
                              <label className="runtime-port-row" key={port.published}>
                                <input
                                  type="checkbox"
                                  checked={alreadyRegistered || selected}
                                  disabled={alreadyRegistered || !runtimeActionable || runtimeLoading || runtimeAccepting}
                                  onChange={(event) => {
                                    const checked = event.currentTarget.checked;
                                    setSelectedRuntimePorts((previous) => {
                                      const next = new Set(previous);
                                      if (checked) next.add(port.published);
                                      else next.delete(port.published);
                                      return next;
                                    });
                                  }}
                                  aria-label={`published port ${port.published} 선택`}
                                />
                                <span className="runtime-port-number">host {port.published}</span>
                                {alreadyRegistered ? <span className="runtime-port-existing">이미 등록됨</span> : null}
                                <ul>
                                  {port.sources.map((source) => (
                                    <li key={`${source.distro}\u0000${source.container}\u0000${source.target}\u0000${source.protocol}`}>
                                      {source.distro} · {source.container} ({source.containerState}) · target {source.target}/{source.protocol}
                                    </li>
                                  ))}
                                </ul>
                              </label>
                            );
                          })}
                        </div>
                      ) : runtimeActionable ? (
                        <p className="field-help">발행된 host 포트 후보가 없습니다.</p>
                      ) : null}
                      <button
                        type="button"
                        className="btn primary"
                        disabled={!runtimeActionable || runtimeLoading || runtimeAccepting || selectedRuntimePorts.size === 0}
                        onClick={() => void acceptRuntimePorts()}
                      >
                        {runtimeAccepting ? "상태 재확인 중..." : "선택 포트를 초안에 반영"}
                      </button>
                    </div>
                  ) : null}
                </fieldset>
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
                  <button type="submit" className="btn primary" disabled={busy || environmentLoading || !draftValidation?.profile}>저장</button>
                  <button type="button" className="btn" disabled={busy} onClick={() => { onCancelPreflight(); closeEditor(); }}>취소</button>
                </div>
              </form>
            </section>
          ) : selectedProfile ? (
            <section className="panel">
              <h2>{selectedProfile.name}</h2>
              <div className="row-actions">
                <button
                  className="btn primary"
                  disabled={busy || run !== null || preflight?.profileId === selectedProfile.id}
                  onClick={() => void onStart(selectedProfile.id)}
                >
                  {startingProfileId === selectedProfile.id ? "Workspace 시작 중…" : "Start Workspace"}
                </button>
                {startingProfileId === selectedProfile.id && (
                  <button
                    className="btn danger"
                    disabled={!busy || startCancelRequested}
                    onClick={() => void onCancelStart(selectedProfile.id)}
                  >
                    {startCancelRequested ? "취소 요청 중…" : "시작 취소"}
                  </button>
                )}
                {run?.profileId === selectedProfile.id && (
                  <button className="btn danger" disabled={busy} onClick={() => void onStop(selectedProfile)}>
                  Stop What I Started
                  </button>
                )}
              </div>

              {preflight?.profileId === selectedProfile.id && (
                <div
                  className={`preflight-dialog ${preflight.ready ? "ready" : "blocked"}`}
                  role="dialog"
                  aria-modal="true"
                  aria-labelledby="workspace-preflight-title"
                  aria-describedby="workspace-preflight-description"
                  onKeyDown={(event) => {
                    if (event.key === "Escape" && !event.nativeEvent.isComposing && !busy) {
                      event.preventDefault();
                      onCancelPreflight();
                    }
                  }}
                >
                  <h3 id="workspace-preflight-title">Start Workspace 사전 점검</h3>
                  <p id="workspace-preflight-description" className="field-help">
                    실행 전에 읽기 전용으로 확인한 결과입니다. 경고는 기존 resource를 유지한 채 계속할 수 있고,
                    차단·확인 불가 항목이 있으면 어떤 앱도 시작하지 않습니다.
                  </p>
                  <div className="preflight-list" aria-label="Workspace 사전 점검 결과">
                    {preflight.items.map((item) => (
                      <div className={`preflight-row status-${item.status}`} key={item.key}>
                        <div className="preflight-row-heading">
                          <strong>{PREFLIGHT_ITEM_LABEL[item.key] ?? item.key}</strong>
                          <span className="preflight-status">{PREFLIGHT_STATUS_LABEL[item.status]}</span>
                        </div>
                        <span className="preflight-detail">{item.detail}</span>
                        {item.resources.length > 0 && (
                          <ul className="preflight-resources">
                            {item.resources.map((resource) => (
                              <li key={`${resource.kind}:${resource.id}`}>
                                {resource.id} · {RESOURCE_STATE_LABEL[resource.state]}
                              </li>
                            ))}
                          </ul>
                        )}
                      </div>
                    ))}
                  </div>
                  {!preflight.ready && (
                    <div className="field-error form-error" role="alert">
                      차단된 항목을 해결한 뒤 사전 점검을 다시 실행하세요.
                    </div>
                  )}
                  <div className="actions">
                    <button
                      type="button"
                      className="btn primary"
                      autoFocus={preflight.ready}
                      disabled={!preflight.ready || busy}
                      onClick={() => void onContinueStart()}
                    >
                      {busy ? "Workspace 시작 중…" : "계속 시작"}
                    </button>
                    <button type="button" className="btn" autoFocus={!preflight.ready} disabled={busy} onClick={onCancelPreflight}>취소</button>
                  </div>
                </div>
              )}

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
                      <span className="health-detail">{PREFLIGHT_STATUS_LABEL[step.status]} · {step.detail}</span>
                    </div>
                  ))}
                  {run.resourceProvenance.length > 0 && (
                    <>
                      <h3 className="subtitle">Resource ownership</h3>
                      {run.resourceProvenance.map((resource) => (
                        <div key={`${resource.kind}:${resource.id}`} className="health-row">
                          <span className="health-name">{resource.id}</span>
                          <span className="health-detail">{RESOURCE_STATE_LABEL[resource.state]}</span>
                        </div>
                      ))}
                    </>
                  )}
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
