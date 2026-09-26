import { useOperation } from "@devbox/hooks";
import { isProductHosted } from "../../transport";
import { useRef } from "react";
import { closeSession, deleteWorkspaceProfile, startSession, retrySession, saveWorkspaceProfile } from "../api";
import { makeId } from "../lib/id";
import { normalizePaneSizing } from "../lib/paneSizing";
import { pushRecentPath } from "../lib/storage";
import { nextTabTitle } from "../lib/tabTitle";
import {
  isSafeWorkspacePath,
  isRestoreOnly,
  normalizeProfile,
  saveLastWorkspace,
  startCommandError,
  workspaceFromRuntime,
} from "../lib/workspace";
import { orderWorkspacePanes, RESTORE_START_CONCURRENCY, runWithConcurrencyLimit } from "../lib/workspaceRestore";
import type { MultiplexerKind, Pane, Tab, WorkspaceDefinition, WorkspaceProfile } from "../types";
import type * as React from "react";

interface Props {
  workspaceLoadingRef: React.RefObject<boolean>;
  logLensBusyRef: React.RefObject<string | null>;
  setError: React.Dispatch<React.SetStateAction<string | null>>;
  cwd: string;
  startCommand: string;
  ask: import("../components/AppDialog").AskDialog;
  multiplexer: import("../types").MultiplexerKind;
  workspaceRestoreGeneration: React.RefObject<number>;
  panesRef: React.RefObject<import("../types").Pane[]>;
  stateRef: React.RefObject<{ tabs: import("../types").Tab[]; activeTabId: string; activePaneId: string | null }>;
  tabs: import("../types").Tab[];
  setPanes: React.Dispatch<React.SetStateAction<import("../types").Pane[]>>;
  setTabs: React.Dispatch<React.SetStateAction<import("../types").Tab[]>>;
  setActiveTabId: React.Dispatch<React.SetStateAction<string>>;
  setActivePaneId: React.Dispatch<React.SetStateAction<string | null>>;
  layoutSaveTimer: React.RefObject<number | undefined>;
  setRecentPaths: React.Dispatch<React.SetStateAction<string[]>>;
  pinned: boolean;
  setCwd: React.Dispatch<React.SetStateAction<string>>;
  refreshDashboard: (force?: boolean) => Promise<void>;
  mountedRef: React.RefObject<boolean>;
  setSelected: React.Dispatch<React.SetStateAction<string>>;
  RESTORE_FAILURE_MESSAGE: "터미널을 복원하지 못했습니다. 설정을 확인한 뒤 다시 시도하세요.";
  contextActionBusyRef: React.RefObject<boolean>;
  setBroadcastOn: React.Dispatch<React.SetStateAction<boolean>>;
  setBroadcastTargetIds: React.Dispatch<React.SetStateAction<Set<string>>>;
  setBroadcastPickerOpen: React.Dispatch<React.SetStateAction<boolean>>;
  setContextBusy: (value: boolean) => void;
  profiles: import("../types").WorkspaceProfile[];
  panes: import("../types").Pane[];
  activeTabId: string;
  activePaneId: string | null;
  setProfiles: React.Dispatch<React.SetStateAction<import("../types").WorkspaceProfile[]>>;
}

export function useWorkspaceLaunch({
  workspaceLoadingRef,
  logLensBusyRef,
  setError,
  cwd,
  startCommand,
  ask,
  multiplexer,
  workspaceRestoreGeneration,
  panesRef,
  stateRef,
  tabs,
  setPanes,
  setTabs,
  setActiveTabId,
  setActivePaneId,
  layoutSaveTimer,
  setRecentPaths,
  pinned,
  setCwd,
  refreshDashboard,
  mountedRef,
  setSelected,
  RESTORE_FAILURE_MESSAGE,
  contextActionBusyRef,
  setBroadcastOn,
  setBroadcastTargetIds,
  setBroadcastPickerOpen,
  setContextBusy,
  profiles,
  panes,
  activeTabId,
  activePaneId,
  setProfiles,
}: Props) {
  const { busy: workspaceLoading, run: runWorkspace } = useOperation();
  /** tabId가 null이면 새 탭을, 아니면 그 탭에 분할 팬을 추가한다. 세션 시작이
   * 성공해야만 탭/팬을 만든다 — "탭은 항상 팬을 최소 1개 갖는다" 불변식의 근거.
   *
   * cwdOverride가 있으면 cwd 입력칸 대신 그 경로를 연다 — applink `path` 타깃(§1.4)이
   * 쓴다. 입력칸에 사용자가 입력 중이던 값은 건드리지 않는다(끝의 조건부 setCwd 참고). */
  const startInTab = async (
    tabId: string | null,
    distro: string,
    cwdOverride?: string,
    safeFailureMessage?: string,
    options?: {
      paneKey?: string;
      startCommand?: string | null;
      multiplexer?: MultiplexerKind;
    },
  ): Promise<boolean> => {
    if (workspaceLoadingRef.current || logLensBusyRef.current !== null) return false;
    if (!distro.trim()) {
      setError("사용 가능한 WSL 배포판이 없습니다.");
      return false;
    }
    setError(null);
    const usedCwd = (cwdOverride ?? cwd).trim() || undefined;
    const usedStartCommand =
      (options?.startCommand === undefined ? startCommand : options.startCommand)?.trim() || undefined;
    if (usedStartCommand) {
      const commandError = startCommandError(usedStartCommand);
      if (commandError) {
        setError(commandError);
        return false;
      }
      const approved = await ask({
        kind: "confirm",
        title: `다음 시작 명령을 '${distro}' 터미널에서 실행할까요?`,
        detail: usedStartCommand,
        confirmLabel: "실행",
        danger: true,
      });
      if (!approved.confirmed) return false;
    }
    const key = options?.paneKey ?? makeId("p");
    const requestedMultiplexer = options?.multiplexer ?? multiplexer;
    if (isProductHosted()) {
      return (
        (await runWorkspace(async () => {
          const generation = ++workspaceRestoreGeneration.current;
          workspaceLoadingRef.current = true;

          const activeTab = tabId ?? makeId("t");
          const placeholder: Pane = {
            key,
            sessionId: null,
            distro,
            cwd: usedCwd,
            startCommand: usedStartCommand,
            initialCommand: usedStartCommand,
            multiplexer: requestedMultiplexer,
            requestedMultiplexer,
            restoreStatus: "connecting",
          };
          const nextPanes = [...panesRef.current, placeholder];
          const nextTabs: Tab[] =
            tabId === null
              ? [
                  ...stateRef.current.tabs,
                  {
                    id: activeTab,
                    title: nextTabTitle(
                      tabs.map((tab) => tab.title),
                      distro,
                    ),
                    customTitle: false,
                    layout: "grid",
                    paneIds: [key],
                    sizing: normalizePaneSizing(undefined, "grid", 1),
                  },
                ]
              : stateRef.current.tabs.map((tab) =>
                  tab.id === tabId
                    ? {
                        ...tab,
                        paneIds: [...tab.paneIds, key],
                        sizing: normalizePaneSizing(undefined, tab.layout, tab.paneIds.length + 1),
                      }
                    : tab,
                );
          panesRef.current = nextPanes;
          stateRef.current = { tabs: nextTabs, activeTabId: activeTab, activePaneId: key };
          setPanes(nextPanes);
          setTabs(nextTabs);
          setActiveTabId(activeTab);
          setActivePaneId(key);
          window.clearTimeout(layoutSaveTimer.current);
          try {
            const plan = workspaceFromRuntime(nextTabs, nextPanes, activeTab, key);
            if (!plan) throw new Error("invalid terminal layout");
            await saveLastWorkspace(plan);
            const started = await startSession(distro, usedCwd, key, requestedMultiplexer);
            if (!adoptRestoredSession(key, requestedMultiplexer, started, generation)) return false;
            if (usedCwd) setRecentPaths(pushRecentPath(usedCwd));
            if (cwdOverride === undefined && !pinned) setCwd("");
            void refreshDashboard(true).catch(() => undefined);
            return true;
          } catch {
            markRestoreFailed(key, generation);
            setError(safeFailureMessage ?? "터미널을 시작하지 못했습니다. 해당 자리에서 상태를 확인해 주세요.");
            return false;
          } finally {
            if (workspaceRestoreGeneration.current === generation) {
              workspaceLoadingRef.current = false;
            }
          }
        })) ?? false
      );
    }
    try {
      const started = await startSession(distro, usedCwd, key, requestedMultiplexer);
      const id = started.sessionId;
      setPanes((prev) => {
        const next = [
          ...prev,
          {
            key,
            sessionId: id,
            distro,
            cwd: usedCwd,
            startCommand: usedStartCommand,
            initialCommand: started.resumed ? undefined : usedStartCommand,
            multiplexer: started.multiplexer,
            requestedMultiplexer,
            resumed: started.resumed,
          },
        ];
        panesRef.current = next;
        return next;
      });

      if (tabId === null) {
        const title = nextTabTitle(
          tabs.map((t) => t.title),
          distro,
        );
        const newTabId = makeId("t");
        setTabs((prev) => [
          ...prev,
          {
            id: newTabId,
            title,
            customTitle: false,
            layout: "grid",
            paneIds: [id],
            sizing: normalizePaneSizing(undefined, "grid", 1),
          },
        ]);
        setActiveTabId(newTabId);
      } else {
        setTabs((prev) =>
          prev.map((tab) => {
            if (tab.id !== tabId) return tab;
            const paneIds = [...tab.paneIds, id];
            return { ...tab, paneIds, sizing: normalizePaneSizing(undefined, tab.layout, paneIds.length) };
          }),
        );
      }
      setActivePaneId(id);

      if (usedCwd) setRecentPaths(pushRecentPath(usedCwd));
      if (cwdOverride === undefined && !pinned) setCwd("");
      // A newly attached/started PTY changes the session half of the shared dashboard
      // generation. Refresh in the background; the terminal itself does not wait on WSL/Docker
      // polling and broadcast remains disabled until the new generation is fresh.
      void refreshDashboard(true).catch(() => undefined);
      return true;
    } catch {
      // Native PTY errors may contain the requested cwd, executable path or OS details. Keep
      // those implementation details out of the renderer; callers that need context provide a
      // fixed feature-specific message.
      setError(safeFailureMessage ?? "터미널을 시작하지 못했습니다.");
      return false;
    }
  };

  const adoptRestoredSession = (
    key: string,
    requestedMultiplexer: MultiplexerKind,
    started: Awaited<ReturnType<typeof startSession>>,
    generation: number,
  ): boolean => {
    if (!mountedRef.current || workspaceRestoreGeneration.current !== generation) return false;
    const placeholder = panesRef.current.find(
      (pane) => pane.key === key && pane.sessionId === null && pane.restoreStatus === "connecting",
    );
    if (!placeholder) return false;
    const restoredPane: Pane = {
      ...placeholder,
      sessionId: started.sessionId,
      initialCommand: started.resumed ? undefined : placeholder.initialCommand,
      multiplexer: started.multiplexer,
      requestedMultiplexer,
      resumed: started.resumed,
      restoreStatus: undefined,
      restoreError: undefined,
    };
    const restoredPanes = panesRef.current.map((pane) => (pane.key === key ? restoredPane : pane));
    panesRef.current = restoredPanes;
    setPanes(restoredPanes);

    const current = stateRef.current;
    const restoredTabs = current.tabs.map((tab) => ({
      ...tab,
      paneIds: tab.paneIds.map((id) => (id === key ? started.sessionId : id)),
    }));
    const restoredActivePane = current.activePaneId === key ? started.sessionId : current.activePaneId;
    stateRef.current = { ...current, tabs: restoredTabs, activePaneId: restoredActivePane };
    setTabs(restoredTabs);
    setActivePaneId(restoredActivePane);
    if (restoredActivePane === started.sessionId) setSelected(placeholder.distro);
    return true;
  };

  const markRestoreFailed = (key: string, generation: number): boolean => {
    if (!mountedRef.current || workspaceRestoreGeneration.current !== generation) return false;
    let changed = false;
    const failedPanes = panesRef.current.map((pane) => {
      if (pane.key !== key || pane.sessionId !== null) return pane;
      changed = true;
      return { ...pane, restoreStatus: "failed" as const, restoreError: RESTORE_FAILURE_MESSAGE };
    });
    if (!changed) return false;
    panesRef.current = failedPanes;
    setPanes(failedPanes);
    return true;
  };

  const launchWorkspace = async (
    workspace: WorkspaceDefinition,
    options: { replaceExisting: boolean; label: string },
  ): Promise<boolean> => {
    if (workspaceLoadingRef.current || contextActionBusyRef.current || logLensBusyRef.current !== null) return false;
    const oldSessionIds = panesRef.current.flatMap((pane) => (pane.sessionId ? [pane.sessionId] : []));
    if (options.replaceExisting && oldSessionIds.length > 0) {
      const switched = await ask({
        kind: "confirm",
        title: `'${options.label}' 프로필로 전환할까요?`,
        lines: [`현재 터미널 ${oldSessionIds.length}개가 닫힙니다.`],
        confirmLabel: "전환",
        danger: true,
      });
      if (!switched.confirmed) return false;
    }

    const commands = workspace.panes.flatMap((pane) =>
      pane.startCommand ? [`[${pane.distro} · ${pane.key}] ${pane.startCommand}`] : [],
    );
    const runStartCommands =
      !isRestoreOnly() &&
      (commands.length === 0 ||
        (
          await ask({
            kind: "confirm",
            title: `시작 명령 ${commands.length}개를 실행할까요?`,
            lines: ["취소하면 레이아웃만 엽니다."],
            detail: commands.join("\n"),
            confirmLabel: "실행",
            cancelLabel: "레이아웃만 열기",
            danger: true,
          })
        ).confirmed);

    return (
      (await runWorkspace(async () => {
        if (isProductHosted()) {
          workspaceLoadingRef.current = true;

          window.clearTimeout(layoutSaveTimer.current);
          try {
            if (options.replaceExisting) {
              // Explicit profile replacement retires these exact old PTYs before a
              // stable pane key can refer to the new profile definition.
              for (const sessionId of oldSessionIds) await closeSession(sessionId);
            }
            await saveLastWorkspace(workspace);
          } catch {
            workspaceLoadingRef.current = false;

            setError("레이아웃을 준비하거나 이전 터미널을 종료하지 못했습니다. 실행 상태를 확인해 주세요.");
            return false;
          }
        }
        const generation = ++workspaceRestoreGeneration.current;
        workspaceLoadingRef.current = true;

        setError(null);
        const nextPanes: Pane[] = workspace.panes.map((definition) => ({
          key: definition.key,
          sessionId: null,
          distro: definition.distro,
          cwd: definition.cwd ?? undefined,
          startCommand: definition.startCommand ?? undefined,
          initialCommand: runStartCommands ? (definition.startCommand ?? undefined) : undefined,
          multiplexer: definition.multiplexer,
          requestedMultiplexer: definition.multiplexer,
          restoreStatus: "connecting",
        }));
        const nextTabs: Tab[] = workspace.tabs.map((definition) => ({
          id: definition.id,
          title: definition.title,
          customTitle: definition.customTitle,
          layout: definition.layout,
          paneIds: [...definition.paneKeys],
          sizing: normalizePaneSizing(definition.sizing, definition.layout, definition.paneKeys.length),
        }));
        const nextActiveTab = nextTabs.find((tab) => tab.id === workspace.activeTabId) ?? nextTabs[0];
        const requestedActivePane =
          workspace.activePaneKey && nextActiveTab.paneIds.includes(workspace.activePaneKey)
            ? workspace.activePaneKey
            : (nextActiveTab.paneIds[0] ?? null);

        // Render the complete topology before starting PTYs. A failed start therefore replaces its
        // connecting card in place instead of deleting the pane and collapsing adjacent tracks.
        stateRef.current = {
          tabs: nextTabs,
          activeTabId: nextActiveTab.id,
          activePaneId: requestedActivePane,
        };
        panesRef.current = nextPanes;
        setPanes(nextPanes);
        setTabs(nextTabs);
        setActiveTabId(nextActiveTab.id);
        setActivePaneId(requestedActivePane);
        setBroadcastOn(false);
        setBroadcastTargetIds(new Set());
        setBroadcastPickerOpen(false);
        const activeDefinition = workspace.panes.find((pane) => pane.key === requestedActivePane);
        if (activeDefinition) setSelected(activeDefinition.distro);

        let failed = 0;
        let startedCount = 0;
        try {
          const startDefinition = async (definition: WorkspaceDefinition["panes"][number]): Promise<void> => {
            if (isProductHosted() && (!mountedRef.current || workspaceRestoreGeneration.current !== generation)) return;
            try {
              const started = await startSession(
                definition.distro,
                definition.cwd ?? undefined,
                definition.key,
                definition.multiplexer,
              );
              if (!adoptRestoredSession(definition.key, definition.multiplexer, started, generation)) {
                if (!isProductHosted()) await closeSession(started.sessionId).catch(() => undefined);
                return;
              }
              startedCount += 1;
            } catch {
              if (markRestoreFailed(definition.key, generation)) failed += 1;
            }
          };

          const restorePlan = orderWorkspacePanes(workspace);
          // The active pane is started alone so the user's primary shell becomes interactive first.
          await startDefinition(restorePlan.active);
          await runWithConcurrencyLimit(restorePlan.remaining, RESTORE_START_CONCURRENCY, startDefinition);

          const closeResults = await Promise.allSettled(
            (isProductHosted() ? [] : oldSessionIds).map((id) => closeSession(id)),
          );
          const closeFailed = closeResults.filter((result) => result.status === "rejected").length;
          if (failed > 0 || closeFailed > 0) {
            const details = [
              failed > 0 ? `복원 실패 ${failed}개(자리에서 재시도 가능)` : "",
              closeFailed > 0 ? `이전 세션 닫기 실패 ${closeFailed}개` : "",
            ]
              .filter(Boolean)
              .join(" · ");
            setError(
              `${startedCount > 0 ? "프로필을 부분적으로 열었습니다." : "프로필 레이아웃만 복원했습니다."} ${details}`,
            );
          }
          void refreshDashboard(true).catch(() => undefined);
          return true;
        } finally {
          if (workspaceRestoreGeneration.current === generation) {
            workspaceLoadingRef.current = false;
          }
        }
      })) ?? false
    );
  };

  const retryWorkspacePane = async (key: string): Promise<void> => {
    if (workspaceLoadingRef.current || contextActionBusyRef.current || logLensBusyRef.current !== null) return;
    const placeholder = panesRef.current.find(
      (pane) => pane.key === key && pane.sessionId === null && pane.restoreStatus === "failed",
    );
    if (!placeholder) return;
    const generation = workspaceRestoreGeneration.current;
    const requestedMultiplexer = placeholder.requestedMultiplexer ?? placeholder.multiplexer;
    const connectingPanes = panesRef.current.map((pane) =>
      pane.key === key ? { ...pane, restoreStatus: "connecting" as const, restoreError: undefined } : pane,
    );
    panesRef.current = connectingPanes;
    setPanes(connectingPanes);
    setContextBusy(true);
    setError(null);
    try {
      const started = await (isProductHosted() ? retrySession : startSession)(
        placeholder.distro,
        placeholder.cwd,
        placeholder.key,
        requestedMultiplexer,
      );
      if (!adoptRestoredSession(key, requestedMultiplexer, started, generation)) {
        if (!isProductHosted()) await closeSession(started.sessionId).catch(() => undefined);
        return;
      }
      void refreshDashboard(true).catch(() => undefined);
    } catch {
      if (markRestoreFailed(key, generation)) {
        setError("터미널 복원 재시도에 실패했습니다. 실패한 자리는 그대로 유지됩니다.");
      }
    } finally {
      if (mountedRef.current && workspaceRestoreGeneration.current === generation) {
        setContextBusy(false);
      }
    }
  };

  const launchWorkspaceRef = useRef(launchWorkspace);
  launchWorkspaceRef.current = launchWorkspace;
  const startInTabRef = useRef(startInTab);
  startInTabRef.current = startInTab;

  const openProfile = async (profile: WorkspaceProfile): Promise<boolean> =>
    launchWorkspace(profile, { replaceExisting: true, label: profile.name });

  const openProfileById = async (id: string): Promise<boolean> => {
    const profile = profiles.find((item) => item.id === id);
    if (!profile) {
      setError("요청한 터미널 프로필을 찾을 수 없습니다.");
      return false;
    }
    return openProfile(profile);
  };

  const saveCurrentProfile = async (): Promise<void> => {
    if (workspaceLoadingRef.current) return;
    if (panes.some((pane) => pane.cwd && !isSafeWorkspacePath(pane.cwd))) {
      setError("안전한 절대 경로가 아닌 cwd가 있어 프로필을 저장할 수 없습니다.");
      return;
    }
    const workspace = workspaceFromRuntime(tabs, panes, activeTabId, activePaneId);
    if (!workspace) {
      setError("저장할 터미널 레이아웃이 없습니다.");
      return;
    }
    const input = await ask({
      kind: "prompt",
      title: "현재 터미널 레이아웃의 프로필 이름",
      inputLabel: "프로필 이름",
      defaultValue: "새 터미널 프로필",
      maxLength: 120,
      confirmLabel: "저장",
    });
    if (!input.confirmed) return;
    const name = input.value.trim();
    if (!name) {
      setError("프로필 이름은 비워둘 수 없습니다.");
      return;
    }
    workspaceLoadingRef.current = true;
    await runWorkspace(async () => {
      setError(null);
      try {
        const saved = await saveWorkspaceProfile({ id: "", name, ...workspace });
        const normalized = normalizeProfile(saved);
        if (!normalized) throw new Error("invalid profile response");
        setProfiles((previous) => [...previous.filter((profile) => profile.id !== normalized.id), normalized]);
      } catch {
        setError("터미널 프로필을 저장하지 못했습니다.");
      } finally {
        workspaceLoadingRef.current = false;
      }
    });
  };

  const requestDeleteProfile = async (profile: WorkspaceProfile): Promise<void> => {
    if (workspaceLoadingRef.current || logLensBusyRef.current !== null) return;
    const confirmed = await ask({
      kind: "confirm",
      title: `'${profile.name}' 터미널 프로필을 삭제할까요?`,
      lines: ["실행 중인 터미널은 닫히지 않습니다."],
      confirmLabel: "삭제",
      danger: true,
    });
    if (!confirmed.confirmed) return;
    workspaceLoadingRef.current = true;
    await runWorkspace(async () => {
      setError(null);
      try {
        await deleteWorkspaceProfile(profile.id);
        setProfiles((previous) => previous.filter((item) => item.id !== profile.id));
      } catch {
        setError("터미널 프로필을 삭제하지 못했습니다.");
      } finally {
        workspaceLoadingRef.current = false;
      }
    });
  };
  return {
    workspaceLoading,
    startInTab,
    openProfileById,
    startInTabRef,
    launchWorkspaceRef,
    saveCurrentProfile,
    openProfile,
    requestDeleteProfile,
    retryWorkspacePane,
  };
}
