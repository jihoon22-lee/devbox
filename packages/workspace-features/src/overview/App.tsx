import { ProfileTemplateManager } from "./components/ProfileTemplateManager";
import { ProjectWizard } from "./components/ProjectWizard";
import { ProfileDetails } from "./components/ProfileDetails";
import { ProfileEditor } from "./components/ProfileEditor";
import { DIALOG_FOCUSABLE_SELECTOR } from "./lib/profilePresentation";
import { useOperation } from "@devbox/hooks";
import { ContextMenu, useContextMenu, type ContextMenuEntry } from "@devbox/context-menu";
import { isKeyboardActivation } from "@devbox/a11y";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  createProfile,
  cancelDependencyHealth,
  cancelStartWorkspace,
  cancelProjectEnvironment,
  cancelProjectHealth,
  cancelWorkspacePreflight,
  currentWorkspaceRun,
  createProfileFromTemplate,
  createProfileTemplate,
  deleteProfile,
  deleteProfileTemplate,
  dependencyHealth,
  listProfiles,
  listProfileTemplates,
  onOpenRequest,
  openProfileIn,
  profileCopyPath,
  projectHealth,
  retryWorkspace,
  profileOpenTargets,
  startWorkspace,
  stopWorkspace,
  takePendingOpen,
  updateProfile,
  updateProfileTemplate,
  previewProjectEnvironment,
  workspacePreflight,
  wslRuntimeSuggestions,
  type OpenRequest,
  type ProjectEnvironmentPreview,
  type ProjectHealth,
  type ProjectProfile,
  type ProfileTemplate,
  type ProfileTemplateSnapshot,
  type WorkspaceRun,
  type WorkspaceRunOwnership,
  type WorkspacePreflight,
  type WorkbenchOpenTarget,
  type RuntimeSuggestions,
} from "./api";
import { routeOpenRequest } from "./lib/applink";
import {
  draftFromProfile,
  emptyProfileDraft,
  parseExpectedPorts,
  validateProfileDraft,
  type ProfileDraft,
} from "./lib/profileEditor";
import { mergeSuggestedPorts } from "./lib/runtimeSuggestions";
import WorkspaceTaskControlPanel from "./components/WorkspaceTaskControlPanel";
import {
  emptyProfileTemplateDraft,
  profileDraftFromTemplate,
  templateDraftFromTemplate,
  validateProfileTemplateDraft,
  type ProfileTemplateDraft,
} from "./lib/profileTemplateEditor";
import "./App.css";

export default function App() {
  const [profiles, setProfiles] = useState<ProjectProfile[]>([]);
  const [templates, setTemplates] = useState<ProfileTemplate[]>([]);
  const [templateRevision, setTemplateRevision] = useState("");
  const [editing, setEditing] = useState<ProfileDraft | null>(null);
  const [templateDialog, setTemplateDialog] = useState<"wizard" | "manage" | null>(null);
  const [wizardDraft, setWizardDraft] = useState<ProfileDraft | null>(null);
  const [wizardTemplateId, setWizardTemplateId] = useState<string>("");
  const [templateEditing, setTemplateEditing] = useState<ProfileTemplateDraft | null>(null);
  const [templateError, setTemplateError] = useState<string | null>(null);
  const { busy: templateBusy, run: runTemplateOperation } = useOperation();
  const [selectedId, setSelectedId] = useState<string>("");
  const [health, setHealth] = useState<ProjectHealth | null>(null);
  const [dependencyStatus, setDependencyStatus] = useState<WorkspacePreflight | null>(null);
  const [dependencyLoading, setDependencyLoading] = useState(false);
  const [run, setRun] = useState<WorkspaceRun | null>(null);
  const [preflight, setPreflight] = useState<WorkspacePreflight | null>(null);
  const [preflightLoading, setPreflightLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [preflightBusy, setPreflightBusy] = useState(false);
  const { busy: profileBusy, run: runProfileOperation } = useOperation();
  const busy = preflightBusy || profileBusy;
  const [runtimeSuggestions, setRuntimeSuggestions] = useState<RuntimeSuggestions | null>(null);
  const [selectedRuntimePorts, setSelectedRuntimePorts] = useState<Set<number>>(new Set());
  const [runtimeLoading, setRuntimeLoading] = useState(false);
  const [runtimeAccepting, setRuntimeAccepting] = useState(false);
  const [environmentLoading, setEnvironmentLoading] = useState(false);
  const [startingProfileId, setStartingProfileId] = useState<string | null>(null);
  const [startCancelRequested, setStartCancelRequested] = useState(false);
  const [retrying, setRetrying] = useState(false);
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
  const dependencyRequest = useRef(0);
  const selectedIdRef = useRef(selectedId);
  selectedIdRef.current = selectedId;
  const saveInFlight = useRef(false);
  const preflightRequest = useRef(0);
  const preflightTarget = useRef<string | null>(null);
  const preflightFocusReturn = useRef<HTMLElement | null>(null);
  const preflightStartInFlight = useRef(false);
  const runtimeRequest = useRef(0);
  const environmentRequest = useRef(0);
  const templateRequest = useRef(0);
  const templateDialogRef = useRef<HTMLElement | null>(null);
  const templateFocusReturn = useRef<HTMLElement | null>(null);
  const editingRef = useRef(editing);
  editingRef.current = editing;
  const [profilesRevision, setProfilesRevision] = useState(0);
  // Flips true once the first listProfiles() resolves (success or failure).
  // Gates applink handling (below) so a `path` target is matched against the
  // real profile list instead of racing the empty initial state.
  const [profilesLoaded, setProfilesLoaded] = useState(false);

  const prepareProfileContext = useCallback(
    (target: HTMLElement) => {
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
    },
    [busy, preflightLoading, profiles],
  );
  const profileContextMenu = useContextMenu({
    disabled: busy && !preflightLoading,
    onBeforeOpen: (_reason, target) => prepareProfileContext(target),
  });
  // The profile row contains several nested buttons, so keep the context-menu
  // event handlers on the row without applying menu ARIA state to a generic
  // container. The nested buttons remain independently accessible controls.
  const profileContextTrigger = profileContextMenu.triggerProps;

  const refresh = useCallback(async () => {
    const hadPreflightOperation = preflightTarget.current !== null;
    const previousPreflightTarget = preflightTarget.current;
    if (previousPreflightTarget) {
      void cancelWorkspacePreflight(previousPreflightTarget).catch(() => undefined);
    }
    preflightRequest.current += 1;
    preflightTarget.current = null;
    preflightFocusReturn.current = null;
    setPreflight(null);
    setPreflightLoading(false);
    if (hadPreflightOperation) setPreflightBusy(false);
    // Invalidate read-only health surfaces before the profile list request
    // starts. Otherwise a late result from the previous snapshot can briefly
    // overwrite the refreshed profile's empty/loading state.
    healthRequest.current += 1;
    const previousHealthProfileId = healthProfileId.current;
    healthProfileId.current = null;
    if (previousHealthProfileId) {
      void cancelProjectHealth(previousHealthProfileId).catch(() => undefined);
    }
    dependencyRequest.current += 1;
    const previousSelectedId = selectedIdRef.current;
    if (previousSelectedId) {
      void cancelDependencyHealth(previousSelectedId).catch(() => undefined);
    }
    setHealth(null);
    setDependencyStatus(null);
    setDependencyLoading(false);
    const request = ++refreshRequest.current;
    try {
      const [list, activeRun] = await Promise.all([listProfiles(), currentWorkspaceRun()]);
      if (request !== refreshRequest.current) return;
      setProfiles(list);
      setRun(activeRun ? { ...activeRun, steps: [], resourceProvenance: [] } : null);
      setSelectedId((prev) => (prev && list.some((p) => p.id === prev) ? prev : (list[0]?.id ?? "")));
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

  const loadTemplates = useCallback(async (): Promise<ProfileTemplateSnapshot | null> => {
    const request = ++templateRequest.current;
    setTemplateError(null);
    try {
      const loaded = await listProfileTemplates();
      if (request !== templateRequest.current) return null;
      setTemplates(loaded.templates);
      setTemplateRevision(loaded.revision);
      return loaded;
    } catch {
      if (request === templateRequest.current) {
        // A failed refresh must not leave an older template list actionable in
        // a newly opened dialog. Keep only the fixed error state visible.
        setTemplates([]);
        setTemplateRevision("");
        setTemplateError("프로필 템플릿을 불러올 수 없습니다.");
      }
      return request === templateRequest.current ? { revision: "", templates: [] } : null;
    }
  }, []);

  const openProjectWizard = async () => {
    if (busy || templateBusy) return;
    templateFocusReturn.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    return runTemplateOperation(async () => {
      setTemplateDialog("wizard");
      setWizardDraft(profileDraftFromTemplate(null));
      setWizardTemplateId("");
      setTemplateError(null);

      const loadRequest = templateRequest.current + 1;
      const loaded = await loadTemplates();
      if (loadRequest !== templateRequest.current) return;
      if (loaded && loaded.templates.length > 0) {
        setWizardTemplateId(loaded.templates[0].id);
        setWizardDraft(profileDraftFromTemplate(loaded.templates[0]));
      }
    });
  };

  const openTemplateManager = async () => {
    if (busy || templateBusy) return;
    templateFocusReturn.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    return runTemplateOperation(async () => {
      setTemplateDialog("manage");
      setTemplateError(null);
      setTemplateEditing(null);

      const loadRequest = templateRequest.current + 1;
      const loaded = await loadTemplates();
      if (loadRequest !== templateRequest.current) return;
      setTemplateEditing(
        loaded && loaded.templates[0] ? templateDraftFromTemplate(loaded.templates[0]) : emptyProfileTemplateDraft(),
      );
    });
  };

  const closeTemplateDialog = () => {
    templateRequest.current += 1;
    const returnTarget = templateFocusReturn.current;
    templateFocusReturn.current = null;
    setTemplateDialog(null);
    setWizardDraft(null);
    setTemplateEditing(null);
    setTemplateError(null);
    if (returnTarget) {
      let attempts = 0;
      const restore = () => {
        if (attempts++ >= 20) return;
        if (!returnTarget.isConnected) return;
        if (returnTarget instanceof HTMLButtonElement && returnTarget.disabled) {
          window.setTimeout(restore, 25);
          return;
        }
        returnTarget.focus({ preventScroll: true });
      };
      window.setTimeout(restore, 0);
    }
  };

  useEffect(() => {
    if (!templateDialog || templateBusy) return;
    const dialog = templateDialogRef.current;
    if (!dialog) return;
    // Do not steal focus from a field while the user edits it. Initial focus
    // is only needed when the dialog becomes available or focus has escaped
    // the dialog after an async template load.
    if (dialog.contains(document.activeElement)) return;
    const first = dialog.querySelector<HTMLElement>(DIALOG_FOCUSABLE_SELECTOR);
    first?.focus({ preventScroll: true });
  }, [templateBusy, templateDialog]);

  useEffect(
    () => () => {
      templateRequest.current += 1;
    },
    [],
  );

  const selectWizardTemplate = (templateId: string) => {
    setWizardTemplateId(templateId);
    const template = templates.find((candidate) => candidate.id === templateId);
    setWizardDraft((previous) => {
      const defaults = profileDraftFromTemplate(template ?? null);
      // “직접 입력” is a mode switch, not a reset action. Keep values the
      // user already entered when the empty option is selected.
      if (!previous) return defaults;
      if (!template) return previous;
      return {
        ...previous,
        windowsPath: previous.windowsPath.trim() ? previous.windowsPath : defaults.windowsPath,
        wslDistro: previous.wslDistro.trim() ? previous.wslDistro : defaults.wslDistro,
        wslPath: previous.wslPath.trim() ? previous.wslPath : defaults.wslPath,
        gitRoot: previous.gitRoot.trim() ? previous.gitRoot : defaults.gitRoot,
        expectedPortsText: previous.expectedPortsText.trim() ? previous.expectedPortsText : defaults.expectedPortsText,
        serviceRows: previous.serviceRows.length > 0 ? previous.serviceRows : defaults.serviceRows,
      };
    });
  };

  // Read SyntheticEvent values before scheduling a functional state update.
  // React may clear currentTarget by the time the updater executes.
  const patchWizardDraft = (changes: Partial<ProfileDraft>) => {
    setWizardDraft((previous) => (previous ? { ...previous, ...changes } : previous));
  };
  const patchTemplateDraft = (changes: Partial<ProfileTemplateDraft>) => {
    setTemplateEditing((previous) => (previous ? { ...previous, ...changes } : previous));
  };

  const onCreateWizardProfile = async () => {
    if (!wizardDraft || templateBusy) return;
    const validation = validateProfileDraft(wizardDraft);
    const profile = validation.profile;
    if (!profile) {
      setTemplateError("프로젝트 입력값을 확인한 뒤 생성하세요.");
      return;
    }
    return runTemplateOperation(async () => {
      setTemplateError(null);
      try {
        await createProfileFromTemplate(wizardTemplateId || null, profile);
        closeTemplateDialog();
        await refresh();
      } catch {
        setTemplateError("프로젝트 프로필을 생성할 수 없습니다. 입력과 경로를 확인하세요.");
      }
    });
  };

  const onSaveTemplate = async () => {
    if (!templateEditing || templateBusy) return;
    const operationRequest = templateRequest.current;
    const validation = validateProfileTemplateDraft(templateEditing);
    const template = validation.template;
    if (!template) {
      setTemplateError("템플릿 입력값을 확인한 뒤 저장하세요.");
      return;
    }
    return runTemplateOperation(async () => {
      setTemplateError(null);
      try {
        if (template.id) {
          await updateProfileTemplate(template, templateRevision);
        } else {
          const created = await createProfileTemplate(template);
          setTemplateEditing(templateDraftFromTemplate(created));
        }
        const loaded = await loadTemplates();
        if (loaded && template.id) {
          setTemplateEditing(
            templateDraftFromTemplate(loaded.templates.find((candidate) => candidate.id === template.id) ?? template),
          );
        }
      } catch {
        if (operationRequest === templateRequest.current) {
          setTemplateError("프로필 템플릿을 저장할 수 없습니다.");
        }
      }
    });
  };

  const onDeleteTemplate = async (templateId: string) => {
    const template = templates.find((candidate) => candidate.id === templateId);
    if (!template || templateBusy) return;
    if (!window.confirm(`'${template.name}' 템플릿을 삭제할까요? 기존 프로젝트 프로필은 변경하지 않습니다.`)) return;
    const operationRequest = templateRequest.current;
    return runTemplateOperation(async () => {
      setTemplateError(null);
      try {
        await deleteProfileTemplate(templateId, templateRevision);
        const loaded = await loadTemplates();
        if (loaded) {
          setTemplateEditing(
            loaded.templates[0] ? templateDraftFromTemplate(loaded.templates[0]) : emptyProfileTemplateDraft(),
          );
        }
      } catch {
        if (operationRequest === templateRequest.current) {
          setTemplateError("프로필 템플릿을 삭제할 수 없습니다.");
        }
      }
    });
  };

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: editing.id invalidates pending runtime suggestions when the editor changes; removing it allows results from the previous profile.
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

  // biome-ignore lint/correctness/useExhaustiveDependencies: editing.id invalidates pending environment suggestions when the editor changes; removing it allows results from the previous profile.
  useEffect(() => {
    environmentRequest.current += 1;
    setEnvironmentLoading(false);
    return () => {
      environmentRequest.current += 1;
    };
  }, [editing?.id]);

  useEffect(() => {
    const target = preflightTarget.current;
    const mismatch =
      (preflightLoading && target !== null && target !== selectedId) ||
      (preflight !== null && preflight.profileId !== selectedId);
    if (!mismatch) return;
    // Once Continue has crossed into the backend start operation, preserve the
    // target selection until that operation settles. A profile click must not
    // make a successful start disappear or clear busy under its promise.
    if (busy && !preflightLoading && target !== null) {
      setSelectedId(target);
      return;
    }
    if (target) void cancelWorkspacePreflight(target).catch(() => undefined);
    preflightRequest.current += 1;
    preflightTarget.current = null;
    preflightFocusReturn.current = null;
    setPreflight(null);
    setPreflightLoading(false);
    if (busy && preflightLoading) setPreflightBusy(false);
  }, [busy, preflight, preflightLoading, selectedId]);

  useEffect(
    () => () => {
      const target = preflightTarget.current;
      if (target) void cancelWorkspacePreflight(target).catch(() => undefined);
      preflightRequest.current += 1;
      preflightTarget.current = null;
    },
    [],
  );

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
        const previousPreflightTarget = preflightTarget.current;
        if (previousPreflightTarget) {
          void cancelWorkspacePreflight(previousPreflightTarget).catch(() => undefined);
        }
        preflightRequest.current += 1;
        preflightTarget.current = null;
        setPreflight(null);
        setPreflightLoading(false);
        preflightFocusReturn.current = null;
        if (hadPreflightOperation) setPreflightBusy(false);
        setSelectedId(action.profileId);
        closeEditor();
        break;
      }
      case "draftProfile": {
        // No matching profile — surface it via the create-profile draft form
        // (this app's existing affordance) instead of silently doing nothing.
        const hadPreflightOperation = preflightTarget.current !== null;
        const previousPreflightTarget = preflightTarget.current;
        if (previousPreflightTarget) {
          void cancelWorkspacePreflight(previousPreflightTarget).catch(() => undefined);
        }
        preflightRequest.current += 1;
        preflightTarget.current = null;
        setPreflight(null);
        setPreflightLoading(false);
        preflightFocusReturn.current = null;
        if (hadPreflightOperation) setPreflightBusy(false);
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

  // biome-ignore lint/correctness/useExhaustiveDependencies: profilesRevision invalidates native health after profile edits even when selectedId is unchanged.
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

  // biome-ignore lint/correctness/useExhaustiveDependencies: profilesRevision invalidates dependency health after profile edits even when selectedId is unchanged.
  useEffect(() => {
    const request = ++dependencyRequest.current;
    if (!selectedId) {
      setDependencyStatus(null);
      setDependencyLoading(false);
      return () => {
        dependencyRequest.current += 1;
        void cancelDependencyHealth(selectedId).catch(() => undefined);
      };
    }
    setDependencyStatus(null);
    setDependencyLoading(true);
    void dependencyHealth(selectedId)
      .then((result) => {
        if (request === dependencyRequest.current && result.profileId === selectedId) {
          setDependencyStatus(result);
        }
      })
      .catch(() => {
        if (request === dependencyRequest.current) {
          setDependencyStatus(null);
          setError("의존성 상태를 확인할 수 없습니다.");
        }
      })
      .finally(() => {
        if (request === dependencyRequest.current) setDependencyLoading(false);
      });
    return () => {
      dependencyRequest.current += 1;
      void cancelDependencyHealth(selectedId).catch(() => undefined);
    };
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
      if (
        latest.status === "stale" &&
        !window.confirm(
          `WSL runtime snapshot이 오래되었습니다. 선택한 포트 ${selected.length}개를 편집 초안에만 반영할까요? 프로필은 저장 버튼을 누르기 전까지 변경되지 않습니다.`,
        )
      ) {
        return;
      }

      const currentDraft = editingRef.current;
      if (!currentDraft) return;
      const merged = mergeSuggestedPorts(currentDraft.expectedPortsText, selected);
      if (merged.nextText === null) {
        setError(merged.error ?? "WSL runtime 포트를 편집 초안에 반영하지 못했습니다.");
        return;
      }
      setEditing((previous) =>
        previous === currentDraft ? { ...previous, expectedPortsText: merged.nextText! } : previous,
      );
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
    const profile = validation.profile;
    if (!profile) {
      setError("프로필 입력값을 확인한 뒤 저장하세요.");
      return;
    }
    saveInFlight.current = true;
    return runProfileOperation(async () => {
      setError(null);
      try {
        if (profile.id) {
          await updateProfile(profile);
        } else {
          await createProfile(profile);
        }
        closeEditor();
        await refresh();
      } catch {
        // Backend errors are deliberately not echoed: path strings and future
        // service metadata must not become UI/telemetry output by accident.
        setError("프로필을 저장할 수 없습니다. 입력과 경로를 확인하세요.");
      } finally {
        saveInFlight.current = false;
      }
    });
  };

  const onDelete = async (profile: ProjectProfile) => {
    if (run?.profileId === profile.id) {
      setError("실행 중인 프로필은 먼저 Workbench가 시작한 리소스를 중지하세요.");
      return;
    }
    if (
      !window.confirm(
        `'${profile.name}' 프로필을 삭제할까요? 저장된 프로필 정의만 삭제하며 프로젝트 파일과 이미 실행 중이던 외부 리소스는 변경하지 않습니다.`,
      )
    )
      return;
    return runProfileOperation(async () => {
      setError(null);
      try {
        await deleteProfile(profile.id);
        setSelectedId("");
        setHealth(null);
        await refresh();
      } catch {
        setError("프로필을 삭제할 수 없습니다.");
      }
    });
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
    setPreflightBusy(true);
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
        setPreflightBusy(false);
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
    setPreflightBusy(true);
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
        setError(
          startCancelRequestedRef.current
            ? "Workspace 시작을 취소했습니다."
            : "Workspace 시작 전 상태가 변경되었습니다. 사전 점검을 다시 실행하세요.",
        );
        restorePreflightFocus();
      }
    } finally {
      preflightStartInFlight.current = false;
      setStartingProfileId(null);
      setStartCancelRequested(false);
      startCancelRequestedRef.current = false;
      if (request === preflightRequest.current) setPreflightBusy(false);
    }
  };

  const onCancelPreflight = () => {
    if (busy && !preflightLoading) return;
    const target = preflightTarget.current;
    if (target) void cancelWorkspacePreflight(target).catch(() => undefined);
    preflightRequest.current += 1;
    preflightTarget.current = null;
    setPreflight(null);
    setPreflightLoading(false);
    if (busy) setPreflightBusy(false);
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
        wsl:
          draft.wslDistro.trim() && draft.wslPath.trim()
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
      setEditing((previous) =>
        previous === draft
          ? {
              ...previous,
              environmentSource: preview.source,
              environmentRevision: preview.revision,
              environmentVariables: metadata,
              environmentPreview: preview,
            }
          : previous,
      );
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
    if (
      !window.confirm(
        `'${profile.name}'에서 Workbench가 시작한 리소스만 중지할까요? 시작 전부터 실행 중이던 리소스는 유지됩니다.`,
      )
    )
      return;
    return runProfileOperation(async () => {
      setError(null);
      try {
        const n = await stopWorkspace(run.runId, profile.id);
        // The backend retains a run when any owned PID could not be safely
        // terminated (for example, an identity/access race). Re-read ownership
        // before clearing the UI so a failed Stop remains actionable instead of
        // becoming stale and invisible until the next full refresh.
        let remaining: WorkspaceRunOwnership | null = null;
        try {
          remaining = await currentWorkspaceRun();
        } catch {
          // A failed ownership read is not proof that the processes are gone.
          remaining = run;
        }
        if (remaining && remaining.runId === run.runId && remaining.profileId === profile.id) {
          setRun(run);
          setError("일부 Workbench 프로세스를 안전하게 종료하지 못했습니다. 내가 시작한 작업 중지를 다시 시도하세요.");
        } else if (remaining) {
          // A mismatched backend run is an invariant violation. Keep the local
          // run visible and fail closed rather than replacing it with unrelated
          // ownership metadata.
          setRun(run);
          setError("Workspace 실행 소유권이 변경되어 중지를 완료하지 못했습니다.");
        } else {
          setRun(null);
          if (n > 0) setError(`Workbench가 시작한 프로세스 ${n}개를 종료했습니다.`);
        }
      } catch {
        setError("Workspace 실행을 중지할 수 없습니다.");
      }
    });
  };

  const onRetry = async (profile: ProjectProfile) => {
    // `canRetry` is computed by the backend's canonical bounded planner. A
    // failed step alone is not sufficient: an unknown/migrated step must not
    // become a renderer-side command selector.
    const retryAllowed = run?.canRetry === true;
    if (!run || run.profileId !== profile.id || retrying || busy || !retryAllowed) {
      setError("다시 시도할 실패 단계를 찾을 수 없습니다.");
      return;
    }
    setRetrying(true);
    return runProfileOperation(async () => {
      setError(null);
      try {
        const nextRun = await retryWorkspace(run.runId, profile.id);
        setRun(nextRun);
        if (nextRun.canRetry) {
          setError("실패한 단계가 남아 있습니다. 같은 실행에서 다시 시도할 수 있습니다.");
        }
      } catch {
        setError("Workspace 재시도를 완료하지 못했습니다. 기존 실행 소유권은 유지됩니다.");
      } finally {
        setRetrying(false);
      }
    });
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
    return runProfileOperation(async () => {
      setError(null);
      try {
        await openProfileIn(profileId, appId);
      } catch {
        setError("선택한 앱으로 프로필을 열 수 없습니다.");
      }
    });
  };

  const resolvedContextTargets =
    contextProfile && contextTargets?.profileId === contextProfile.id ? contextTargets.targets : null;
  const contextRun = contextProfile && run?.profileId === contextProfile.id ? run : null;
  const contextPreflight = contextProfile && preflight?.profileId === contextProfile.id ? preflight : null;
  const contextHasPath = Boolean(contextProfile?.windowsPath?.trim() || contextProfile?.wsl?.path.trim());
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
        label: "Workspace 시작",
        disabled: busy || environmentLoading || run !== null || contextPreflight !== null,
      },
      {
        type: "item",
        id: "stop",
        label: "내가 시작한 작업 중지",
        disabled: busy || environmentLoading || contextRun === null,
        danger: true,
      },
      {
        type: "item",
        id: "retry",
        label: "실패 단계부터 다시 시도",
        disabled: busy || environmentLoading || contextRun === null || contextRun.canRetry !== true,
      },
      { type: "separator", id: "lifecycle-separator" },
      {
        type: "item",
        id: "edit",
        label: "프로필 편집",
        disabled: busy || environmentLoading || contextPreflight !== null,
      },
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
        disabled:
          busy ||
          environmentLoading ||
          contextPreflight !== null ||
          resolvedContextTargets === null ||
          openTargetItems.length === 0,
        items: openTargetItems,
      },
    ];
  }, [
    busy,
    contextHasPath,
    contextPreflight,
    contextProfile,
    contextRun,
    environmentLoading,
    resolvedContextTargets,
    run,
  ]);

  const onProfileContextSelect = (id: string) => {
    const profile = contextProfile;
    if (!profile) return;
    if (id === "start") void onStart(profile.id);
    else if (id === "stop") void onStop(profile);
    else if (id === "retry") void onRetry(profile);
    else if (id === "edit") {
      onCancelPreflight();
      openEditor(draftFromProfile(profile));
    } else if (id === "delete") void onDelete(profile);
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
    setEditing((prev) =>
      prev
        ? {
            ...prev,
            ...p,
            environmentRevision: "",
            environmentVariables: [],
            environmentPreview: null,
          }
        : prev,
    );
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
  const existingRuntimePorts = new Set(editing ? parseExpectedPorts(editing.expectedPortsText).ports : []);
  const runtimeActionable = runtimeSuggestions?.status === "fresh" || runtimeSuggestions?.status === "stale";

  const selectedProfile = profiles.find((profile) => profile.id === selectedId) ?? null;
  const wizardValidation = wizardDraft ? validateProfileDraft(wizardDraft) : null;
  const templateValidation = templateEditing ? validateProfileTemplateDraft(templateEditing) : null;

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">Workbench</h1>
        <button
          type="button"
          className="btn"
          disabled={busy || environmentLoading || templateBusy}
          onClick={() => {
            onCancelPreflight();
            openEditor(emptyProfileDraft());
          }}
        >
          + 프로필
        </button>
        <button
          type="button"
          className="btn"
          disabled={busy || environmentLoading || templateBusy}
          onClick={() => void openProjectWizard()}
        >
          새 프로젝트 마법사
        </button>
        <button
          type="button"
          className="btn"
          disabled={busy || environmentLoading || templateBusy}
          onClick={() => void openTemplateManager()}
        >
          템플릿 관리
        </button>
        <button
          type="button"
          className="btn refresh"
          disabled={busy || environmentLoading}
          onClick={() => void refresh()}
        >
          새로고침
        </button>
      </header>

      {error && (
        <div className="error" role="alert" aria-live="assertive">
          {error}
        </div>
      )}

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
              onClick={() => {
                if (!busy || preflightLoading) setSelectedId(p.id);
              }}
              onContextMenu={profileContextTrigger.onContextMenu}
              onKeyDown={(event) => {
                profileContextTrigger.onKeyDown?.(event);
                if (event.defaultPrevented || event.target !== event.currentTarget || !isKeyboardActivation(event))
                  return;
                event.preventDefault();
                if (!busy || preflightLoading) setSelectedId(p.id);
              }}
            >
              <button
                type="button"
                className="profile-name"
                disabled={busy || environmentLoading}
                onClick={() => {
                  if (!busy || preflightLoading) setSelectedId(p.id);
                }}
              >
                {p.name}
              </button>
              <button
                type="button"
                className="mini"
                disabled={busy || environmentLoading}
                onClick={() => {
                  onCancelPreflight();
                  openEditor(draftFromProfile(p));
                }}
                title="편집"
                aria-label={`${p.name} 프로필 편집`}
              >
                ✏️
              </button>
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
          <WorkspaceTaskControlPanel disabled={busy || environmentLoading || templateBusy} />
          {editing ? (
            <ProfileEditor
              busy={busy}
              runtimeLoading={runtimeLoading}
              runtimeAccepting={runtimeAccepting}
              environmentLoading={environmentLoading}
              onCancelPreflight={onCancelPreflight}
              closeEditor={closeEditor}
              editing={editing}
              onSave={onSave}
              draftValidation={draftValidation}
              patch={patch}
              patchProjectLocation={patchProjectLocation}
              patchEnvironmentSource={patchEnvironmentSource}
              inspectEnvironment={inspectEnvironment}
              loadRuntimeSuggestions={loadRuntimeSuggestions}
              runtimeSuggestions={runtimeSuggestions}
              existingRuntimePorts={existingRuntimePorts}
              selectedRuntimePorts={selectedRuntimePorts}
              runtimeActionable={runtimeActionable}
              setSelectedRuntimePorts={setSelectedRuntimePorts}
              acceptRuntimePorts={acceptRuntimePorts}
            />
          ) : selectedProfile ? (
            <ProfileDetails
              selectedProfile={selectedProfile}
              busy={busy}
              run={run}
              preflight={preflight}
              onStart={onStart}
              startingProfileId={startingProfileId}
              startCancelRequested={startCancelRequested}
              onCancelStart={onCancelStart}
              retrying={retrying}
              onRetry={onRetry}
              onStop={onStop}
              preflightLoading={preflightLoading}
              preflightTarget={preflightTarget}
              onCancelPreflight={onCancelPreflight}
              onContinueStart={onContinueStart}
              health={health}
              dependencyLoading={dependencyLoading}
              dependencyRequest={dependencyRequest}
              setDependencyStatus={setDependencyStatus}
              setDependencyLoading={setDependencyLoading}
              setError={setError}
              dependencyStatus={dependencyStatus}
            />
          ) : (
            <div className="empty">왼쪽에서 프로필을 선택하세요.</div>
          )}
        </main>
      </div>
      {templateDialog === "wizard" && wizardDraft && (
        <div className="template-dialog-backdrop">
          <ProjectWizard
            templateDialogRef={templateDialogRef}
            templateBusy={templateBusy}
            closeTemplateDialog={closeTemplateDialog}
            wizardTemplateId={wizardTemplateId}
            selectWizardTemplate={selectWizardTemplate}
            templates={templates}
            onCreateWizardProfile={onCreateWizardProfile}
            wizardDraft={wizardDraft}
            wizardValidation={wizardValidation}
            patchWizardDraft={patchWizardDraft}
            templateError={templateError}
          />
        </div>
      )}
      {templateDialog === "manage" && templateEditing && (
        <div className="template-dialog-backdrop">
          <ProfileTemplateManager
            templateDialogRef={templateDialogRef}
            templateBusy={templateBusy}
            closeTemplateDialog={closeTemplateDialog}
            setTemplateEditing={setTemplateEditing}
            templates={templates}
            onDeleteTemplate={onDeleteTemplate}
            onSaveTemplate={onSaveTemplate}
            templateEditing={templateEditing}
            templateValidation={templateValidation}
            patchTemplateDraft={patchTemplateDraft}
            templateError={templateError}
          />
        </div>
      )}
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
