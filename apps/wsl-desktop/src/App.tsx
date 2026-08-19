import { useCallback, useEffect, useRef, useState } from "react";
import {
  closeSession,
  dockerAction,
  dockerPs,
  getWindowsBuildNumber,
  listDistros,
  onOpenRequest,
  onTerminalClosed,
  onTerminalOutput,
  startSession,
  takePendingOpen,
} from "./api";
import DistroPanel from "./components/DistroPanel";
import PaneCanvas from "./components/PaneCanvas";
import TabBar from "./components/TabBar";
import { routeOpenRequest } from "./lib/applink";
import { makeId } from "./lib/id";
import { matchShortcut, type ShortcutAction } from "./lib/shortcuts";
import { loadPinned, loadPinnedCwd, loadRecentPaths, pushRecentPath, savePinned, savePinnedCwd } from "./lib/storage";
import { nextTabTitle } from "./lib/tabTitle";
import type { ContainerInfo, DistroInfo, Layout, OpenRequest, Pane, Tab } from "./types";
import "./App.css";

export default function App() {
  const [distros, setDistros] = useState<DistroInfo[]>([]);
  const [selected, setSelected] = useState("");
  const [containers, setContainers] = useState<ContainerInfo[]>([]);
  const [dockerMissing, setDockerMissing] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [panelOpen, setPanelOpen] = useState(true);
  const [pinned, setPinned] = useState<boolean>(loadPinned);
  const [cwd, setCwd] = useState<string>(() => (loadPinned() ? loadPinnedCwd() : ""));
  const [recentPaths, setRecentPaths] = useState<string[]>(loadRecentPaths);
  const [panes, setPanes] = useState<Pane[]>([]);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeTabId, setActiveTabId] = useState<string>("");
  const [activePaneId, setActivePaneId] = useState<string | null>(null);
  const [broadcastOn, setBroadcastOn] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // TermPane must not create xterm until this one-time lookup resolves, so
  // every terminal receives its final ConPTY build-number option.
  const [windowsBuildNumber, setWindowsBuildNumber] = useState<number | null | undefined>(undefined);
  // Flips true once the first listDistros() resolves. Gates applink handling
  // (below) so a `path` target has a real default distro to open into,
  // rather than racing the empty initial `selected` state.
  const [distrosLoaded, setDistrosLoaded] = useState(false);
  const writes = useRef(new Map<string, (data: string) => void>());

  // onTerminalClosed 구독은 마운트 시 한 번만 걸린다(아래 effect, deps []). 그 콜백이
  // dropPane을 부를 때 tabs/activeTabId/activePaneId를 직접 클로저로 참조하면 마운트
  // 시점의 낡은 값에 고정된다. 매 렌더마다 최신 값을 채우는 ref로 우회한다.
  const stateRef = useRef({ tabs, activeTabId, activePaneId });
  stateRef.current = { tabs, activeTabId, activePaneId };

  const registerWrite = useCallback((id: string, fn: (data: string) => void) => {
    writes.current.set(id, fn);
  }, []);
  const unregisterWrite = useCallback((id: string) => {
    writes.current.delete(id);
  }, []);

  useEffect(() => {
    void listDistros().then((ds) => {
      setDistros(ds);
      setSelected((prev) => prev || ds.find((d) => d.default)?.name || ds[0]?.name || "Ubuntu");
      setDistrosLoaded(true);
    });
  }, []);

  useEffect(() => {
    void getWindowsBuildNumber()
      .then(setWindowsBuildNumber)
      .catch(() => setWindowsBuildNumber(null));
  }, []);

  const refreshDashboard = useCallback(async () => {
    const ds = await listDistros();
    setDistros(ds);
    const distro = ds.find((d) => d.default)?.name ?? ds[0]?.name ?? "Ubuntu";
    setSelected((prev) => prev || distro);
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
  }, []);

  useEffect(() => {
    void refreshDashboard().catch(() => undefined);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onDockerAction = async (id: string, action: "start" | "stop" | "restart") => {
    setBusy(`${id}:${action}`);
    try {
      await dockerAction(selected, id, action);
      setContainers(await dockerPs(selected));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };

  const openDistroTerminal = (name: string) => {
    setSelected(name);
    void startInTab(null, name);
  };

  // 팬 하나(세션)를 상태에서 제거한다. 마지막 팬이면 소속 탭도 함께 닫는다.
  // stateRef를 통해서만 tabs/activeTabId/activePaneId를 읽는다 — 위 주석 참고.
  const dropPane = useCallback((paneId: string) => {
    const { tabs: curTabs, activeTabId: curActiveTabId, activePaneId: curActivePaneId } = stateRef.current;
    setPanes((prev) => prev.filter((p) => p.sessionId !== paneId));

    const ownerIdx = curTabs.findIndex((t) => t.paneIds.includes(paneId));
    if (ownerIdx === -1) {
      setActivePaneId((prev) => (prev === paneId ? null : prev));
      return;
    }
    const owner = curTabs[ownerIdx];
    const remaining = owner.paneIds.filter((id) => id !== paneId);
    const tabClosed = remaining.length === 0;

    const nextTabs = tabClosed
      ? curTabs.filter((t) => t.id !== owner.id)
      : curTabs.map((t) => (t.id === owner.id ? { ...t, paneIds: remaining } : t));
    setTabs(nextTabs);

    if (tabClosed && curActiveTabId === owner.id) {
      const fallback = nextTabs[Math.min(ownerIdx, nextTabs.length - 1)] ?? null;
      setActiveTabId(fallback ? fallback.id : "");
      setActivePaneId(fallback ? (fallback.paneIds[fallback.paneIds.length - 1] ?? null) : null);
    } else if (curActivePaneId === paneId) {
      setActivePaneId(remaining[remaining.length - 1] ?? null);
    }
  }, []);

  useEffect(() => {
    const unsubs: (() => void)[] = [];
    void onTerminalOutput(({ session_id, data }) => {
      writes.current.get(session_id)?.(data);
    }).then((u) => unsubs.push(u));
    void onTerminalClosed(({ session_id }) => {
      // 세션이 (셸 종료 등으로) 스스로 끝났을 때. close_session 호출은 필요 없다.
      dropPane(session_id);
      writes.current.delete(session_id);
    }).then((u) => unsubs.push(u));
    return () => unsubs.forEach((u) => u());
  }, [dropPane]);

  // Inbound cross-app open requests (§3). Redefined every render so it always
  // closes over the latest tabs/activeTabId/selected/startInTab — the
  // devbox://open listener below is set up once and lives for the app's
  // lifetime, so without this a relaunch long after mount would act on
  // stale state (same hazard the stateRef comment above documents for
  // onTerminalClosed).
  const handleOpenRequest = (request: OpenRequest) => {
    const action = routeOpenRequest(request);
    switch (action.kind) {
      case "openTerminal":
        // repo-manager는 저장소의 Windows 경로를 보내는데(연동 설계 §0.1) 이
        // 터미널은 WSL이다. 경로는 `--cd`의 별도 argv 값으로 그대로 전달되므로
        // 입력 경로를 프론트에서 변환하지 않는다.
        void startInTab(tabs.length === 0 ? null : activeTabId, selected, action.path);
        break;
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
  // distrosLoaded so `selected` already has a real default distro.
  useEffect(() => {
    if (!distrosLoaded) return;
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
  }, [distrosLoaded]);

  // 핀이 켜져 있는 동안은 cwd가 바뀔 때마다 localStorage에 저장한다 (핀을 막 켠
  // 순간도 pinned가 deps에 있어 여기서 함께 처리된다).
  useEffect(() => {
    if (pinned) savePinnedCwd(cwd);
  }, [pinned, cwd]);

  const togglePinned = () => {
    setPinned((prev) => {
      const next = !prev;
      savePinned(next);
      return next;
    });
  };

  /** tabId가 null이면 새 탭을, 아니면 그 탭에 분할 팬을 추가한다. 세션 시작이
   * 성공해야만 탭/팬을 만든다 — "탭은 항상 팬을 최소 1개 갖는다" 불변식의 근거.
   *
   * cwdOverride가 있으면 cwd 입력칸 대신 그 경로를 연다 — applink `path` 타깃(§1.4)이
   * 쓴다. 입력칸에 사용자가 입력 중이던 값은 건드리지 않는다(끝의 조건부 setCwd 참고). */
  const startInTab = async (tabId: string | null, distro: string, cwdOverride?: string) => {
    setError(null);
    const usedCwd = (cwdOverride ?? cwd).trim() || undefined;
    try {
      const id = await startSession(distro, usedCwd);
      const key = makeId("p");
      setPanes((prev) => [...prev, { key, sessionId: id, distro, cwd: usedCwd }]);

      if (tabId === null) {
        const title = nextTabTitle(tabs.map((t) => t.title), distro);
        const newTabId = makeId("t");
        setTabs((prev) => [...prev, { id: newTabId, title, layout: "grid", paneIds: [id] }]);
        setActiveTabId(newTabId);
      } else {
        setTabs((prev) => prev.map((t) => (t.id === tabId ? { ...t, paneIds: [...t.paneIds, id] } : t)));
      }
      setActivePaneId(id);

      if (usedCwd) setRecentPaths(pushRecentPath(usedCwd));
      if (cwdOverride === undefined && !pinned) setCwd("");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const openNewTab = () => startInTab(null, selected);
  // 툴바 "+ Terminal"과 Ctrl+Shift+D는 같은 동작이다: 활성 탭이 있으면 분할 추가,
  // 없으면(앱을 막 띄운 직후) 새 탭을 만든다.
  const addPane = () => startInTab(tabs.length === 0 ? null : activeTabId, selected);

  const closePane = (paneId: string) => {
    void closeSession(paneId);
    dropPane(paneId);
  };

  const closeTab = (tabId: string) => {
    const idx = tabs.findIndex((t) => t.id === tabId);
    if (idx === -1) return;
    const tab = tabs[idx];
    tab.paneIds.forEach((id) => void closeSession(id));
    setPanes((prev) => prev.filter((p) => p.sessionId === null || !tab.paneIds.includes(p.sessionId)));
    const nextTabs = tabs.filter((t) => t.id !== tabId);
    setTabs(nextTabs);
    if (activeTabId === tabId) {
      const fallback = nextTabs[Math.min(idx, nextTabs.length - 1)] ?? null;
      setActiveTabId(fallback ? fallback.id : "");
      setActivePaneId(fallback ? (fallback.paneIds[fallback.paneIds.length - 1] ?? null) : null);
    }
  };

  const activateTab = (tabId: string) => {
    setActiveTabId(tabId);
    const tab = tabs.find((t) => t.id === tabId);
    if (tab) {
      setActivePaneId((prev) => (prev && tab.paneIds.includes(prev) ? prev : (tab.paneIds[tab.paneIds.length - 1] ?? null)));
    }
  };

  const stepTab = (dir: 1 | -1) => {
    if (tabs.length === 0) return;
    const idx = tabs.findIndex((t) => t.id === activeTabId);
    const base = idx === -1 ? 0 : idx;
    const nextIdx = (base + dir + tabs.length) % tabs.length;
    activateTab(tabs[nextIdx].id);
  };

  const gotoTab = (index: number) => {
    const tab = tabs[index];
    if (tab) activateTab(tab.id);
  };

  const reorderTabs = (fromId: string, toId: string) => {
    setTabs((prev) => {
      const from = prev.find((t) => t.id === fromId);
      if (!from) return prev;
      const withoutFrom = prev.filter((t) => t.id !== fromId);
      const idx = withoutFrom.findIndex((t) => t.id === toId);
      if (idx === -1) return prev;
      const next = [...withoutFrom];
      next.splice(idx, 0, from);
      return next;
    });
  };

  const movePaneToTab = (paneId: string, targetTabId: string) => {
    const ownerIdx = tabs.findIndex((t) => t.paneIds.includes(paneId));
    if (ownerIdx === -1) return;
    const owner = tabs[ownerIdx];
    if (owner.id === targetTabId) return;

    const remaining = owner.paneIds.filter((id) => id !== paneId);
    const withoutOwnerPane =
      remaining.length === 0
        ? tabs.filter((t) => t.id !== owner.id)
        : tabs.map((t) => (t.id === owner.id ? { ...t, paneIds: remaining } : t));
    const next = withoutOwnerPane.map((t) => (t.id === targetTabId ? { ...t, paneIds: [...t.paneIds, paneId] } : t));
    setTabs(next);
    setActiveTabId(targetTabId);
    setActivePaneId(paneId);
  };

  const setActiveTabLayout = (layout: Layout) => {
    setTabs((prev) => prev.map((t) => (t.id === activeTabId ? { ...t, layout } : t)));
  };

  const focusPane = (dir: 1 | -1) => {
    const tab = tabs.find((t) => t.id === activeTabId);
    if (!tab || tab.paneIds.length === 0) return;
    const idx = tab.paneIds.indexOf(activePaneId ?? "");
    const base = idx === -1 ? (dir === 1 ? -1 : 0) : idx;
    const next = (base + dir + tab.paneIds.length) % tab.paneIds.length;
    setActivePaneId(tab.paneIds[next]);
  };

  const handleShortcut = (action: ShortcutAction) => {
    switch (action.type) {
      case "new-tab":
        void openNewTab();
        break;
      case "new-pane":
        void addPane();
        break;
      case "close-pane":
        if (activePaneId) closePane(activePaneId);
        break;
      case "next-tab":
        stepTab(1);
        break;
      case "prev-tab":
        stepTab(-1);
        break;
      case "goto-tab":
        gotoTab(action.index);
        break;
      case "focus-pane":
        focusPane(action.dir);
        break;
    }
  };

  // 터미널 밖(탭 바, cwd 입력칸 등)에 포커스가 있을 때를 위한 전역 리스너.
  // handleShortcut이 tabs/activeTabId/cwd/selected/pinned 등 여러 상태를 참조하므로
  // deps 배열로 정확히 추적하는 대신, 매 렌더마다 재등록해 항상 최신 클로저를 쓴다
  // (단일 window 리스너 add/remove라 비용은 무시할 만하다).
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const action = matchShortcut(e);
      if (!action) return;
      // 터미널에 포커스가 있으면 TermPane의 attachCustomKeyEventHandler가 이미
      // stopPropagation으로 처리했다 (여기 도달했다면 처리 안 된 경우에 대한 방어선).
      const el = document.activeElement;
      if (el instanceof HTMLElement && el.closest(".term-wrap")) return;
      e.preventDefault();
      handleShortcut(action);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  const activeTab = tabs.find((t) => t.id === activeTabId) ?? null;
  const activeLayout = activeTab?.layout ?? "grid";

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">WSL Desktop</h1>
        <button
          className={`btn panel-toggle ${panelOpen ? "active" : ""}`}
          title="사이드 패널 토글 (distro/Docker/프로젝트)"
          onClick={() => setPanelOpen((prev) => !prev)}
        >
          ☰
        </button>
        <select value={selected} onChange={(e) => setSelected(e.currentTarget.value)}>
          {distros.map((d) => (
            <option key={d.name} value={d.name}>
              {d.name} {d.default ? "(default)" : ""}
            </option>
          ))}
        </select>
        <input
          className="cwd"
          list="cwd-recent"
          placeholder="Open path (optional, e.g. /mnt/c/projects)"
          value={cwd}
          onChange={(e) => setCwd(e.currentTarget.value)}
          onKeyDown={(e) => e.key === "Enter" && void addPane()}
        />
        <datalist id="cwd-recent">
          {recentPaths.map((p) => (
            <option key={p} value={p} />
          ))}
        </datalist>
        <button
          className={`btn pin-btn ${pinned ? "active" : ""}`}
          title={pinned ? "경로 고정됨 — 클릭하면 해제" : "경로 고정 — 켜면 열어도 입력칸이 비워지지 않습니다"}
          onClick={togglePinned}
        >
          📌
        </button>
        <button className="btn" onClick={() => void addPane()}>
          + Terminal
        </button>
        <span className="spacer" />
        <label className="toggle">
          <input type="checkbox" checked={broadcastOn} onChange={(e) => setBroadcastOn(e.currentTarget.checked)} />
          broadcast
        </label>
        {(["grid", "cols", "rows"] as const).map((l) => (
          <button key={l} className={`btn ${activeLayout === l ? "active" : ""}`} onClick={() => setActiveTabLayout(l)}>
            {l}
          </button>
        ))}
      </header>

      <div className="main">
        {panelOpen && (
          <aside className="side-panel">
            <DistroPanel
              distros={distros}
              selectedDistro={selected}
              onSelectDistro={setSelected}
              onOpenTerminal={openDistroTerminal}
              containers={containers}
              dockerMissing={dockerMissing}
              busy={busy}
              onAction={onDockerAction}
              onRefresh={() => void refreshDashboard().catch(() => undefined)}
            />
          </aside>
        )}

        <div className="terminal-area">
          {error && <div className="error">{error}</div>}

          <TabBar
            tabs={tabs}
            activeTabId={activeTabId}
            onActivate={activateTab}
            onClose={closeTab}
            onReorder={reorderTabs}
            onDropPane={movePaneToTab}
            onNewTab={() => void openNewTab()}
          />

          {windowsBuildNumber !== undefined && (
            <PaneCanvas
              tabs={tabs}
              panes={panes}
              activeTabId={activeTabId}
              activePaneId={activePaneId}
              broadcastOn={broadcastOn}
              registerWrite={registerWrite}
              unregisterWrite={unregisterWrite}
              onClosePane={closePane}
              onFocusPane={(id) => {
                setActivePaneId(id);
                const owner = tabs.find((t) => t.paneIds.includes(id));
                if (owner) setActiveTabId(owner.id);
              }}
              onShortcut={handleShortcut}
              windowsBuildNumber={windowsBuildNumber}
            />
          )}
        </div>
      </div>
    </div>
  );
}
