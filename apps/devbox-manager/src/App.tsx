import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  available,
  catalog,
  current,
  installApp,
  installPath,
  installMany,
  installed,
  launchApp,
  openInstallFolder,
  removeApp,
  rollback,
  runDiagnosis,
  type DiagnosisItem,
} from "./api";
import type {
  BatchInstallRequest,
  BatchInstallResult,
  CatalogApp,
  Current,
  InstalledApp,
  InstallPathInfo,
  InstallMode,
  ReleaseManifest,
} from "./types";
import "./App.css";

export default function App() {
  const [apps, setApps] = useState<CatalogApp[]>([]);
  const [manifest, setManifest] = useState<ReleaseManifest | null>(null);
  const [installedList, setInstalledList] = useState<InstalledApp[]>([]);
  const [currentMap, setCurrentMap] = useState<Record<string, Current | null>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [tab, setTab] = useState<"apps" | "doctor">("apps");
  const [diagnosis, setDiagnosis] = useState<DiagnosisItem[]>([]);
  const [selectedAppId, setSelectedAppId] = useState<string | null>(null);
  const [contextApp, setContextApp] = useState<CatalogApp | null>(null);
  const [batchSelection, setBatchSelection] = useState<Set<string>>(() => new Set());
  const [batchResults, setBatchResults] = useState<BatchInstallResult[]>([]);
  const [installPathDetails, setInstallPathDetails] = useState<InstallPathInfo | null>(null);
  const [batchBusy, setBatchBusy] = useState(false);
  const batchBusyRef = useRef(false);
  const operationBusyRef = useRef(false);

  const prepareAppContext = useCallback((target: HTMLElement) => {
    const id = target.dataset.appId;
    const app = apps.find((candidate) => candidate.id === id);
    if (!app) return;
    setSelectedAppId(app.id);
    setContextApp(app);
  }, [apps]);
  const appContextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => prepareAppContext(target),
  });

  const onDiagnose = useCallback(async () => {
    setError(null);
    try {
      setDiagnosis(await runDiagnosis());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  const refresh = useCallback(async () => {
    setError(null);
    setInstallPathDetails(null);
    try {
      const [cat, av, inst] = await Promise.all([catalog(), available(), installed()]);
      const visibleApps = cat.filter((a) => a.managerVisible && !a.selfManaged);
      const visibleIds = new Set(visibleApps.map((app) => app.id));
      setApps(visibleApps);
      setManifest(av);
      setInstalledList(inst.filter((item) => visibleIds.has(item.app)));
      // portable로 설치된 앱의 current.json 정보
      const curMap: Record<string, Current | null> = {};
      for (const i of inst) {
        if (i.mode === "portable" && visibleIds.has(i.app)) {
          curMap[i.app] = await current(i.app);
        }
      }
      setCurrentMap(curMap);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (tab !== "apps") {
      appContextMenu.close();
      setContextApp(null);
    }
  }, [appContextMenu.close, tab]);

  useEffect(() => {
    const id = contextApp?.id;
    if (!id) return;
    const currentApp = apps.find((app) => app.id === id) ?? null;
    if (currentApp) setContextApp(currentApp);
    else {
      appContextMenu.close();
      setContextApp(null);
      setSelectedAppId((selected) => (selected === id ? null : selected));
    }
  }, [appContextMenu.close, apps, contextApp?.id]);

  const manifestOf = (appId: string) => manifest?.apps.find((a) => a.id === appId);
  const installedOf = (appId: string) => installedList.find((i) => i.app === appId);

  const isAppBusy = (_appId: string) => batchBusy || busy !== null;

  const isUpToDate = (appId: string) => {
    const inst = installedOf(appId);
    const app = manifestOf(appId);
    if (!inst || !app) return false;
    return inst.version === app.version;
  };

  const batchCandidateIds = useMemo(() => new Set(
    apps
      .filter((candidate) => {
        const installedApp = installedList.find((item) => item.app === candidate.id);
        const availableApp = manifest?.apps.find((item) => item.id === candidate.id);
        return Boolean(availableApp && installedApp?.version !== availableApp.version);
      })
      .map((candidate) => candidate.id),
  ), [apps, installedList, manifest]);
  const selectedBatchIds = [...batchSelection].filter((id) => batchCandidateIds.has(id));
  const allBatchCandidatesSelected = batchCandidateIds.size > 0
    && selectedBatchIds.length === batchCandidateIds.size;

  const toggleBatchApp = (appId: string) => {
    if (batchBusyRef.current || !batchCandidateIds.has(appId)) return;
    setBatchSelection((currentSelection) => {
      const next = new Set(currentSelection);
      if (next.has(appId)) next.delete(appId);
      else next.add(appId);
      return next;
    });
  };

  const toggleAllBatchApps = () => {
    if (batchBusyRef.current) return;
    setBatchSelection(allBatchCandidatesSelected ? new Set() : new Set(batchCandidateIds));
  };

  const runBatch = async (requests: BatchInstallRequest[]) => {
    if (operationBusyRef.current || requests.length === 0) return;
    const installerCount = requests.filter((request) => request.mode === "installer").length;
    if (installerCount > 0 && !window.confirm(
      `${installerCount}개 앱의 설치 마법사를 각각 실행할까요? 각 창에서 설치를 완료해야 합니다.`,
    )) return;

    batchBusyRef.current = true;
    operationBusyRef.current = true;
    setBatchBusy(true);
    setError(null);
    setNotice(null);
    try {
      const results = await installMany(requests);
      setBatchResults(results);
      const failed = results.filter((result) => !result.ok);
      setBatchSelection(new Set(failed.map((result) => result.appId)));
      const succeededCount = results.length - failed.length;
      setNotice(`일괄 작업 완료: 성공 ${succeededCount}개, 실패 ${failed.length}개`);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      batchBusyRef.current = false;
      operationBusyRef.current = false;
      setBatchBusy(false);
    }
  };

  const onBatchInstall = (mode: InstallMode) => {
    const requests = selectedBatchIds.map((appId) => ({ appId, mode }));
    void runBatch(requests);
  };

  const onRetryFailed = () => {
    void runBatch(batchResults
      .filter((result) => !result.ok)
      .map(({ appId, mode }) => ({ appId, mode })));
  };

  const onInstall = async (appId: string, mode: InstallMode) => {
    if (operationBusyRef.current) return;
    operationBusyRef.current = true;
    setBusy(`${appId}:${mode}`);
    setError(null);
    setNotice(null);
    try {
      const msg = await installApp(appId, mode);
      setNotice(`${appId}: ${msg}`);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      operationBusyRef.current = false;
      setBusy(null);
    }
  };

  const onLaunch = async (appId: string) => {
    if (operationBusyRef.current) return;
    operationBusyRef.current = true;
    setBusy(`${appId}:launch`);
    setError(null);
    setNotice(null);
    try {
      await launchApp(appId);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      operationBusyRef.current = false;
      setBusy(null);
    }
  };

  const onRollback = async (appId: string) => {
    if (operationBusyRef.current) return;
    operationBusyRef.current = true;
    setBusy(`${appId}:rollback`);
    setError(null);
    setNotice(null);
    try {
      const msg = await rollback(appId);
      setNotice(msg);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      operationBusyRef.current = false;
      setBusy(null);
    }
  };

  const onOpenInstallFolder = async (appId: string) => {
    if (operationBusyRef.current) return;
    operationBusyRef.current = true;
    setBusy(`${appId}:folder`);
    setError(null);
    setNotice(null);
    try {
      await openInstallFolder(appId);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      operationBusyRef.current = false;
      setBusy(null);
    }
  };

  const onShowInstallPath = async (appId: string) => {
    if (operationBusyRef.current) return;
    operationBusyRef.current = true;
    setBusy(`${appId}:path`);
    setError(null);
    setNotice(null);
    try {
      setInstallPathDetails(await installPath(appId));
      setSelectedAppId(appId);
    } catch (e) {
      setInstallPathDetails(null);
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      operationBusyRef.current = false;
      setBusy(null);
    }
  };

  const onRemove = async (app: CatalogApp) => {
    if (operationBusyRef.current) return;
    if (!window.confirm(
      `'${app.displayName}' 휴대용 앱을 제거할까요? Manager가 관리하는 실행 파일과 보존 버전만 삭제하며 앱 사용자 데이터는 유지됩니다.`,
    )) return;
    operationBusyRef.current = true;
    setBusy(`${app.id}:remove`);
    setError(null);
    setNotice(null);
    try {
      setNotice(await removeApp(app.id));
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      operationBusyRef.current = false;
      setBusy(null);
    }
  };

  const contextInstalled = contextApp ? installedOf(contextApp.id) : undefined;
  const contextManifest = contextApp ? manifestOf(contextApp.id) : undefined;
  const contextCurrent = contextApp ? currentMap[contextApp.id] : null;
  const contextUpToDate = contextApp ? isUpToDate(contextApp.id) : false;
  const contextBusy = contextApp ? isAppBusy(contextApp.id) : false;
  const contextPortable = contextInstalled?.mode === "portable";
  const contextCanRollback = contextPortable && contextCurrent?.previousVersion != null;
  const appContextItems = useMemo<readonly ContextMenuEntry[]>(() => {
    if (!contextApp) return [];
    return [
      {
        type: "submenu",
        id: "install-update",
        label: "설치/업데이트",
        disabled: contextBusy || contextUpToDate || !contextManifest,
        items: [
          { type: "item", id: "install-portable", label: "휴대용" },
          { type: "item", id: "install-installer", label: "설치 패키지" },
        ],
      },
      { type: "item", id: "launch", label: "실행", disabled: contextBusy || !contextPortable },
      {
        type: "item",
        id: "rollback",
        label: "이전 버전 롤백",
        disabled: contextBusy || !contextCanRollback,
      },
      { type: "separator", id: "install-folder-separator" },
      {
        type: "item",
        id: "open-folder",
        label: "설치 폴더 열기",
        disabled: contextBusy || !contextPortable,
      },
      {
        type: "item",
        id: "install-path",
        label: "설치 경로 정보",
        disabled: contextBusy || !contextInstalled,
      },
      { type: "separator", id: "remove-separator" },
      {
        type: "item",
        id: "remove",
        label: "제거",
        disabled: contextBusy || !contextPortable,
        danger: true,
      },
    ];
  }, [
    contextApp,
    contextBusy,
    contextCanRollback,
    contextInstalled,
    contextManifest,
    contextPortable,
    contextUpToDate,
  ]);

  const onAppContextSelect = (id: string) => {
    const app = contextApp;
    if (!app) return;
    if (id === "install-portable") void onInstall(app.id, "portable");
    else if (id === "install-installer") void onInstall(app.id, "installer");
    else if (id === "launch") void onLaunch(app.id);
    else if (id === "rollback") void onRollback(app.id);
    else if (id === "open-folder") void onOpenInstallFolder(app.id);
    else if (id === "install-path") void onShowInstallPath(app.id);
    else if (id === "remove") void onRemove(app);
  };

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">Devbox Manager</h1>
        <button
          className={`btn ${tab === "apps" ? "active" : ""}`}
          disabled={batchBusy}
          onClick={() => setTab("apps")}
        >
          앱
        </button>
        <button
          className={`btn ${tab === "doctor" ? "active" : ""}`}
          disabled={batchBusy}
          onClick={() => { setTab("doctor"); void onDiagnose(); }}
        >
          환경 진단
        </button>
        <span className="latest">Latest: {manifest ? manifest.releaseTag : "..."}</span>
        <span className="spacer" />
        <button className="btn refresh" disabled={batchBusy} onClick={() => void refresh()}>
          Refresh
        </button>
      </header>

      {error && <div className="error">{error}</div>}
      {notice && <div className="notice">{notice}</div>}

      {tab === "doctor" ? (
        <div className="doctor">
          <div className="doctor-head">
            <span className="dim">read-only 진단 · 자동 설치·수정 없음</span>
            <button className="btn" onClick={() => void onDiagnose()}>다시 진단</button>
          </div>
          {diagnosis.map((d) => (
            <div key={d.name} className={`doctor-row ${d.ok ? "ok" : "bad"}`}>
              <span className="doctor-name">{d.name}</span>
              <span className="doctor-detail">{d.detail}</span>
            </div>
          ))}
          {diagnosis.length === 0 && <div className="dim">진단을 실행해 주세요.</div>}
          <div className="dim doctor-note">지원 번들·path·환경변수는 redaction되어야 합니다 (§15.4 경계).</div>
        </div>
      ) : (
      <div className="table-wrap">
        {installPathDetails && (
          <section className="install-path-panel" aria-label="검증된 설치 경로 정보">
            <div className="install-path-head">
              <div>
                <strong>
                  {apps.find((candidate) => candidate.id === installPathDetails.appId)?.displayName
                    ?? installPathDetails.appId}
                </strong>
                <span className="read-only-tag">읽기 전용</span>
              </div>
              <button
                className="btn"
                aria-label="설치 경로 정보 닫기"
                onClick={() => setInstallPathDetails(null)}
              >
                닫기
              </button>
            </div>
            <dl className="install-path-grid">
              <dt>Executable</dt>
              <dd><code>{installPathDetails.executable ?? "Manager가 실제 설치 위치를 추적하지 않습니다."}</code></dd>
              <dt>Install root</dt>
              <dd><code>{installPathDetails.installRoot ?? "Manager가 실제 설치 위치를 추적하지 않습니다."}</code></dd>
              <dt>Source manifest</dt>
              <dd><code>{installPathDetails.sourceManifest}</code></dd>
            </dl>
            <div className="dim install-path-note">
              locator·catalog revision·manifest·canonical executable을 검증한 결과만 표시하며 파일을 열거나 변경하지 않습니다.
              {installPathDetails.mode === "installer" && (
                <> 설치 패키지는 마법사 실행 뒤의 실제 위치를 Manager가 소유하지 않아 추측하지 않습니다.</>
              )}
            </div>
          </section>
        )}
        <section className="batch-panel" aria-label="일괄 설치 및 업데이트">
          <div className="batch-actions">
            <strong>일괄 작업</strong>
            <span className="dim">설치/업데이트 가능한 앱 {selectedBatchIds.length}개 선택</span>
            <button
              className="btn"
              disabled={batchBusy || busy !== null || selectedBatchIds.length === 0}
              onClick={() => onBatchInstall("portable")}
            >
              {batchBusy ? "처리 중..." : "휴대용 일괄 실행"}
            </button>
            <button
              className="btn"
              disabled={batchBusy || busy !== null || selectedBatchIds.length === 0}
              onClick={() => onBatchInstall("installer")}
            >
              설치 패키지 일괄 실행
            </button>
            {batchResults.some((result) => !result.ok) && (
              <button
                className="btn retry"
                disabled={batchBusy || busy !== null}
                onClick={onRetryFailed}
              >
                실패 항목만 재시도 ({batchResults.filter((result) => !result.ok).length})
              </button>
            )}
          </div>
          <div className="dim batch-note">
            성공한 앱은 유지하고 실패한 앱만 선택 상태로 남깁니다. 설치 패키지는 앱마다 별도 마법사를 실행합니다.
          </div>
          {batchResults.length > 0 && (
            <div className="batch-results" aria-label="일괄 작업 결과" aria-live="polite">
              {batchResults.map((result) => {
                const displayName = apps.find((candidate) => candidate.id === result.appId)?.displayName
                  ?? result.appId;
                return (
                  <div
                    key={`${result.appId}:${result.mode}`}
                    className={`batch-result ${result.ok ? "ok" : "bad"}`}
                  >
                    <span className="batch-result-name">{displayName}</span>
                    <span>{result.mode === "portable" ? "휴대용" : "설치 패키지"}</span>
                    <span>{result.ok ? "성공" : "실패"}</span>
                    <span className="batch-result-message">{result.message}</span>
                  </div>
                );
              })}
            </div>
          )}
        </section>
        <table>
          <thead>
            <tr>
              <th className="batch-select-cell">
                <input
                  type="checkbox"
                  aria-label="설치 및 업데이트 가능한 앱 전체 선택"
                  checked={allBatchCandidatesSelected}
                  disabled={batchBusy || batchCandidateIds.size === 0}
                  onChange={toggleAllBatchApps}
                />
              </th>
              <th>APP</th>
              <th>INSTALLED</th>
              <th>LATEST</th>
              <th>ACTION</th>
            </tr>
          </thead>
          <tbody>
            {apps.map((a) => {
              const inst = installedOf(a.id);
              const app = manifestOf(a.id);
              const upToDate = isUpToDate(a.id);
              const cur = currentMap[a.id];
              const canRollback = inst?.mode === "portable" && cur?.previousVersion != null;
              return (
                <tr
                  key={a.id}
                  className={`app-row ${selectedAppId === a.id ? "selected" : ""}`}
                  tabIndex={0}
                  aria-current={selectedAppId === a.id ? "true" : undefined}
                  data-app-id={a.id}
                  onClick={() => setSelectedAppId(a.id)}
                  {...appContextMenu.triggerProps}
                >
                  <td className="batch-select-cell">
                    <input
                      type="checkbox"
                      aria-label={`${a.displayName} 일괄 선택`}
                      checked={batchSelection.has(a.id) && batchCandidateIds.has(a.id)}
                      disabled={batchBusy || !batchCandidateIds.has(a.id)}
                      onClick={(event) => event.stopPropagation()}
                      onChange={() => toggleBatchApp(a.id)}
                    />
                  </td>
                  <td className="app-name">{a.displayName}</td>
                  <td>
                    {inst ? (
                      <span>
                        {inst.version} ({inst.mode})
                        {canRollback && (
                          <span className="dim"> ← prev {cur?.previousVersion}</span>
                        )}
                      </span>
                    ) : (
                      <span className="dim">-</span>
                    )}
                  </td>
                  <td>{app ? app.version : "-"}</td>
                  <td className="actions">
                    {inst && (
                      <button
                        className="btn"
                        aria-label={`${a.displayName} 설치 경로 정보`}
                        disabled={isAppBusy(a.id)}
                        onClick={() => void onShowInstallPath(a.id)}
                      >
                        {busy === `${a.id}:path` ? "..." : "Paths"}
                      </button>
                    )}
                    {inst?.mode === "portable" && (
                      <button className="btn" disabled={isAppBusy(a.id)} onClick={() => void onLaunch(a.id)}>
                        Launch
                      </button>
                    )}
                    {canRollback && (
                      <button className="btn" disabled={isAppBusy(a.id)} onClick={() => void onRollback(a.id)}>
                        {busy === `${a.id}:rollback` ? "..." : "Rollback"}
                      </button>
                    )}
                    {!upToDate && app && (
                      <>
                        <button className="btn" disabled={isAppBusy(a.id)} onClick={() => void onInstall(a.id, "portable")}>
                          {busy === `${a.id}:portable` ? "..." : inst ? "Update (portable)" : "Install (portable)"}
                        </button>
                        <button className="btn" disabled={isAppBusy(a.id)} onClick={() => void onInstall(a.id, "installer")}>
                          {busy === `${a.id}:installer` ? "..." : inst ? "Update (setup)" : "Install (setup)"}
                        </button>
                      </>
                    )}
                    {upToDate && <span className="dim tag">up to date</span>}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      )}

      <footer className="foot">
        <div className="dim">휴대용(portable): exe를 자체 폴더에 받아 관리. 설치 마법사(setup): 공식 설치 프로그램 실행.</div>
        <div className="dim">각 앱의 최신 버전은 release-manifest.json에서 읽는다 (release tag와 독립적).</div>
        <div className="dim">자세한 사용법: docs/windows-guide.md</div>
      </footer>
      <ContextMenu
        open={appContextMenu.open}
        anchor={appContextMenu.anchor}
        restoreFocusTo={appContextMenu.restoreFocusTo}
        items={appContextItems}
        onSelect={onAppContextSelect}
        onClose={appContextMenu.close}
        ariaLabel="앱 메뉴"
      />
    </div>
  );
}
