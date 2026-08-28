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
  cancelDataDiagnostics,
  cancelSupportBundle,
  current,
  exportDataPreview,
  exportSupportBundle,
  inspectDataDatabases,
  installApp,
  installPath,
  installMany,
  installed,
  installRelatedTool,
  launchApp,
  launchRelatedTool,
  openRelatedToolUrl,
  openInstallFolder,
  onPendingOpen,
  previewRemoveApp,
  previewDataQuery,
  previewSupportBundle,
  previewInstallRoot,
  relatedTools,
  removeApp,
  rollback,
  runDiagnosis,
  takePendingOpen,
  type DiagnosisItem,
} from "./api";
import type {
  BatchInstallRequest,
  BatchInstallResult,
  CatalogApp,
  Current,
  DataDatabaseInfo,
  DataInspectorSnapshot,
  DataQueryResult,
  InstalledApp,
  InstallPathInfo,
  InstallRootPreview,
  InstallMode,
  RemoveAppRequest,
  RemovePreview,
  RemoveResult,
  RelatedTool,
  ReleaseManifest,
  SupportBundlePreview,
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

const REMOVE_PREVIEW_ERROR = "제거 대상을 확인할 수 없습니다. 설치 상태를 확인한 뒤 다시 시도하세요.";
const REMOVE_STALE_ERROR = "설치 상태가 바뀌었습니다. 최신 제거 미리 보기를 다시 확인하세요.";
const RELATED_TOOL_GENERIC_ERROR = "관련 도구 작업을 완료할 수 없습니다.";
const RELATED_TOOL_SAFE_ERRORS = new Set([
  "관련 도구 식별자가 올바르지 않습니다.",
  "관련 도구 설치 확인값이 올바르지 않습니다.",
  "관련 도구 설치는 사용자 확인이 필요합니다.",
  "다른 관련 도구 작업이 진행 중입니다. 잠시 후 다시 시도하세요.",
  "관련 도구 감지를 완료할 수 없습니다.",
  "관련 도구 감지 응답이 올바르지 않습니다.",
  "관련 도구 작업 결과가 올바르지 않습니다.",
  "관련 도구 설치를 시작할 수 없습니다.",
  "관련 도구를 실행할 수 없습니다.",
  "관련 도구를 실행할 수 없습니다. 잠시 후 다시 시도하세요.",
  "설치된 실행 파일을 찾을 수 없습니다. 먼저 확인 후 설치하세요.",
  "Related Tools는 Windows에서만 사용할 수 있습니다.",
  "WinGet을 사용할 수 없습니다. Windows App Installer를 설치한 뒤 다시 시도하세요.",
  "WinGet 설치가 실패했거나 취소되었습니다. 네트워크와 패키지 상태를 확인하세요.",
  "WinGet 설치가 제한 시간 안에 끝나지 않았습니다. 설치 창과 앱 상태를 확인하세요.",
]);

function safeRelatedToolError(error: unknown): string {
  const message = error instanceof Error
    ? error.message
    : typeof error === "string" ? error : "";
  return RELATED_TOOL_SAFE_ERRORS.has(message) ? message : RELATED_TOOL_GENERIC_ERROR;
}

function removalStateDescription(preview: RemovePreview): string {
  if (preview.mode === "installer") {
    return "설치 패키지는 마법사가 관리하는 실제 설치 위치와 제거 프로그램을 Manager가 소유하지 않습니다.";
  }
  switch (preview.state) {
    case "ready":
      return "Manager가 소유한 portable 실행 파일과 보존 버전만 제거합니다.";
    case "partial":
      return "이전 제거가 중단된 상태입니다. 남아 있는 Manager 소유 파일만 다시 정리합니다.";
    case "missing":
      return "실행 파일은 이미 없고 설치 기록만 남아 있습니다. 기록을 정리할 수 있습니다.";
  }
  return "Manager가 소유한 portable 파일만 제거합니다.";
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.floor(bytes / 1024)} KiB`;
  return `${Math.floor(bytes / 1024 / 1024)} MiB`;
}

function operationId(prefix: string): string {
  const random = globalThis.crypto?.randomUUID?.();
  return `${prefix}-${random ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`}`;
}

function downloadTextFile(filename: string, mimeType: string, content: string): void {
  const blob = new Blob([content], { type: mimeType });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.rel = "noreferrer";
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
}

function dataStateLabel(state: DataDatabaseInfo["state"]): string {
  switch (state) {
    case "available": return "사용 가능";
    case "missing": return "없음";
    case "unsafe-path": return "안전하지 않은 경로";
    case "unreadable": return "읽을 수 없음";
  }
}

function dataIntegrityLabel(integrity: DataDatabaseInfo["integrity"]): string {
  switch (integrity) {
    case "ok": return "정상";
    case "failed": return "무결성 실패";
    case "timed-out": return "시간 초과";
    case "unavailable": return "확인 불가";
  }
}

function relatedDetectionDescription(tool: RelatedTool): string {
  switch (tool.detection) {
    case "path":
      return "시스템 명령에서 실행 파일을 확인했습니다.";
    case "known-location":
      return "표준 설치 위치에서 실행 파일을 확인했습니다.";
    case "not-found":
      return "표준 감지 위치에서 찾지 못했습니다.";
    case "unavailable":
      return "Windows 실행 환경에서 감지를 사용할 수 없습니다.";
    default:
      return "감지 결과를 표시할 수 없습니다.";
  }
}

const RELATED_TOOL_OFFICIAL_HOSTS = new Set([
  "learn.microsoft.com",
  "github.com",
  "code.visualstudio.com",
  "www.usebruno.com",
  "dbeaver.io",
  "sqlitebrowser.org",
  "desktop.github.com",
  "podman-desktop.io",
  "www.docker.com",
]);
const MAX_RELATED_TOOL_URL_LENGTH = 2048;

function safeExternalUrl(value: string): string | null {
  try {
    if (value.length > MAX_RELATED_TOOL_URL_LENGTH) return null;
    const url = new URL(value);
    if (
      url.protocol !== "https:"
      || url.username
      || url.password
      || url.port
      || url.hostname.length === 0
      || !RELATED_TOOL_OFFICIAL_HOSTS.has(url.hostname)
    ) {
      return null;
    }
    return url.toString();
  } catch {
    return null;
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
  const [tab, setTab] = useState<"apps" | "doctor" | "related-tools">("apps");
  const [diagnosis, setDiagnosis] = useState<DiagnosisItem[]>([]);
  const [dataSnapshot, setDataSnapshot] = useState<DataInspectorSnapshot | null>(null);
  const [dataAppId, setDataAppId] = useState<string | null>(null);
  const [dataSql, setDataSql] = useState("SELECT name, type FROM sqlite_schema");
  const [dataResult, setDataResult] = useState<DataQueryResult | null>(null);
  const [dataBusy, setDataBusy] = useState(false);
  const [supportPreview, setSupportPreview] = useState<SupportBundlePreview | null>(null);
  const [supportBusy, setSupportBusy] = useState(false);
  const [relatedToolList, setRelatedToolList] = useState<RelatedTool[]>([]);
  const [relatedBusy, setRelatedBusy] = useState(false);
  const [relatedError, setRelatedError] = useState<string | null>(null);
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
  const [removePreview, setRemovePreview] = useState<RemovePreview | null>(null);
  const [removePreviewError, setRemovePreviewError] = useState<string | null>(null);
  const [removeResult, setRemoveResult] = useState<RemoveResult | null>(null);
  const [batchBusy, setBatchBusy] = useState(false);
  const [readBusy, setReadBusy] = useState(false);
  const batchBusyRef = useRef(false);
  const operationBusyRef = useRef(false);
  const readBusyRef = useRef(false);
  const rootBusyRef = useRef(false);
  const rootRequestIdRef = useRef(0);
  const mountedRef = useRef(true);
  const refreshRequestIdRef = useRef(0);
  const removeRequestIdRef = useRef(0);
  const dataRequestIdRef = useRef(0);
  const dataOperationIdRef = useRef<string | null>(null);
  const supportRequestIdRef = useRef(0);
  const supportOperationIdRef = useRef<string | null>(null);
  const relatedRequestIdRef = useRef(0);
  const relatedActionIdRef = useRef(0);

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

  const onInspectData = useCallback(async () => {
    if (operationBusyRef.current || readBusyRef.current || dataBusy || supportBusy) return;
    const requestId = ++dataRequestIdRef.current;
    const id = operationId("data");
    dataOperationIdRef.current = id;
    readBusyRef.current = true;
    setReadBusy(true);
    setDataBusy(true);
    setError(null);
    setDataResult(null);
    try {
      const snapshot = await inspectDataDatabases(id);
      if (mountedRef.current && requestId === dataRequestIdRef.current) {
        setDataSnapshot(snapshot);
        const firstAvailable = snapshot.databases.find((database) => database.state === "available");
        setDataAppId((currentId) => (
          currentId && snapshot.databases.some((database) => (
            database.appId === currentId && database.state === "available"
          ))
            ? currentId
            : firstAvailable?.appId ?? null
        ));
      }
    } catch (e) {
      if (mountedRef.current && requestId === dataRequestIdRef.current) {
        setError(e instanceof Error ? e.message : "데이터베이스를 확인할 수 없습니다.");
      }
    } finally {
      if (requestId === dataRequestIdRef.current) {
        dataOperationIdRef.current = null;
        readBusyRef.current = false;
        if (mountedRef.current) {
          setDataBusy(false);
          setReadBusy(false);
        }
      }
    }
  }, [dataBusy, supportBusy]);

  const onCancelData = useCallback(() => {
    const id = dataOperationIdRef.current;
    if (id) void cancelDataDiagnostics(id).catch(() => undefined);
  }, []);

  const onPreviewData = useCallback(async () => {
    if (operationBusyRef.current || readBusyRef.current || dataBusy || supportBusy) return;
    const database = dataSnapshot?.databases.find((candidate) => candidate.appId === dataAppId);
    if (!database || database.state !== "available" || !dataSql.trim()) {
      setError("사용 가능한 데이터베이스와 조회문을 선택하세요.");
      return;
    }
    const queryId = operationId("query");
    const requestId = ++dataRequestIdRef.current;
    dataOperationIdRef.current = queryId;
    readBusyRef.current = true;
    setReadBusy(true);
    setDataBusy(true);
    setDataResult(null);
    setError(null);
    try {
      const result = await previewDataQuery({
        appId: database.appId,
        sql: dataSql,
        queryId,
        expectedRevision: database.revision,
      });
      if (mountedRef.current && requestId === dataRequestIdRef.current) setDataResult(result);
    } catch (e) {
      if (mountedRef.current && requestId === dataRequestIdRef.current) {
        setError(e instanceof Error ? e.message : "읽기 전용 조회에 실패했습니다.");
      }
    } finally {
      if (requestId === dataRequestIdRef.current) {
        dataOperationIdRef.current = null;
        readBusyRef.current = false;
        if (mountedRef.current) {
          setDataBusy(false);
          setReadBusy(false);
        }
      }
    }
  }, [dataAppId, dataBusy, dataSnapshot, dataSql, supportBusy]);

  const onExportData = useCallback(async (format: "json" | "csv") => {
    const result = dataResult;
    if (!result || operationBusyRef.current || readBusyRef.current || dataBusy || supportBusy) return;
    readBusyRef.current = true;
    setReadBusy(true);
    setDataBusy(true);
    setError(null);
    try {
      const exportResult = await exportDataPreview(result.previewId, format);
      downloadTextFile(exportResult.filename, exportResult.mimeType, exportResult.content);
      setNotice(`${format.toUpperCase()} 파일을 준비했습니다.`);
      // Native export claims the preview before validation, so the result is
      // one-time even after a successful download. Do not leave export
      // buttons pointing at a token that the backend has already consumed.
      setDataResult(null);
    } catch (e) {
      // Stale/failed claims are also consumed to prevent replay. Require a
      // fresh native preview instead of keeping a misleading retry button.
      setDataResult(null);
      setError(e instanceof Error ? e.message : "조회 결과를 내보낼 수 없습니다.");
    } finally {
      readBusyRef.current = false;
      if (mountedRef.current) {
        setDataBusy(false);
        setReadBusy(false);
      }
    }
  }, [dataBusy, dataResult, supportBusy]);

  const onPreviewSupport = useCallback(async () => {
    if (operationBusyRef.current || readBusyRef.current || dataBusy || supportBusy) return;
    const requestId = ++supportRequestIdRef.current;
    const id = operationId("support");
    supportOperationIdRef.current = id;
    readBusyRef.current = true;
    setReadBusy(true);
    setSupportBusy(true);
    setSupportPreview(null);
    setError(null);
    try {
      const preview = await previewSupportBundle(id);
      if (mountedRef.current && requestId === supportRequestIdRef.current) setSupportPreview(preview);
    } catch (e) {
      if (mountedRef.current && requestId === supportRequestIdRef.current) {
        setError(e instanceof Error ? e.message : "지원 번들 미리 보기에 실패했습니다.");
      }
    } finally {
      if (requestId === supportRequestIdRef.current) {
        supportOperationIdRef.current = null;
        readBusyRef.current = false;
        if (mountedRef.current) {
          setSupportBusy(false);
          setReadBusy(false);
        }
      }
    }
  }, [dataBusy, supportBusy]);

  const onCancelSupport = useCallback(() => {
    const id = supportOperationIdRef.current;
    if (id) void cancelSupportBundle(id).catch(() => undefined);
  }, []);

  const onExportSupport = useCallback(async () => {
    const preview = supportPreview;
    if (!preview || operationBusyRef.current || readBusyRef.current || supportBusy || dataBusy) return;
    if (Date.now() > preview.expiresAtMs) {
      setSupportPreview(null);
      setError("지원 번들 미리 보기가 만료되었습니다. 다시 미리 확인하세요.");
      return;
    }
    readBusyRef.current = true;
    setReadBusy(true);
    setSupportBusy(true);
    setError(null);
    try {
      const exportResult = await exportSupportBundle(preview.previewId);
      downloadTextFile(exportResult.filename, exportResult.mimeType, exportResult.content);
      setNotice("redacted 지원 번들을 준비했습니다.");
      setSupportPreview(null);
    } catch (e) {
      // Support export claims/removes its token before source revalidation;
      // stale and failed attempts therefore require a fresh preview too.
      setSupportPreview(null);
      setError(e instanceof Error ? e.message : "지원 번들을 내보낼 수 없습니다.");
    } finally {
      readBusyRef.current = false;
      if (mountedRef.current) {
        setSupportBusy(false);
        setReadBusy(false);
      }
    }
  }, [dataBusy, supportBusy, supportPreview]);

  const refreshRelatedTools = useCallback(async () => {
    if (operationBusyRef.current || readBusyRef.current) return;
    const requestId = ++relatedRequestIdRef.current;
    readBusyRef.current = true;
    setReadBusy(true);
    setRelatedBusy(true);
    setRelatedError(null);
    try {
      const result = await relatedTools();
      if (mountedRef.current && requestId === relatedRequestIdRef.current) {
        setRelatedToolList(result);
      }
    } catch {
      if (mountedRef.current && requestId === relatedRequestIdRef.current) {
        setRelatedToolList([]);
        setRelatedError("관련 도구 감지를 완료할 수 없습니다. Windows 환경을 확인하세요.");
      }
    } finally {
      if (requestId === relatedRequestIdRef.current) {
        readBusyRef.current = false;
        if (mountedRef.current) {
          setReadBusy(false);
          setRelatedBusy(false);
        }
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
      removeRequestIdRef.current += 1;
      dataRequestIdRef.current += 1;
      supportRequestIdRef.current += 1;
      const dataOperationId = dataOperationIdRef.current;
      if (dataOperationId) void cancelDataDiagnostics(dataOperationId).catch(() => undefined);
      const supportOperationId = supportOperationIdRef.current;
      if (supportOperationId) void cancelSupportBundle(supportOperationId).catch(() => undefined);
      relatedRequestIdRef.current += 1;
      relatedActionIdRef.current += 1;
    };
  }, [refresh]);

  useEffect(() => {
    let alive = true;
    const applyInstallRequest = (request: { target: { kind: "install"; appId: string } }) => {
      if (!alive || request.target.kind !== "install") return;
      setTab("apps");
      setSelectedAppId(request.target.appId);
      setNotice("Launcher 요청: 선택한 앱의 설치 방법을 고르세요.");
    };
    let unlisten: (() => void) | undefined;
    void onPendingOpen((request) => applyInstallRequest(request)).then((dispose) => {
      unlisten = dispose;
      void takePendingOpen().then((request) => { if (request) applyInstallRequest(request); }).catch(() => undefined);
    }).catch(() => undefined);
    return () => { alive = false; unlisten?.(); };
  }, []);

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

  const onRelatedInstall = async (tool: RelatedTool) => {
    if (tool.installed || operationBusyRef.current || readBusyRef.current) return;
    if (!window.confirm(
      `'${tool.displayName}'을 WinGet으로 설치할까요? WinGet이 공식 패키지 설치를 진행합니다.`,
    )) return;
    const actionId = ++relatedActionIdRef.current;
    operationBusyRef.current = true;
    setBusy(`related:${tool.id}:install`);
    setError(null);
    setNotice(null);
    let shouldRefresh = false;
    try {
      const result = await installRelatedTool(tool.id, true);
      if (result.toolId !== tool.id || result.status !== "installed") {
        throw new Error("관련 도구 작업 결과가 올바르지 않습니다.");
      }
      if (mountedRef.current && actionId === relatedActionIdRef.current) {
        // The API boundary normalizes this message. Keep the local fallback
        // as a second guard for mocked/older callers.
        shouldRefresh = true;
        setNotice(result.message === "WinGet 설치가 완료되었습니다."
          ? result.message
          : "WinGet 설치가 완료되었습니다.");
      }
    } catch (e) {
      if (mountedRef.current && actionId === relatedActionIdRef.current) {
        setError(safeRelatedToolError(e));
      }
    } finally {
      operationBusyRef.current = false;
      if (mountedRef.current && actionId === relatedActionIdRef.current) setBusy(null);
    }
    if (shouldRefresh && mountedRef.current && actionId === relatedActionIdRef.current) {
      await refreshRelatedTools();
    }
  };

  const onRelatedLaunch = async (tool: RelatedTool) => {
    if (!tool.installed || operationBusyRef.current || readBusyRef.current) return;
    const actionId = ++relatedActionIdRef.current;
    operationBusyRef.current = true;
    setBusy(`related:${tool.id}:launch`);
    setError(null);
    setNotice(null);
    try {
      const result = await launchRelatedTool(tool.id);
      if (result.toolId !== tool.id || result.status !== "launched") {
        throw new Error("관련 도구 작업 결과가 올바르지 않습니다.");
      }
      if (mountedRef.current && actionId === relatedActionIdRef.current) {
        setNotice(result.message === "관련 도구를 실행했습니다."
          ? result.message
          : "관련 도구를 실행했습니다.");
      }
    } catch (e) {
      if (mountedRef.current && actionId === relatedActionIdRef.current) {
        setError(safeRelatedToolError(e));
      }
    } finally {
      operationBusyRef.current = false;
      if (mountedRef.current && actionId === relatedActionIdRef.current) setBusy(null);
    }
  };

  const onRelatedExternalLink = (url: string) => {
    void openRelatedToolUrl(url).catch(() => {
      if (mountedRef.current) setError("공식 링크를 열 수 없습니다.");
    });
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

  const onPreviewRemove = async (app: CatalogApp) => {
    if (operationBusyRef.current || readBusyRef.current) return;
    const requestId = ++removeRequestIdRef.current;
    operationBusyRef.current = true;
    setBusy(`${app.id}:remove-preview`);
    setRemovePreview(null);
    setRemoveResult(null);
    setRemovePreviewError(null);
    setError(null);
    setNotice(null);
    try {
      const preview = await previewRemoveApp(app.id);
      if (mountedRef.current && requestId === removeRequestIdRef.current) {
        setRemovePreview(preview);
        setSelectedAppId(app.id);
      }
    } catch {
      if (mountedRef.current && requestId === removeRequestIdRef.current) {
        setRemovePreviewError(REMOVE_PREVIEW_ERROR);
      }
    } finally {
      if (requestId === removeRequestIdRef.current) {
        operationBusyRef.current = false;
        if (mountedRef.current) setBusy(null);
      }
    }
  };

  const onRemove = async (app: CatalogApp) => {
    const preview = removePreview;
    if (!preview || preview.appId !== app.id || !preview.canRemove) return;
    if (operationBusyRef.current || readBusyRef.current) return;
    if (!window.confirm(
      `'${app.displayName}'의 Manager 소유 portable 파일을 제거할까요? 앱 사용자 데이터는 유지됩니다.`,
    )) return;
    const requestId = ++removeRequestIdRef.current;
    operationBusyRef.current = true;
    setBusy(`${app.id}:remove`);
    setRemovePreviewError(null);
    setError(null);
    setNotice(null);
    try {
      const request: RemoveAppRequest = {
        appId: preview.appId,
        expectedRegistryRevision: preview.registryRevision,
        expectedCatalogRevision: preview.catalogRevision,
        expectedRootId: preview.rootId,
        expectedManifestDigest: preview.manifestDigest,
      };
      const result = await removeApp(request);
      if (mountedRef.current && requestId === removeRequestIdRef.current) {
        setRemovePreview(null);
        setRemoveResult(result);
        if (result.status === "partial") setRemovePreviewError(result.message);
        // The detailed removal result already owns the live status region.
        // Repeating the same message in the global notice would announce it
        // twice and make the page expose two indistinguishable status nodes.
      }
      await refresh(true);
    } catch {
      if (mountedRef.current && requestId === removeRequestIdRef.current) {
        setRemovePreview(null);
        setRemoveResult(null);
        setRemovePreviewError(REMOVE_STALE_ERROR);
      }
    } finally {
      if (requestId === removeRequestIdRef.current) {
        operationBusyRef.current = false;
        if (mountedRef.current) setBusy(null);
      }
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
    else if (id === "remove") void onPreviewRemove(app);
  };

  const selectedDataDatabase = dataSnapshot?.databases.find(
    (database) => database.appId === dataAppId,
  ) ?? null;

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">Devbox Manager</h1>
        <button
          className={`btn ${tab === "apps" ? "active" : ""}`}
          type="button"
          aria-current={tab === "apps" ? "page" : undefined}
          disabled={batchBusy || busy !== null || installRootBusy || readBusy}
          onClick={() => setTab("apps")}
        >
          앱
        </button>
        <button
          className={`btn ${tab === "doctor" ? "active" : ""}`}
          type="button"
          aria-current={tab === "doctor" ? "page" : undefined}
          disabled={batchBusy || busy !== null || installRootBusy || readBusy}
          onClick={() => { setTab("doctor"); void onDiagnose(); }}
        >
          환경 진단
        </button>
        <button
          className={`btn ${tab === "related-tools" ? "active" : ""}`}
          type="button"
          aria-current={tab === "related-tools" ? "page" : undefined}
          disabled={batchBusy || busy !== null || installRootBusy || readBusy}
          onClick={() => {
            setTab("related-tools");
            if (relatedToolList.length === 0) void refreshRelatedTools();
          }}
        >
          관련 도구
        </button>
        <span className="latest">Latest: {manifest ? manifest.releaseTag : "..."}</span>
        <span className="spacer" />
        <button
          className="btn refresh"
          disabled={batchBusy || busy !== null || installRootBusy || readBusy}
          onClick={() => void refresh()}
        >
          Refresh
        </button>
      </header>

      {error && <div className="error" role="alert">{error}</div>}
      {notice && <div className="notice" role="status" aria-live="polite">{notice}</div>}

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

          <section className="diagnostic-tool" aria-labelledby="data-inspector-heading">
            <div className="diagnostic-tool-head">
              <div>
                <h2 id="data-inspector-heading">Data Inspector</h2>
                <p className="dim">
                  catalog가 아는 devbox 앱의 data.db만 자동 발견합니다. 경로 입력·쓰기·migration·network는 없습니다.
                </p>
              </div>
              <div className="diagnostic-tool-actions">
                {dataBusy && dataOperationIdRef.current && (
                  <button className="btn" type="button" onClick={onCancelData}>취소</button>
                )}
                <button
                  className="btn"
                  type="button"
                  disabled={dataBusy || supportBusy || installRootBusy}
                  onClick={() => void onInspectData()}
                >
                  {dataBusy ? "확인 중..." : "데이터 다시 확인"}
                </button>
              </div>
            </div>
            <div className="diagnostic-safety-note">
              read-only open · PRAGMA query_only=ON · SQLite authorizer · 2초 · 최대 1,000행 / 1 MiB ·
              secret/username/path masking
            </div>
            {dataSnapshot && (
              <>
                <div className="database-list" aria-label="발견된 devbox 데이터베이스">
                  {dataSnapshot.databases.map((database) => (
                    <button
                      key={database.appId}
                      type="button"
                      className={`database-card ${dataAppId === database.appId ? "selected" : ""} ${database.state}`}
                      disabled={database.state !== "available" || dataBusy || supportBusy}
                      onClick={() => {
                        setDataAppId(database.appId);
                        setDataResult(null);
                      }}
                    >
                      <span className="database-card-name">{database.displayName}</span>
                      <span className="database-card-state">{dataStateLabel(database.state)}</span>
                      {database.state === "available" && (
                        <span className="database-card-meta">
                          table {database.tables.length} · view {database.views.length} · integrity {dataIntegrityLabel(database.integrity)}
                        </span>
                      )}
                      {database.warning && <span className="database-card-warning">{database.warning}</span>}
                    </button>
                  ))}
                </div>
                {selectedDataDatabase && selectedDataDatabase.state === "available" && (
                  <div className="database-schema" aria-label="SQLite schema 요약">
                    <strong>{selectedDataDatabase.displayName} schema</strong>
                    <div className="schema-items">
                      {selectedDataDatabase.tables.map((table) => (
                        <span key={`table:${table.name}`} className="schema-item">
                          table {table.name} ({table.rowCount == null ? "?" : table.rowCount} rows)
                        </span>
                      ))}
                      {selectedDataDatabase.views.map((view) => (
                        <span key={`view:${view.name}`} className="schema-item">view {view.name}</span>
                      ))}
                      {selectedDataDatabase.tables.length === 0 && selectedDataDatabase.views.length === 0 && (
                        <span className="dim">표시할 table/view가 없습니다.</span>
                      )}
                    </div>
                    <div className="dim schema-note">schema version {selectedDataDatabase.schemaVersion ?? "?"} · database path는 표시하지 않습니다.</div>
                  </div>
                )}
                <div className="query-panel">
                  <label htmlFor="data-query">읽기 전용 SQL preview</label>
                  <textarea
                    id="data-query"
                    value={dataSql}
                    maxLength={16 * 1024}
                    disabled={dataBusy || supportBusy || !selectedDataDatabase || selectedDataDatabase.state !== "available"}
                    spellCheck={false}
                    rows={3}
                    onChange={(event) => {
                      setDataSql(event.target.value);
                      setDataResult(null);
                    }}
                  />
                  <div className="query-actions">
                    <span className="dim">SELECT/WITH/EXPLAIN만 허용 · PRAGMA/ATTACH/쓰기문 차단</span>
                    <button
                      className="btn primary"
                      type="button"
                      disabled={dataBusy || supportBusy || !selectedDataDatabase || selectedDataDatabase.state !== "available" || !dataSql.trim()}
                      onClick={() => void onPreviewData()}
                    >
                      {dataBusy ? "조회 중..." : "미리 보기"}
                    </button>
                  </div>
                  {dataResult && (
                    <div className="query-result" aria-live="polite">
                      <div className="query-result-head">
                        <strong>조회 결과 preview</strong>
                        <span className="dim">{dataResult.rowCount} rows · {formatBytes(dataResult.resultBytes)} · {dataResult.elapsedMs} ms{dataResult.truncated ? " · 일부 결과만 표시" : ""}</span>
                      </div>
                      <div className="query-result-table-wrap">
                        <table className="query-result-table">
                          <thead><tr>{dataResult.columns.map((column) => <th key={column}>{column}</th>)}</tr></thead>
                          <tbody>
                            {dataResult.rows.map((row, rowIndex) => (
                              <tr key={`query-row-${rowIndex}`}>
                                {row.map((value, columnIndex) => <td key={`${rowIndex}:${columnIndex}`}>{value == null ? <span className="dim">null</span> : String(value)}</td>)}
                              </tr>
                            ))}
                          </tbody>
                        </table>
                      </div>
                      <div className="query-export-actions">
                        <span className="dim">내보내기 전 preview를 검토하세요. credential·path·raw body는 backend에서 masking됩니다.</span>
                        <button className="btn" type="button" disabled={dataBusy || supportBusy} onClick={() => void onExportData("json")}>JSON export</button>
                        <button className="btn" type="button" disabled={dataBusy || supportBusy} onClick={() => void onExportData("csv")}>CSV export</button>
                      </div>
                    </div>
                  )}
                </div>
              </>
            )}
            {!dataSnapshot && <div className="dim diagnostic-empty">데이터 다시 확인을 눌러 catalog 기반 DB를 발견하세요.</div>}
          </section>

          <section className="diagnostic-tool support-tool" aria-labelledby="support-bundle-heading">
            <div className="diagnostic-tool-head">
              <div>
                <h2 id="support-bundle-heading">Redacted support bundle</h2>
                <p className="dim">app/catalog/schema/log metadata와 진단 상태만 포함하며 raw DB·raw log·path·user·secret·Authorization/Cookie는 포함하지 않습니다.</p>
              </div>
              <div className="diagnostic-tool-actions">
                {supportBusy && supportOperationIdRef.current && (
                  <button className="btn" type="button" onClick={onCancelSupport}>취소</button>
                )}
                <button
                  className="btn"
                  type="button"
                  disabled={supportBusy || dataBusy || installRootBusy}
                  onClick={() => void onPreviewSupport()}
                >
                  {supportBusy ? "준비 중..." : "번들 미리 확인"}
                </button>
              </div>
            </div>
            {supportPreview && (
              <div className="support-preview" role="status" aria-live="polite">
                <div className="query-result-head">
                  <strong>내보내기 preview · redaction {supportPreview.redactionVersion}</strong>
                  <span className="dim">{formatBytes(supportPreview.estimatedBytes)} · DB {supportPreview.databaseCount}개 · 5분 이내 1회 export</span>
                </div>
                <div className="support-sections">
                  <div><strong>포함</strong>{supportPreview.includedSections.map((section) => <span key={section}>{section}</span>)}</div>
                  <div><strong>제외</strong>{supportPreview.omittedSections.map((section) => <span key={section}>{section}</span>)}</div>
                </div>
                <div className="query-export-actions">
                  <span className="dim">진단 상태가 바뀌면 stale로 중단되며 새 preview가 필요합니다.</span>
                  <button className="btn primary" type="button" disabled={supportBusy} onClick={() => void onExportSupport()}>확인 후 JSON export</button>
                  <button className="btn" type="button" disabled={supportBusy} onClick={() => setSupportPreview(null)}>취소</button>
                </div>
              </div>
            )}
            {!supportPreview && <div className="dim diagnostic-empty">번들 미리 확인 후 포함/제외 범위를 검토할 수 있습니다.</div>}
          </section>
        </div>
      ) : tab === "related-tools" ? (
        <section
          className="related-tools"
          aria-busy={relatedBusy || readBusy}
          aria-labelledby="related-tools-heading"
        >
          <div className="related-tools-head">
            <div>
              <h2 id="related-tools-heading">Related Tools</h2>
              <p className="dim">
                개발 흐름을 보완하는 작은 공식 도구 목록입니다. 설치 여부만 로컬에서 감지하며 경로와 버전은 표시하지 않습니다.
                감지와 이미 설치된 도구 실행은 인터넷 없이 가능합니다. WinGet 설치는 Windows와 네트워크가 필요하고, 공식·라이선스 링크는 플랫폼과 관계없이 네트워크 연결 시 열 수 있습니다.
              </p>
            </div>
            <button
              className="btn"
              type="button"
              disabled={batchBusy || busy !== null || installRootBusy || readBusy}
              onClick={() => void refreshRelatedTools()}
            >
              {relatedBusy ? "감지 중..." : "다시 감지"}
            </button>
          </div>
          {relatedError && <div className="error related-tools-error" role="alert">{relatedError}</div>}
          {relatedToolList.length === 0 && !relatedBusy && !relatedError && (
            <div className="dim related-tools-empty" role="status" aria-live="polite">
              관련 도구 목록을 확인하려면 다시 감지를 누르세요.
            </div>
          )}
          <div className="related-tools-grid">
            {relatedToolList.map((tool) => {
              const officialUrl = safeExternalUrl(tool.officialUrl);
              const licenseUrl = safeExternalUrl(tool.licenseUrl);
              return (
                <article key={tool.id} className={`related-tool-card ${tool.installed ? "installed" : ""}`}>
                  <div className="related-tool-card-head">
                    <div>
                      <h3>{tool.displayName}</h3>
                      <p className="dim">{tool.summary}</p>
                    </div>
                    <span className={`related-tool-state ${tool.installed ? "ok" : "dim"}`}>
                      {tool.installed ? "설치됨" : "미설치"}
                    </span>
                  </div>
                  <dl className="related-tool-facts">
                    <div><dt>감지</dt><dd>{relatedDetectionDescription(tool)}</dd></div>
                    <div><dt>WinGet ID</dt><dd><code>{tool.wingetId}</code></dd></div>
                    <div><dt>라이선스</dt><dd>{tool.license}</dd></div>
                  </dl>
                  <div className="related-tool-links">
                    {officialUrl && (
                      <a
                        href={officialUrl}
                        target="_blank"
                        rel="noreferrer noopener"
                        onClick={(event) => {
                          event.preventDefault();
                          onRelatedExternalLink(officialUrl);
                        }}
                      >
                        공식 사이트
                      </a>
                    )}
                    {licenseUrl && (
                      <a
                        href={licenseUrl}
                        target="_blank"
                        rel="noreferrer noopener"
                        onClick={(event) => {
                          event.preventDefault();
                          onRelatedExternalLink(licenseUrl);
                        }}
                      >
                        라이선스
                      </a>
                    )}
                  </div>
                  <div className="related-tool-actions">
                    {tool.installed ? (
                      <button
                        className="btn"
                        type="button"
                        aria-busy={busy === `related:${tool.id}:launch`}
                        disabled={batchBusy || busy !== null || installRootBusy || readBusy}
                        onClick={() => void onRelatedLaunch(tool)}
                      >
                        {busy === `related:${tool.id}:launch` ? "실행 중..." : "실행"}
                      </button>
                    ) : (
                      <button
                        className="btn"
                        type="button"
                        aria-busy={busy === `related:${tool.id}:install`}
                        disabled={!tool.platformSupported || batchBusy || busy !== null || installRootBusy || readBusy}
                        title={tool.platformSupported ? undefined : "WinGet 설치는 Windows에서만 사용할 수 있습니다."}
                        onClick={() => void onRelatedInstall(tool)}
                      >
                        {busy === `related:${tool.id}:install`
                          ? "설치 중..."
                          : tool.platformSupported ? "확인 후 WinGet 설치" : "WinGet 설치: Windows 전용"}
                      </button>
                    )}
                  </div>
                </article>
              );
            })}
          </div>
          <p className="dim related-tools-note">
            Manager의 native 기능이 항상 기본 동작이며, 외부 도구는 선택적 보완재입니다. 자동 업데이트·제거·광범위한 WinGet 검색은 지원하지 않습니다.
          </p>
        </section>
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
        {(removePreview || removePreviewError || removeResult) && (
          <section
            className="remove-preview-panel"
            aria-label="제거 대상 미리 보기"
            aria-busy={busy?.endsWith(":remove-preview") || busy?.endsWith(":remove") || undefined}
          >
            <div className="remove-preview-head">
              <div>
                <h2>제거 대상 미리 보기</h2>
                {removePreview && (
                  <strong>
                    {apps.find((candidate) => candidate.id === removePreview.appId)?.displayName
                      ?? removePreview.appId}
                  </strong>
                )}
              </div>
              {removePreview && <span className="read-only-tag">검증 후 확인</span>}
            </div>
            {removePreview && (
              <>
                <p className="dim">{removalStateDescription(removePreview)}</p>
                {removePreview.targetPath && (
                  <code className="remove-preview-target">{removePreview.targetPath}</code>
                )}
                <dl className="remove-preview-facts">
                  <div><dt>방식</dt><dd>{removePreview.mode === "portable" ? "휴대용" : "설치 패키지"}</dd></div>
                  <div><dt>버전</dt><dd>{removePreview.version}</dd></div>
                  <div><dt>Manager 소유 항목</dt><dd>{removePreview.ownedEntryCount}개 · {formatBytes(removePreview.ownedBytes)}</dd></div>
                  <div><dt>앱 사용자 데이터</dt><dd>{removePreview.preservesUserData ? "보존" : "삭제"}</dd></div>
                </dl>
                {removePreview.canRemove && (
                  <div className="remove-preview-actions">
                    <button
                      className="btn danger"
                      type="button"
                      disabled={batchBusy || busy !== null || installRootBusy || readBusy}
                      onClick={() => {
                        const target = apps.find((candidate) => candidate.id === removePreview.appId);
                        if (target) void onRemove(target);
                      }}
                    >
                      {busy?.endsWith(":remove") ? "제거 중..." : "확인 후 제거"}
                    </button>
                    <button
                      className="btn"
                      type="button"
                      disabled={busy !== null}
                      onClick={() => setRemovePreview(null)}
                    >
                      취소
                    </button>
                  </div>
                )}
              </>
            )}
            {removePreviewError && (
              <div className="error remove-preview-error" role="alert">{removePreviewError}</div>
            )}
            {removeResult && (
              <div className={`remove-result ${removeResult.status === "partial" ? "bad" : "ok"}`} role="status" aria-live="polite">
                <strong>{removeResult.status === "partial" ? "부분 제거" : "제거 완료"}</strong>
                <span>{removeResult.message}</span>
                <span>제거 {removeResult.removedEntryCount}개 · 남음 {removeResult.remainingEntryCount}개 · 사용자 데이터 보존</span>
              </div>
            )}
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
