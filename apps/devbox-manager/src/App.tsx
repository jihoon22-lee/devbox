import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  available,
  applyInstallRoot,
  catalog,
  current,
  installApp,
  installPath,
  installMany,
  installed,
  launchApp,
  openInstallFolder,
  previewInstallRoot,
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
  InstallRootPreview,
  InstallMode,
  ReleaseManifest,
} from "./types";
import "./App.css";

const ROOT_STATUS_LABEL: Record<InstallRootPreview["status"], string> = {
  ready: "적용 가능",
  "already-active": "이미 사용 중",
  "existing-install": "기존 설치로 이동 차단",
  "candidate-conflict": "비어 있지 않음",
  "permission-denied": "쓰기 권한 없음",
  "insufficient-free-space": "여유 공간 부족",
  "free-space-unavailable": "여유 공간 확인 불가",
};

function rootStatusDescription(preview: InstallRootPreview): string {
  switch (preview.status) {
    case "ready":
      return "검증된 빈 디렉터리입니다. 적용을 누르면 다음 설치부터 이 root를 사용합니다.";
    case "already-active":
      return "현재 설치 root와 같습니다. 파일은 변경되지 않습니다.";
    case "existing-install":
      return "현재 root에 설치 기록 또는 관리 파일이 있어 자동 이동하지 않습니다.";
    case "candidate-conflict":
      return "기존 파일이 있는 디렉터리는 덮어쓰지 않습니다.";
    case "permission-denied":
      return "설치 root에 쓸 권한이 없어 적용하지 않습니다.";
    case "insufficient-free-space":
      return "필수 여유 공간을 확보한 뒤 다시 preview하세요.";
    case "free-space-unavailable":
      return "여유 공간을 확인할 수 없으므로 적용하지 않습니다.";
  }
}

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
  const [installRootInput, setInstallRootInput] = useState("");
  const [installRootPreview, setInstallRootPreview] = useState<InstallRootPreview | null>(null);
  const [installRootBusy, setInstallRootBusy] = useState(false);
  const [installRootError, setInstallRootError] = useState<string | null>(null);
  const [installRootComposing, setInstallRootComposing] = useState(false);
  const [batchBusy, setBatchBusy] = useState(false);
  const [readBusy, setReadBusy] = useState(false);
  const batchBusyRef = useRef(false);
  const operationBusyRef = useRef(false);
  const readBusyRef = useRef(false);
  const rootBusyRef = useRef(false);
  const rootRequestIdRef = useRef(0);
  const mountedRef = useRef(true);
  const refreshRequestIdRef = useRef(0);

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
    if (operationBusyRef.current || readBusyRef.current) return;
    readBusyRef.current = true;
    setReadBusy(true);
    const requestId = ++refreshRequestIdRef.current;
    setError(null);
    try {
      const result = await runDiagnosis();
      if (mountedRef.current && requestId === refreshRequestIdRef.current) setDiagnosis(result);
    } catch (e) {
      if (mountedRef.current && requestId === refreshRequestIdRef.current) {
        setError(e instanceof Error ? e.message : String(e));
      }
    } finally {
      if (requestId === refreshRequestIdRef.current) {
        readBusyRef.current = false;
        if (mountedRef.current) setReadBusy(false);
      }
    }
  }, []);

  const refresh = useCallback(async (internal = false) => {
    if (!internal && (operationBusyRef.current || readBusyRef.current)) return;
    readBusyRef.current = true;
    setReadBusy(true);
    const requestId = ++refreshRequestIdRef.current;
    setError(null);
    setInstallPathDetails(null);
    try {
      const [cat, av, inst] = await Promise.all([catalog(), available(), installed()]);
      if (!mountedRef.current || requestId !== refreshRequestIdRef.current) return;
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
      if (!mountedRef.current || requestId !== refreshRequestIdRef.current) return;
      setCurrentMap(curMap);
    } catch (e) {
      if (mountedRef.current && requestId === refreshRequestIdRef.current) {
        setError(e instanceof Error ? e.message : String(e));
      }
    } finally {
      if (requestId === refreshRequestIdRef.current) {
        readBusyRef.current = false;
        if (mountedRef.current) setReadBusy(false);
      }
    }
  }, []);

  useEffect(() => {
    // React StrictMode mounts effects twice in development. Reset this guard
    // for the live effect so the second (real) refresh is not discarded after
    // the first effect's cleanup invalidates its request.
    mountedRef.current = true;
    void refresh(true);
    return () => {
      mountedRef.current = false;
      refreshRequestIdRef.current += 1;
      rootRequestIdRef.current += 1;
    };
  }, [refresh]);

  const onInstallRootInput = (value: string) => {
    // A late preview/apply response must never resurrect a preview for an
    // earlier path. The input is disabled while native work is in flight, but
    // this generation bump also protects programmatic/IME-driven changes.
    rootRequestIdRef.current += 1;
    setInstallRootInput(value);
    setInstallRootPreview(null);
    setInstallRootError(null);
  };

  const previewRoot = async () => {
    if (
      rootBusyRef.current
      || operationBusyRef.current
      || readBusyRef.current
      || installRootComposing
      || !installRootInput.trim()
    ) return;
    const requestId = ++rootRequestIdRef.current;
    rootBusyRef.current = true;
    operationBusyRef.current = true;
    setInstallRootBusy(true);
    setInstallRootError(null);
    try {
      const preview = await previewInstallRoot(installRootInput);
      if (mountedRef.current && requestId === rootRequestIdRef.current) {
        setInstallRootPreview(preview);
      }
    } catch {
      if (mountedRef.current && requestId === rootRequestIdRef.current) {
        setInstallRootPreview(null);
        setInstallRootError("설치 root를 확인할 수 없습니다. 경로와 권한을 확인하세요.");
      }
    } finally {
      rootBusyRef.current = false;
      operationBusyRef.current = false;
      if (mountedRef.current) setInstallRootBusy(false);
    }
  };

  const applyRoot = async () => {
    const preview = installRootPreview;
    if (
      !preview
      || !preview.canApply
      || preview.status !== "ready"
      || rootBusyRef.current
      || operationBusyRef.current
      || readBusyRef.current
    ) return;
    if (!window.confirm(
      "검증된 빈 디렉터리를 새 설치 root로 적용할까요? 기존 설치는 자동으로 이동하거나 삭제하지 않습니다.",
    )) return;
    const requestId = ++rootRequestIdRef.current;
    rootBusyRef.current = true;
    operationBusyRef.current = true;
    setInstallRootBusy(true);
    setInstallRootError(null);
    try {
      const result = await applyInstallRoot(installRootInput, preview.registryRevision);
      if (mountedRef.current && requestId === rootRequestIdRef.current) {
        setInstallRootPreview(null);
        setNotice(`설치 root를 적용했습니다. revision ${result.registryRevision}`);
      }
      await refresh(true);
    } catch {
      if (mountedRef.current && requestId === rootRequestIdRef.current) {
        setInstallRootPreview(null);
        setInstallRootError("설치 root를 적용할 수 없습니다. 최신 preview를 다시 확인하세요.");
      }
    } finally {
      rootBusyRef.current = false;
      operationBusyRef.current = false;
      if (mountedRef.current) setInstallRootBusy(false);
    }
  };

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

  const isAppBusy = (_appId: string) => (
    batchBusy || busy !== null || installRootBusy || readBusy
  );

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
    if (
      operationBusyRef.current
      || readBusyRef.current
      || batchBusyRef.current
      || !batchCandidateIds.has(appId)
    ) return;
    setBatchSelection((currentSelection) => {
      const next = new Set(currentSelection);
      if (next.has(appId)) next.delete(appId);
      else next.add(appId);
      return next;
    });
  };

  const toggleAllBatchApps = () => {
    if (operationBusyRef.current || readBusyRef.current || batchBusyRef.current) return;
    setBatchSelection(allBatchCandidatesSelected ? new Set() : new Set(batchCandidateIds));
  };

  const runBatch = async (requests: BatchInstallRequest[]) => {
    if (operationBusyRef.current || readBusyRef.current || requests.length === 0) return;
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
      await refresh(true);
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
    if (operationBusyRef.current || readBusyRef.current) return;
    operationBusyRef.current = true;
    setBusy(`${appId}:${mode}`);
    setError(null);
    setNotice(null);
    try {
      const msg = await installApp(appId, mode);
      setNotice(`${appId}: ${msg}`);
      await refresh(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      operationBusyRef.current = false;
      setBusy(null);
    }
  };

  const onLaunch = async (appId: string) => {
    if (operationBusyRef.current || readBusyRef.current) return;
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
    if (operationBusyRef.current || readBusyRef.current) return;
    operationBusyRef.current = true;
    setBusy(`${appId}:rollback`);
    setError(null);
    setNotice(null);
    try {
      const msg = await rollback(appId);
      setNotice(msg);
      await refresh(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      operationBusyRef.current = false;
      setBusy(null);
    }
  };

  const onOpenInstallFolder = async (appId: string) => {
    if (operationBusyRef.current || readBusyRef.current) return;
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
    if (operationBusyRef.current || readBusyRef.current) return;
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
    if (operationBusyRef.current || readBusyRef.current) return;
    if (!window.confirm(
      `'${app.displayName}' 휴대용 앱을 제거할까요? Manager가 관리하는 실행 파일과 보존 버전만 삭제하며 앱 사용자 데이터는 유지됩니다.`,
    )) return;
    operationBusyRef.current = true;
    setBusy(`${app.id}:remove`);
    setError(null);
    setNotice(null);
    try {
      setNotice(await removeApp(app.id));
      await refresh(true);
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
          disabled={batchBusy || installRootBusy || readBusy}
          onClick={() => setTab("apps")}
        >
          앱
        </button>
        <button
          className={`btn ${tab === "doctor" ? "active" : ""}`}
          disabled={batchBusy || installRootBusy || readBusy}
          onClick={() => { setTab("doctor"); void onDiagnose(); }}
        >
          환경 진단
        </button>
        <span className="latest">Latest: {manifest ? manifest.releaseTag : "..."}</span>
        <span className="spacer" />
        <button
          className="btn refresh"
          disabled={batchBusy || installRootBusy || readBusy}
          onClick={() => void refresh()}
        >
          Refresh
        </button>
      </header>

      {error && <div className="error">{error}</div>}
      {notice && <div className="notice">{notice}</div>}

      {tab === "doctor" ? (
        <div className="doctor">
          <div className="doctor-head">
            <span className="dim">read-only 진단 · 자동 설치·수정 없음</span>
            <button
              className="btn"
              disabled={installRootBusy || readBusy}
              onClick={() => void onDiagnose()}
            >
              다시 진단
            </button>
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
        <section
          className="install-root-panel"
          aria-labelledby="install-root-heading"
          aria-busy={installRootBusy || readBusy}
        >
          <div className="install-root-head">
            <div>
              <h2 id="install-root-heading">설치 root 지정</h2>
              <p className="dim">
                기존 설치는 이동하지 않으며, 비어 있고 검증된 디렉터리만 다음 설치에 적용합니다.
              </p>
            </div>
            <span className="read-only-tag">preview 후 확인</span>
          </div>
          <div className="install-root-form">
            <label htmlFor="install-root-path">설치 root 경로</label>
            <input
              id="install-root-path"
              value={installRootInput}
              placeholder="예: C:\\Devbox"
              maxLength={4096}
              disabled={batchBusy || busy !== null || installRootBusy || readBusy}
              autoComplete="off"
              spellCheck={false}
              aria-describedby="install-root-help"
              onCompositionStart={() => setInstallRootComposing(true)}
              onCompositionEnd={(event) => {
                setInstallRootComposing(false);
                onInstallRootInput(event.currentTarget.value);
              }}
              onChange={(event) => onInstallRootInput(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.nativeEvent.isComposing && !installRootComposing) {
                  event.preventDefault();
                  void previewRoot();
                }
              }}
            />
            <button
              className="btn"
              type="button"
              disabled={
                batchBusy
                || busy !== null
                || installRootBusy
                || readBusy
                || !installRootInput.trim()
              }
              onClick={() => void previewRoot()}
            >
              {installRootBusy ? "확인 중..." : "미리 확인"}
            </button>
          </div>
          <div id="install-root-help" className="dim install-root-help">
            절대 경로만 허용하며 symlink/reparse point, root·home·workspace, 환경변수 표기와 기존 파일을 거부합니다.
          </div>
          {installRootError && (
            <div className="error install-root-error" role="alert">{installRootError}</div>
          )}
          {installRootPreview && (
            <div className="install-root-preview" role="status" aria-live="polite">
              <div className="install-root-preview-head">
                <strong>{ROOT_STATUS_LABEL[installRootPreview.status]}</strong>
                <span className="dim">registry revision {installRootPreview.registryRevision}</span>
              </div>
              <code className="install-root-candidate">{installRootPreview.candidatePath}</code>
              <p className="dim">{rootStatusDescription(installRootPreview)}</p>
              <dl className="install-root-facts">
                <div><dt>여유 공간</dt><dd>{installRootPreview.freeSpaceBytes == null ? "확인 불가" : `${Math.floor(installRootPreview.freeSpaceBytes / 1024 / 1024)} MiB`}</dd></div>
                <div><dt>필요 최소 공간</dt><dd>{Math.floor(installRootPreview.requiredFreeSpaceBytes / 1024 / 1024)} MiB</dd></div>
                <div><dt>현재 설치</dt><dd>{installRootPreview.activeInstallCount}개</dd></div>
                <div><dt>후보 항목</dt><dd>{installRootPreview.candidateEntryCount}개</dd></div>
              </dl>
              {installRootPreview.status === "ready" && (
                <button
                  className="btn primary"
                  type="button"
                  disabled={batchBusy || busy !== null || installRootBusy || readBusy}
                  onClick={() => void applyRoot()}
                >
                  확인 후 이 root 적용
                </button>
              )}
            </div>
          )}
        </section>
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
              disabled={
                batchBusy
                || busy !== null
                || installRootBusy
                || readBusy
                || selectedBatchIds.length === 0
              }
              onClick={() => onBatchInstall("portable")}
            >
              {batchBusy ? "처리 중..." : "휴대용 일괄 실행"}
            </button>
            <button
              className="btn"
              disabled={
                batchBusy
                || busy !== null
                || installRootBusy
                || readBusy
                || selectedBatchIds.length === 0
              }
              onClick={() => onBatchInstall("installer")}
            >
              설치 패키지 일괄 실행
            </button>
            {batchResults.some((result) => !result.ok) && (
              <button
                className="btn retry"
                disabled={batchBusy || busy !== null || installRootBusy || readBusy}
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
                  disabled={
                    batchBusy || installRootBusy || readBusy || batchCandidateIds.size === 0
                  }
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
                      disabled={
                        batchBusy || installRootBusy || readBusy || !batchCandidateIds.has(a.id)
                      }
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
