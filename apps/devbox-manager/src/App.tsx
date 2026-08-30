import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { isKeyboardActivation } from "@devbox/a11y";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  available,
  applyDevSetupConfiguration,
  applyInstallRoot,
  catalog,
  cancelDataDiagnostics,
  cancelDevSetupApply,
  cancelSupportBundle,
  current,
  devSetupAudit,
  discardDevSetupConfiguration,
  exportDevSetupConfiguration,
  exportDataPreview,
  exportSupportBundle,
  inspectDataDatabases,
  importDevSetupConfiguration,
  installApp,
  installPath,
  installMany,
  installed,
  inspectLocalQuality,
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
  DevSetupAudit,
  DevSetupCapability,
  DevSetupPlanItem,
  InstalledApp,
  InstallPathInfo,
  InstallRootPreview,
  InstallMode,
  LocalQualityIssueKind,
  LocalQualitySnapshot,
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
      return "검증된 빈 디렉터리입니다. 적용을 누르면 다음 설치부터 이 루트를 사용합니다.";
    case "already-active":
      return "현재 설치 루트와 같습니다. 파일은 변경되지 않습니다.";
    case "existing-install":
      return "현재 루트에 설치 기록 또는 관리 파일이 있어 자동 이동하지 않습니다.";
    case "candidate-conflict":
      return "기존 파일이 있는 디렉터리는 덮어쓰지 않습니다.";
    case "permission-denied":
      return "설치 루트에 쓸 권한이 없어 적용하지 않습니다.";
    case "insufficient-free-space":
      return "필수 여유 공간을 확보한 뒤 다시 미리 확인하세요.";
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

const RELATED_TOOL_ERROR_DISPLAY: Readonly<Record<string, string>> = {
  "Related Tools는 Windows에서만 사용할 수 있습니다.": "관련 도구는 Windows에서만 사용할 수 있습니다.",
};

function safeRelatedToolError(error: unknown): string {
  const message = error instanceof Error
    ? error.message
    : typeof error === "string" ? error : "";
  return RELATED_TOOL_SAFE_ERRORS.has(message)
    ? RELATED_TOOL_ERROR_DISPLAY[message] ?? message
    : RELATED_TOOL_GENERIC_ERROR;
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

function formatFreshness(milliseconds: number): string {
  if (milliseconds < 60_000) return "1분 미만";
  const minutes = Math.floor(milliseconds / 60_000);
  if (minutes < 60) return `${minutes}분`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}시간`;
  return `${Math.floor(hours / 24)}일`;
}

function localQualityIssueLabel(kind: LocalQualityIssueKind): string {
  switch (kind) {
    case "invalid": return "형식 검증 실패";
    case "unreadable": return "읽기 실패";
    case "unsafe": return "안전하지 않은 파일 형식";
    case "limit-exceeded": return "검사 상한 초과";
  }
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

function installCapabilityLabel(state: RelatedTool["installState"]): string {
  if (state === "present") return "설치됨";
  if (state === "absent") return "미설치";
  return "설치 상태 확인 필요";
}

function availabilityCapabilityLabel(state: RelatedTool["launchState"]): string {
  if (state === "available") return "사용 가능";
  if (state === "unavailable") return "사용 불가";
  return "확인 필요";
}

function backendCapabilityLabel(state: NonNullable<RelatedTool["dockerCapability"]>["wslBackend"]): string {
  switch (state) {
    case "running": return "실행 중";
    case "stopped": return "중지됨";
    case "present": return "등록됨 · 실행 상태 확인 불가";
    case "absent": return "등록되지 않음";
    case "unknown": return "확인 필요";
  }
}

const EVIDENCE_LABELS: Record<string, string> = {
  "desktop-executable:path": "Windows PATH 실행 파일",
  "desktop-executable:known-location": "검토된 설치 위치 실행 파일",
  "desktop-executable:not-observed": "검토 위치에서 실행 파일 미확인",
  "desktop-executable:unavailable": "Windows 실행 파일 검사 불가",
  "windows-cli:path": "Windows PATH의 공식 Docker CLI",
  "windows-cli:known-location": "Docker 설치 위치의 공식 CLI",
  "windows-cli:not-observed": "Windows Docker CLI 미확인",
  "windows-cli:unrecognized": "docker 호환 명령의 제품 확인 불가",
  "windows-cli:unavailable": "Windows CLI 검사 불가",
  "wsl-registration:registered": "docker-desktop WSL 등록 확인",
  "wsl-registration:not-registered": "docker-desktop WSL 미등록",
  "wsl-registration:unavailable": "WSL 등록 상태 확인 불가",
  "wsl-runtime:running": "docker-desktop WSL 실행 중",
  "wsl-runtime:stopped": "docker-desktop WSL 중지됨",
  "wsl-runtime:not-observed": "docker-desktop WSL 실행 미확인",
  "wsl-runtime:unavailable": "WSL 실행 상태 확인 불가",
  "winget-executable:trusted-location": "검토된 Windows 위치의 WinGet",
  "winget-executable:not-observed": "WinGet 실행 파일 미확인",
  "winget-executable:unavailable": "WinGet 검사 불가",
};

function evidenceLabel(source: string, result: string): string {
  return EVIDENCE_LABELS[`${source}:${result}`] ?? "검증할 수 없는 근거";
}

const DEV_SETUP_CAPABILITY_LABELS: Record<DevSetupCapability["id"], string> = {
  "docker-desktop-install": "Docker Desktop 설치",
  "docker-desktop-launch": "Docker Desktop 실행",
  "docker-windows-cli": "Windows Docker CLI",
  "docker-wsl-backend": "docker-desktop WSL backend",
  winget: "WinGet",
};

const DEV_SETUP_ACTION_LABELS: Record<DevSetupPlanItem["action"], string> = {
  none: "추가 조치 없음",
  "review-install": "공식 패키지 설치 검토",
  "verify-installation": "설치·실행 환경 직접 확인",
  "review-launch-path": "Docker Desktop 실행 위치 확인",
  "review-cli": "Windows Docker CLI 설치 또는 PATH 확인",
  "start-backend": "Docker Desktop에서 backend 시작 확인",
  "review-backend": "Docker Desktop WSL 통합 설정 확인",
  "review-winget": "Windows App Installer 상태 확인",
};

type DevSetupConfigurationReview = NonNullable<
  Awaited<ReturnType<typeof importDevSetupConfiguration>>
>;
type DevSetupConfigurationPackage = DevSetupConfigurationReview["packages"][number];
type DevSetupConfigurationApplyResult = Awaited<
  ReturnType<typeof applyDevSetupConfiguration>
>;

const DEV_SETUP_CONFIGURATION_IMPORT_ERROR =
  "WinGet Configuration v3 파일을 불러올 수 없습니다. Microsoft.WinGet/Package와 고정된 winget source 이름만 지원합니다.";
const DEV_SETUP_CONFIGURATION_EXPORT_ERROR =
  "정규화된 WinGet Configuration을 내보낼 수 없습니다.";
const DEV_SETUP_CONFIGURATION_APPLY_ERROR =
  "Dev Setup 구성을 적용할 수 없습니다. 만료되었거나 최신 검토가 필요합니다.";
const DEV_SETUP_CONFIGURATION_CANCEL_ERROR =
  "Dev Setup 적용 취소를 완료할 수 없습니다.";
const DEV_SETUP_CONFIGURATION_DISCARD_ERROR =
  "Dev Setup 구성 검토를 폐기할 수 없습니다.";
const DEV_SETUP_CONFIGURATION_EXPIRED =
  "Dev Setup 적용 미리 보기가 만료되었습니다. 구성을 다시 가져오세요.";
const DEV_SETUP_CONFIGURATION_CANCELLED = "Dev Setup 적용을 취소했습니다.";

const DEV_SETUP_CONFIGURATION_DESIRED_LABELS: Record<string, string> = {
  present: "설치됨",
  latest: "최신 버전",
  version: "지정 버전",
};

const DEV_SETUP_CONFIGURATION_STATE_LABELS: Record<string, string> = {
  present: "설치됨",
  absent: "미설치",
  "update-available": "업데이트 가능",
  unknown: "확인 불가",
};

const DEV_SETUP_CONFIGURATION_ACTION_LABELS: Record<string, string> = {
  none: "변경 없음",
  install: "설치",
  update: "업데이트",
  "reconcile-version": "지정 버전으로 맞춤",
  verify: "상태 확인 필요",
};

const DEV_SETUP_APPLY_STATUS_LABELS: Record<string, string> = {
  complete: "전체 적용 완료",
  partial: "일부 적용",
  cancelled: "취소됨",
  unchanged: "변경 없음",
  applied: "적용 완료",
  failed: "실패",
  "timed-out": "시간 초과",
  skipped: "건너뜀",
};

function devSetupConfigurationDesiredLabel(packageReview: DevSetupConfigurationPackage): string {
  const label = DEV_SETUP_CONFIGURATION_DESIRED_LABELS[packageReview.desired]
    ?? packageReview.desired;
  return packageReview.desired === "version" && packageReview.version
    ? `${label} ${packageReview.version}`
    : label;
}

function devSetupConfigurationStateLabel(state: string): string {
  return DEV_SETUP_CONFIGURATION_STATE_LABELS[state] ?? "확인 불가";
}

function devSetupConfigurationActionLabel(action: string): string {
  return DEV_SETUP_CONFIGURATION_ACTION_LABELS[action] ?? "확인 필요";
}

function devSetupApplyStatusLabel(status: string): string {
  return DEV_SETUP_APPLY_STATUS_LABELS[status] ?? "확인 필요";
}

function devSetupStateLabel(capability: DevSetupCapability): string {
  if (capability.id === "docker-desktop-install") {
    return installCapabilityLabel(capability.state as RelatedTool["installState"]);
  }
  if (capability.id === "docker-wsl-backend") {
    return backendCapabilityLabel(
      capability.state as NonNullable<RelatedTool["dockerCapability"]>["wslBackend"],
    );
  }
  return availabilityCapabilityLabel(capability.state as RelatedTool["launchState"]);
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
  const [tab, setTab] = useState<"apps" | "local-quality" | "doctor" | "dev-setup" | "related-tools">("apps");
  const [diagnosis, setDiagnosis] = useState<DiagnosisItem[]>([]);
  const [localQualitySnapshot, setLocalQualitySnapshot] = useState<LocalQualitySnapshot | null>(null);
  const [localQualityBusy, setLocalQualityBusy] = useState(false);
  const [localQualityError, setLocalQualityError] = useState<string | null>(null);
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
  const [devSetupSnapshot, setDevSetupSnapshot] = useState<DevSetupAudit | null>(null);
  const [devSetupBusy, setDevSetupBusy] = useState(false);
  const [devSetupError, setDevSetupError] = useState<string | null>(null);
  const [devSetupConfigurationReview, setDevSetupConfigurationReview] =
    useState<DevSetupConfigurationReview | null>(null);
  const [devSetupConfigurationBusy, setDevSetupConfigurationBusy] = useState(false);
  const [devSetupConfigurationError, setDevSetupConfigurationError] = useState<string | null>(null);
  const [devSetupConfigurationNotice, setDevSetupConfigurationNotice] = useState<string | null>(null);
  const [devSetupConfigurationResult, setDevSetupConfigurationResult] =
    useState<DevSetupConfigurationApplyResult | null>(null);
  const [devSetupConfigurationConsumed, setDevSetupConfigurationConsumed] = useState(false);
  const [devSetupApplyInFlight, setDevSetupApplyInFlight] = useState(false);
  const [devSetupConfigurationClockMs, setDevSetupConfigurationClockMs] = useState(() => Date.now());
  const [devSetupReviewAcknowledged, setDevSetupReviewAcknowledged] = useState(false);
  const [devSetupAgreementsAccepted, setDevSetupAgreementsAccepted] = useState(false);
  const [devSetupAdminRiskAcknowledged, setDevSetupAdminRiskAcknowledged] = useState(false);
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
  const localQualityRequestIdRef = useRef(0);
  const removeRequestIdRef = useRef(0);
  const dataRequestIdRef = useRef(0);
  const dataOperationIdRef = useRef<string | null>(null);
  const supportRequestIdRef = useRef(0);
  const supportOperationIdRef = useRef<string | null>(null);
  const relatedRequestIdRef = useRef(0);
  const relatedActionIdRef = useRef(0);
  const devSetupRequestIdRef = useRef(0);
  const devSetupConfigurationRequestIdRef = useRef(0);
  const devSetupConfigurationApplyRequestIdRef = useRef(0);
  const devSetupConfigurationApplyBusyRef = useRef(false);

  useEffect(() => {
    const expiresAtMs = devSetupConfigurationReview?.expiresAtMs;
    if (expiresAtMs == null) return undefined;
    let timeout: number | undefined;
    const refreshExpiry = () => {
      const remainingMs = expiresAtMs - Date.now();
      if (remainingMs <= 0) {
        setDevSetupConfigurationClockMs(Date.now());
        return;
      }
      // Timers can fire a little early on a busy event loop or under fake
      // timers. Re-check the deadline and leave a small safety margin so an
      // unexpired preview never gets stuck in the non-expired state.
      timeout = window.setTimeout(refreshExpiry, Math.max(50, remainingMs + 50));
    };
    refreshExpiry();
    return () => {
      if (timeout !== undefined) window.clearTimeout(timeout);
    };
  }, [devSetupConfigurationReview?.expiresAtMs]);

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

  const onInspectLocalQuality = useCallback(async () => {
    if (operationBusyRef.current || readBusyRef.current) return;
    const requestId = ++localQualityRequestIdRef.current;
    readBusyRef.current = true;
    setReadBusy(true);
    setLocalQualityBusy(true);
    setLocalQualityError(null);
    try {
      const snapshot = await inspectLocalQuality();
      if (mountedRef.current && requestId === localQualityRequestIdRef.current) {
        setLocalQualitySnapshot(snapshot);
      }
    } catch {
      if (mountedRef.current && requestId === localQualityRequestIdRef.current) {
        setLocalQualityError(
          localQualitySnapshot
            ? "최신 로컬 품질 상태를 확인하지 못했습니다. 이전 결과를 유지합니다."
            : "로컬 품질 상태를 확인하지 못했습니다. 잠시 후 다시 시도하세요.",
        );
      }
    } finally {
      if (requestId === localQualityRequestIdRef.current) {
        readBusyRef.current = false;
        if (mountedRef.current) {
          setReadBusy(false);
          setLocalQualityBusy(false);
        }
      }
    }
  }, [localQualitySnapshot]);

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

  const refreshDevSetup = useCallback(async () => {
    if (operationBusyRef.current || readBusyRef.current) return;
    const requestId = ++devSetupRequestIdRef.current;
    readBusyRef.current = true;
    setReadBusy(true);
    setDevSetupBusy(true);
    setDevSetupError(null);
    try {
      const result = await devSetupAudit();
      if (mountedRef.current && requestId === devSetupRequestIdRef.current) {
        setDevSetupSnapshot(result);
      }
    } catch {
      if (mountedRef.current && requestId === devSetupRequestIdRef.current) {
        setDevSetupSnapshot(null);
        setDevSetupError("Dev Setup 감사를 완료할 수 없습니다. Windows와 WSL 환경을 확인하세요.");
      }
    } finally {
      if (requestId === devSetupRequestIdRef.current) {
        readBusyRef.current = false;
        if (mountedRef.current) {
          setReadBusy(false);
          setDevSetupBusy(false);
        }
      }
    }
  }, []);

  const onImportDevSetupConfiguration = useCallback(async () => {
    if (
      operationBusyRef.current
      || readBusyRef.current
      || devSetupConfigurationBusy
      || devSetupConfigurationApplyBusyRef.current
    ) return;
    const requestId = ++devSetupConfigurationRequestIdRef.current;
    readBusyRef.current = true;
    setReadBusy(true);
    setDevSetupConfigurationBusy(true);
    setDevSetupConfigurationError(null);
    setDevSetupConfigurationNotice(null);
    // The API invalidates the previous native preview as soon as a new
    // import starts, including when the picker is cancelled or parsing fails.
    // Clear local review state before awaiting that result so an old token
    // cannot remain actionable in the renderer.
    setDevSetupConfigurationReview(null);
    setDevSetupConfigurationResult(null);
    setDevSetupConfigurationConsumed(false);
    setDevSetupReviewAcknowledged(false);
    setDevSetupAgreementsAccepted(false);
    setDevSetupAdminRiskAcknowledged(false);
    try {
      const review = await importDevSetupConfiguration();
      if (mountedRef.current && requestId === devSetupConfigurationRequestIdRef.current && review) {
        setDevSetupConfigurationClockMs(Date.now());
        setDevSetupConfigurationReview(review);
        setDevSetupConfigurationNotice("구성을 정규화했습니다. 적용 전에 패키지와 안전 확인을 검토하세요.");
      }
    } catch {
      if (mountedRef.current && requestId === devSetupConfigurationRequestIdRef.current) {
        setDevSetupConfigurationError(DEV_SETUP_CONFIGURATION_IMPORT_ERROR);
      }
    } finally {
      if (requestId === devSetupConfigurationRequestIdRef.current) {
        readBusyRef.current = false;
        if (mountedRef.current) {
          setDevSetupConfigurationBusy(false);
          setReadBusy(false);
        }
      }
    }
  }, [devSetupConfigurationBusy]);

  const onExportDevSetupConfiguration = useCallback(async () => {
    const review = devSetupConfigurationReview;
    if (
      !review
      || devSetupConfigurationConsumed
      || devSetupConfigurationBusy
      || operationBusyRef.current
      || readBusyRef.current
    ) return;
    if (Date.now() >= review.expiresAtMs) {
      setDevSetupConfigurationReview(null);
      setDevSetupReviewAcknowledged(false);
      setDevSetupAgreementsAccepted(false);
      setDevSetupAdminRiskAcknowledged(false);
      setDevSetupConfigurationError(DEV_SETUP_CONFIGURATION_EXPIRED);
      return;
    }
    const requestId = ++devSetupConfigurationRequestIdRef.current;
    readBusyRef.current = true;
    setReadBusy(true);
    setDevSetupConfigurationBusy(true);
    setDevSetupConfigurationError(null);
    setDevSetupConfigurationNotice(null);
    try {
      const exportResult = await exportDevSetupConfiguration(review.previewId);
      if (mountedRef.current && requestId === devSetupConfigurationRequestIdRef.current) {
        downloadTextFile(exportResult.filename, exportResult.mimeType, exportResult.content);
        setDevSetupConfigurationNotice("정규화된 package-only 구성을 저장했습니다.");
      }
    } catch {
      if (mountedRef.current && requestId === devSetupConfigurationRequestIdRef.current) {
        setDevSetupConfigurationError(DEV_SETUP_CONFIGURATION_EXPORT_ERROR);
      }
    } finally {
      if (requestId === devSetupConfigurationRequestIdRef.current) {
        readBusyRef.current = false;
        if (mountedRef.current) {
          setDevSetupConfigurationBusy(false);
          setReadBusy(false);
        }
      }
    }
  }, [devSetupConfigurationBusy, devSetupConfigurationConsumed, devSetupConfigurationReview]);

  const onCancelDevSetupApply = useCallback(() => {
    if (!devSetupConfigurationApplyBusyRef.current) return;
    void cancelDevSetupApply().catch(() => {
      if (mountedRef.current) setDevSetupConfigurationError(DEV_SETUP_CONFIGURATION_CANCEL_ERROR);
    });
  }, []);

  const onDiscardDevSetupConfiguration = useCallback(async () => {
    const review = devSetupConfigurationReview;
    if (
      !review
      || devSetupConfigurationBusy
      || devSetupConfigurationApplyBusyRef.current
      || operationBusyRef.current
      || readBusyRef.current
    ) return;
    const requestId = ++devSetupConfigurationRequestIdRef.current;
    readBusyRef.current = true;
    setReadBusy(true);
    setDevSetupConfigurationBusy(true);
    setDevSetupConfigurationError(null);
    setDevSetupConfigurationNotice(null);
    try {
      await discardDevSetupConfiguration(review.previewId);
      if (mountedRef.current && requestId === devSetupConfigurationRequestIdRef.current) {
        devSetupConfigurationApplyRequestIdRef.current += 1;
        setDevSetupConfigurationReview(null);
        setDevSetupConfigurationResult(null);
        setDevSetupConfigurationConsumed(false);
        setDevSetupReviewAcknowledged(false);
        setDevSetupAgreementsAccepted(false);
        setDevSetupAdminRiskAcknowledged(false);
      }
    } catch {
      if (mountedRef.current && requestId === devSetupConfigurationRequestIdRef.current) {
        setDevSetupConfigurationError(DEV_SETUP_CONFIGURATION_DISCARD_ERROR);
      }
    } finally {
      if (requestId === devSetupConfigurationRequestIdRef.current) {
        readBusyRef.current = false;
        if (mountedRef.current) {
          setReadBusy(false);
          setDevSetupConfigurationBusy(false);
        }
      }
    }
  }, [devSetupConfigurationBusy, devSetupConfigurationReview]);

  const onApplyDevSetupConfiguration = useCallback(async () => {
    const review = devSetupConfigurationReview;
    const hasUnknownPackage = review?.packages.some((packageReview) => (
      packageReview.currentState === "unknown" || packageReview.action === "verify"
    )) ?? false;
    if (
      !review
      || devSetupConfigurationConsumed
      || devSetupConfigurationBusy
      || devSetupConfigurationApplyBusyRef.current
      || operationBusyRef.current
      || readBusyRef.current
    ) return;
    if (Date.now() >= review.expiresAtMs) {
      setDevSetupConfigurationError(DEV_SETUP_CONFIGURATION_EXPIRED);
      return;
    }
    if (hasUnknownPackage) {
      setDevSetupConfigurationError("확인할 수 없는 패키지 상태가 있어 적용할 수 없습니다. 설치를 제안하지 않습니다.");
      return;
    }
    if (
      !review.canApply
      || !review.hasChanges
      || !devSetupReviewAcknowledged
      || !devSetupAgreementsAccepted
      || !devSetupAdminRiskAcknowledged
    ) return;
    if (!window.confirm(
      "정규화된 package-only 구성의 패키지 변경을 적용할까요? 네트워크와 UAC/관리자 권한이 필요할 수 있으며 자동 재부팅은 실행하지 않습니다.",
    )) return;

    const requestId = ++devSetupConfigurationApplyRequestIdRef.current;
    devSetupConfigurationApplyBusyRef.current = true;
    operationBusyRef.current = true;
    // The native command consumes this preview before starting the first
    // package. Keep the token unavailable even if the process later fails.
    setDevSetupConfigurationConsumed(true);
    setDevSetupApplyInFlight(true);
    setDevSetupConfigurationBusy(true);
    setDevSetupConfigurationError(null);
    setDevSetupConfigurationNotice(null);
    setDevSetupConfigurationResult(null);
    setBusy("dev-setup:apply");
    try {
      const result = await applyDevSetupConfiguration(
        review.previewId,
        true,
        true,
        true,
      );
      if (mountedRef.current && requestId === devSetupConfigurationApplyRequestIdRef.current) {
        setDevSetupConfigurationResult(result);
        setDevSetupConfigurationNotice(result.status === "complete"
          ? "Dev Setup 패키지 적용이 완료되었습니다."
          : result.status === "cancelled"
            ? DEV_SETUP_CONFIGURATION_CANCELLED
            : "Dev Setup 패키지 적용이 일부 완료되었습니다. 결과를 확인하세요.");
      }
    } catch {
      if (mountedRef.current && requestId === devSetupConfigurationApplyRequestIdRef.current) {
        setDevSetupConfigurationError(DEV_SETUP_CONFIGURATION_APPLY_ERROR);
      }
    } finally {
      devSetupConfigurationApplyBusyRef.current = false;
      operationBusyRef.current = false;
      if (mountedRef.current && requestId === devSetupConfigurationApplyRequestIdRef.current) {
        setDevSetupApplyInFlight(false);
        setDevSetupConfigurationBusy(false);
        setBusy(null);
      }
    }
  }, [
    devSetupAgreementsAccepted,
    devSetupConfigurationBusy,
    devSetupConfigurationConsumed,
    devSetupConfigurationReview,
    devSetupAdminRiskAcknowledged,
    devSetupReviewAcknowledged,
  ]);

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
      localQualityRequestIdRef.current += 1;
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
      devSetupRequestIdRef.current += 1;
      devSetupConfigurationRequestIdRef.current += 1;
      devSetupConfigurationApplyRequestIdRef.current += 1;
      if (devSetupConfigurationApplyBusyRef.current) {
        void cancelDevSetupApply().catch(() => undefined);
      }
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
        setInstallRootError("설치 루트를 확인할 수 없습니다. 경로와 권한을 확인하세요.");
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
      "검증된 빈 디렉터리를 새 설치 루트로 적용할까요? 기존 설치는 자동으로 이동하거나 삭제하지 않습니다.",
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
        setNotice(`설치 루트를 적용했습니다. revision ${result.registryRevision}`);
      }
      await refresh(true);
    } catch {
      if (mountedRef.current && requestId === rootRequestIdRef.current) {
        setInstallRootPreview(null);
        setInstallRootError("설치 루트를 적용할 수 없습니다. 최신 미리 보기를 다시 확인하세요.");
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
    if (tool.installState !== "absent" || operationBusyRef.current || readBusyRef.current) return;
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
    if (tool.launchState !== "available" || operationBusyRef.current || readBusyRef.current) return;
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
  const devSetupConfigurationExpired = devSetupConfigurationReview != null
    && devSetupConfigurationClockMs >= devSetupConfigurationReview.expiresAtMs;
  const devSetupConfigurationHasUnknown = devSetupConfigurationReview?.packages.some((packageReview) => (
    packageReview.currentState === "unknown" || packageReview.action === "verify"
  )) ?? false;
  const devSetupConfigurationApplyDisabled = (
    !devSetupConfigurationReview
    || devSetupConfigurationConsumed
    || devSetupConfigurationExpired
    || devSetupConfigurationBusy
    || devSetupApplyInFlight
    || busy !== null
    || operationBusyRef.current
    || readBusyRef.current
    || devSetupConfigurationHasUnknown
    || !devSetupConfigurationReview.canApply
    || !devSetupConfigurationReview.hasChanges
    || !devSetupReviewAcknowledged
    || !devSetupAgreementsAccepted
    || !devSetupAdminRiskAcknowledged
  );

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
          className={`btn ${tab === "local-quality" ? "active" : ""}`}
          type="button"
          aria-current={tab === "local-quality" ? "page" : undefined}
          disabled={batchBusy || busy !== null || installRootBusy || readBusy}
          onClick={() => setTab("local-quality")}
        >
          로컬 품질
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
        <button
          className={`btn ${tab === "dev-setup" ? "active" : ""}`}
          type="button"
          aria-current={tab === "dev-setup" ? "page" : undefined}
          disabled={batchBusy || busy !== null || installRootBusy || readBusy}
          onClick={() => {
            setTab("dev-setup");
            if (!devSetupSnapshot) void refreshDevSetup();
          }}
        >
          Dev Setup
        </button>
        <span className="latest">최신 버전: {manifest ? manifest.releaseTag : "..."}</span>
        <span className="spacer" />
        <button
          className="btn refresh"
          disabled={batchBusy || busy !== null || installRootBusy || readBusy}
          onClick={() => void refresh()}
        >
          새로고침
        </button>
      </header>

      {error && <div className="error" role="alert">{error}</div>}
      {notice && <div className="notice" role="status" aria-live="polite">{notice}</div>}

      {tab === "local-quality" ? (
        <section
          className="local-quality"
          aria-busy={localQualityBusy}
          aria-labelledby="local-quality-heading"
        >
          <div className="local-quality-head">
            <div>
              <h2 id="local-quality-heading">로컬 품질</h2>
              <p className="dim">
                검증된 설치 registry와 integration summary의 상태만 현재 메모리에 표시합니다.
              </p>
            </div>
            <button
              className="btn"
              type="button"
              disabled={batchBusy || busy !== null || installRootBusy || readBusy}
              onClick={() => void onInspectLocalQuality()}
            >
              {localQualityBusy ? "확인 중..." : "상태 새로고침"}
            </button>
          </div>
          <div className="diagnostic-safety-note">
            명시적 새로고침 · 읽기 전용 · 로컬 메모리 전용 · 원격 전송 없음 · 경로/원문 오류/환경변수 비공개
            <br />정상은 진단 원본의 정합성을 뜻하며 모든 앱이 설치되었다는 의미는 아닙니다.
          </div>
          {localQualityError && (
            <div className="error local-quality-error" role="alert">{localQualityError}</div>
          )}
          {!localQualitySnapshot && !localQualityBusy && (
            <div className="dim local-quality-empty" role="status" aria-live="polite">
              상태 새로고침을 눌러 현재 설치와 integration snapshot 상태를 확인하세요.
            </div>
          )}
          {localQualitySnapshot && (
            <>
              <div className="local-quality-meta" role="status" aria-live="polite">
                <span className={`quality-status ${localQualitySnapshot.status}`}>
                  {localQualitySnapshot.status === "healthy" ? "정상" : "확인 필요"}
                </span>
                <span className="dim">
                  schema v{localQualitySnapshot.schemaVersion} · 로컬 전용 · {new Date(localQualitySnapshot.observedAtMs).toLocaleString("ko-KR")}
                </span>
              </div>

              <section className="quality-card" aria-labelledby="installation-quality-heading">
                <div className="quality-card-head">
                  <div>
                    <h3 id="installation-quality-heading">설치 catalog / registry</h3>
                    <p className="dim">실행 파일과 설치 root 경로를 노출하지 않는 검증 결과입니다.</p>
                  </div>
                  <span className={`quality-source ${localQualitySnapshot.installation.registryState}`}>
                    {localQualitySnapshot.installation.registryState === "ready" ? "registry 정상" : "registry 확인 불가"}
                  </span>
                </div>
                <dl className="quality-summary">
                  <div>
                    <dt>catalog</dt>
                    <dd>{localQualitySnapshot.installation.catalogState === "ready"
                      ? `revision ${localQualitySnapshot.installation.catalogRevision}`
                      : "확인 불가"}</dd>
                  </div>
                  <div>
                    <dt>registry</dt>
                    <dd>{localQualitySnapshot.installation.registryState === "ready"
                      ? `revision ${localQualitySnapshot.installation.registryRevision}`
                      : "확인 불가"}</dd>
                  </div>
                  <div>
                    <dt>관리 대상</dt>
                    <dd>{localQualitySnapshot.installation.managedAppCount}개</dd>
                  </div>
                  <div>
                    <dt>설치 기록</dt>
                    <dd>{localQualitySnapshot.installation.installedAppCount == null
                      ? "확인 불가"
                      : `${localQualitySnapshot.installation.installedAppCount}개`}</dd>
                  </div>
                </dl>
                {localQualitySnapshot.installation.apps.length > 0 ? (
                  <div className="quality-table-wrap">
                    <table className="quality-table">
                      <caption className="visually-hidden">Manager 설치 상태</caption>
                      <thead>
                        <tr>
                          <th scope="col">앱</th>
                          <th scope="col">상태</th>
                          <th scope="col">버전 / 방식</th>
                        </tr>
                      </thead>
                      <tbody>
                        {localQualitySnapshot.installation.apps.map((appHealth) => (
                          <tr key={appHealth.appId}>
                            <td>{apps.find((app) => app.id === appHealth.appId)?.displayName ?? appHealth.appId}</td>
                            <td>{appHealth.state === "installed"
                              ? "설치됨"
                              : appHealth.state === "not-installed" ? "설치 기록 없음" : "확인 불가"}</td>
                            <td>{appHealth.version
                              ? `${appHealth.version} · ${appHealth.mode === "portable" ? "휴대용" : "설치 패키지"}`
                              : "—"}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                ) : (
                  <div className="dim local-quality-empty">표시할 검증된 catalog 앱이 없습니다.</div>
                )}
                {localQualitySnapshot.installation.truncated && (
                  <p className="quality-warning">설치 항목 상한에 도달해 일부 항목만 표시합니다.</p>
                )}
              </section>

              <section className="quality-card" aria-labelledby="integration-quality-heading">
                <div className="quality-card-head">
                  <div>
                    <h3 id="integration-quality-heading">Integration snapshot</h3>
                    <p className="dim">payload 내용은 표시하지 않고 검증된 summary의 producer·schema·freshness·개수만 표시합니다.</p>
                  </div>
                  <span className={`quality-source ${localQualitySnapshot.integration.rootState}`}>
                    {localQualitySnapshot.integration.rootState === "ready" ? "root 정상" : "root 확인 불가"}
                  </span>
                </div>
                <dl className="quality-summary">
                  <div><dt>검증된 snapshot</dt><dd>{localQualitySnapshot.integration.snapshotCount}개</dd></div>
                  <div><dt>격리된 문제</dt><dd>{localQualitySnapshot.integration.issueCount}개</dd></div>
                </dl>
                {localQualitySnapshot.integration.rootIssue && (
                  <p className="quality-warning">
                    Integration root: {localQualityIssueLabel(localQualitySnapshot.integration.rootIssue)}
                  </p>
                )}
                <div className="snapshot-health-list">
                  {localQualitySnapshot.integration.snapshots.map((snapshot) => (
                    <article key={`${snapshot.producer}:v${snapshot.schemaVersion}`} className="snapshot-health-card">
                      <div className="snapshot-health-title">
                        <h4>{snapshot.producer}</h4>
                        <span className="dim">schema v{snapshot.schemaVersion} · app {snapshot.producerVersion}</span>
                      </div>
                      <span className="snapshot-freshness">{formatFreshness(snapshot.freshnessMs)} 경과</span>
                      {snapshot.views.length > 0 ? (
                        <ul className="snapshot-view-list">
                          {snapshot.views.map((view) => (
                            <li key={view.kind}>
                              <code>{view.kind}</code>
                              <span>v{view.schemaVersion} · {view.entryCount}개 · {formatFreshness(view.freshnessMs)} 경과</span>
                            </li>
                          ))}
                        </ul>
                      ) : (
                        <span className="dim">legacy summary · 별도 view 없음</span>
                      )}
                      {snapshot.viewsTruncated && (
                        <span className="quality-warning">view 상한에 도달해 일부만 표시합니다.</span>
                      )}
                    </article>
                  ))}
                </div>
                {localQualitySnapshot.integration.snapshots.length === 0 && (
                  <div className="dim local-quality-empty">
                    발견된 summary가 없습니다. producer 앱의 설치·실행 여부에 따라 정상일 수 있습니다.
                  </div>
                )}
                {localQualitySnapshot.integration.issues.length > 0 && (
                  <ul className="quality-issue-list" aria-label="격리된 integration snapshot 문제">
                    {localQualitySnapshot.integration.issues.map((issue, index) => (
                      <li key={`${issue.producer}:${issue.schemaVersion ?? "root"}:${index}`}>
                        <code>{issue.producer}{issue.schemaVersion == null ? "" : `/v${issue.schemaVersion}`}</code>
                        <span>{localQualityIssueLabel(issue.kind)}</span>
                      </li>
                    ))}
                  </ul>
                )}
                {(localQualitySnapshot.integration.snapshotsTruncated
                  || localQualitySnapshot.integration.issuesTruncated) && (
                  <p className="quality-warning">Integration 검사 표시 상한에 도달해 일부 결과만 표시합니다.</p>
                )}
              </section>
            </>
          )}
        </section>
      ) : tab === "doctor" ? (
        <div className="doctor">
          <div className="doctor-head">
            <span className="dim">읽기 전용 진단 · 자동 설치·수정 없음</span>
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
          <div className="dim doctor-note">지원 번들·경로·환경변수는 비식별화되어야 합니다 (§15.4 경계).</div>

          <section className="diagnostic-tool" aria-labelledby="data-inspector-heading">
            <div className="diagnostic-tool-head">
              <div>
                <h2 id="data-inspector-heading">데이터 검사기</h2>
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
              읽기 전용 열기 · PRAGMA query_only=ON · SQLite authorizer · 2초 · 최대 1,000행 / 1 MiB ·
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
                          table {database.tables.length} · view {database.views.length} · 무결성 {dataIntegrityLabel(database.integrity)}
                        </span>
                      )}
                      {database.warning && <span className="database-card-warning">{database.warning}</span>}
                    </button>
                  ))}
                </div>
                {selectedDataDatabase && selectedDataDatabase.state === "available" && (
                  <div className="database-schema" aria-label="SQLite schema 요약">
                    <strong>{selectedDataDatabase.displayName} 스키마</strong>
                    <div className="schema-items">
                      {selectedDataDatabase.tables.map((table) => (
                        <span key={`table:${table.name}`} className="schema-item">
                          table {table.name} ({table.rowCount == null ? "?" : table.rowCount}행)
                        </span>
                      ))}
                      {selectedDataDatabase.views.map((view) => (
                        <span key={`view:${view.name}`} className="schema-item">view {view.name}</span>
                      ))}
                      {selectedDataDatabase.tables.length === 0 && selectedDataDatabase.views.length === 0 && (
                        <span className="dim">표시할 table/view가 없습니다.</span>
                      )}
                    </div>
                    <div className="dim schema-note">스키마 버전 {selectedDataDatabase.schemaVersion ?? "?"} · database path는 표시하지 않습니다.</div>
                  </div>
                )}
                <div className="query-panel">
                  <label htmlFor="data-query">읽기 전용 SQL 미리 보기</label>
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
                        <strong>조회 결과 미리 보기</strong>
                        <span className="dim">{dataResult.rowCount}행 · {formatBytes(dataResult.resultBytes)} · {dataResult.elapsedMs}밀리초{dataResult.truncated ? " · 일부 결과만 표시" : ""}</span>
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
                        <span className="dim">내보내기 전 미리 보기를 검토하세요. credential·경로·raw body는 backend에서 마스킹됩니다.</span>
                        <button className="btn" type="button" disabled={dataBusy || supportBusy} onClick={() => void onExportData("json")}>JSON 내보내기</button>
                        <button className="btn" type="button" disabled={dataBusy || supportBusy} onClick={() => void onExportData("csv")}>CSV 내보내기</button>
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
                <h2 id="support-bundle-heading">비식별화된 지원 번들</h2>
                <p className="dim">app/catalog/schema/log 메타데이터와 진단 상태만 포함하며 raw DB·raw log·경로·user·secret·Authorization/Cookie는 포함하지 않습니다.</p>
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
                  <strong>내보내기 미리 보기 · 비식별화 {supportPreview.redactionVersion}</strong>
                  <span className="dim">{formatBytes(supportPreview.estimatedBytes)} · DB {supportPreview.databaseCount}개 · 5분 이내 1회 내보내기</span>
                </div>
                <div className="support-sections">
                  <div><strong>포함</strong>{supportPreview.includedSections.map((section) => <span key={section}>{section}</span>)}</div>
                  <div><strong>제외</strong>{supportPreview.omittedSections.map((section) => <span key={section}>{section}</span>)}</div>
                </div>
                <div className="query-export-actions">
                  <span className="dim">진단 상태가 바뀌면 stale로 중단되며 새 미리 보기가 필요합니다.</span>
                  <button className="btn primary" type="button" disabled={supportBusy} onClick={() => void onExportSupport()}>확인 후 JSON 내보내기</button>
                  <button className="btn" type="button" disabled={supportBusy} onClick={() => setSupportPreview(null)}>취소</button>
                </div>
              </div>
            )}
            {!supportPreview && <div className="dim diagnostic-empty">번들 미리 확인 후 포함/제외 범위를 검토할 수 있습니다.</div>}
          </section>
        </div>
      ) : tab === "dev-setup" ? (
        <section
          className="dev-setup related-tools"
          aria-busy={devSetupBusy || readBusy || devSetupConfigurationBusy || devSetupApplyInFlight}
          aria-labelledby="dev-setup-heading"
        >
          <div className="related-tools-head">
            <div>
              <h2 id="dev-setup-heading">Dev Setup</h2>
              <p className="dim">
                위 capability 감사는 계속 읽기 전용이며 설치·실행·registry·PATH 변경은 수행하지 않습니다.
                아래 package-only 경로는 별도의 명시적 검토·확인 뒤에만 WinGet 패키지 적용을 수행합니다.
              </p>
            </div>
            <button
              className="btn"
              type="button"
              disabled={batchBusy || busy !== null || installRootBusy || readBusy}
              onClick={() => void refreshDevSetup()}
            >
              {devSetupBusy ? "감사 중..." : "다시 감사"}
            </button>
          </div>
          <div className="diagnostic-safety-note">
            읽기 전용 감사 · 고정된 실행 파일과 WSL 목록만 조회 · 원본 경로/환경변수/프로세스 출력 비공개 · 아래 패키지 적용과 분리됨
          </div>
          {devSetupError && <div className="error related-tools-error" role="alert">{devSetupError}</div>}
          {!devSetupSnapshot && !devSetupBusy && !devSetupError && (
            <div className="dim related-tools-empty" role="status" aria-live="polite">
              개발 환경 감사를 실행해 주세요.
            </div>
          )}
          {devSetupSnapshot && (
            <>
              <div className="dev-setup-meta dim">
                schema v{devSetupSnapshot.schemaVersion} · {devSetupSnapshot.mode} · 이번 감사 {new Date(devSetupSnapshot.observedAtMs).toLocaleTimeString("ko-KR")}
              </div>
              <div className="dev-setup-grid">
                {devSetupSnapshot.capabilities.map((capability) => {
                  const plan = devSetupSnapshot.plan.find(
                    (candidate) => candidate.capabilityId === capability.id,
                  );
                  return (
                    <article
                      key={capability.id}
                      className={`dev-setup-card ${plan?.status ?? "unknown"}`}
                    >
                      <div className="related-tool-card-head">
                        <h3>{DEV_SETUP_CAPABILITY_LABELS[capability.id]}</h3>
                        <span className={`related-tool-state ${plan?.status === "satisfied" ? "ok" : "warning"}`}>
                          {devSetupStateLabel(capability)}
                        </span>
                      </div>
                      <div className="dim dev-setup-scope">
                        범위: {capability.scope === "wsl" ? "WSL" : "Windows"}
                      </div>
                      <ul className="dev-setup-evidence">
                        {capability.evidence.map((evidence) => (
                          <li key={`${evidence.source}:${evidence.result}`}>
                            {evidenceLabel(evidence.source, evidence.result)}
                          </li>
                        ))}
                      </ul>
                      {plan && (
                        <div className={`dev-setup-plan ${plan.status}`}>
                          {DEV_SETUP_ACTION_LABELS[plan.action]}
                        </div>
                      )}
                    </article>
                  );
                })}
              </div>
              <section
                className="dev-setup-config"
                aria-labelledby="dev-setup-config-heading"
                aria-busy={devSetupConfigurationBusy || devSetupApplyInFlight}
              >
                <div className="dev-setup-config-head">
                  <div>
                <h3 id="dev-setup-config-heading">WinGet 구성 v3 · package-only</h3>
                    <p className="dim">
                      외부 YAML은 그대로 실행하지 않습니다. Microsoft.WinGet/Package 리소스와 고정된
                      <code>winget</code> source 이름만 정규화해 검토합니다.
                    </p>
                  </div>
                  <div className="dev-setup-config-actions">
                    {devSetupApplyInFlight && (
                      <button
                        className="btn"
                        type="button"
                        onClick={onCancelDevSetupApply}
                        aria-label="Dev Setup 패키지 적용 취소"
                      >
                        적용 취소
                      </button>
                    )}
                    <button
                      className="btn"
                      type="button"
                      disabled={
                        batchBusy
                        || busy !== null
                        || installRootBusy
                        || readBusy
                        || devSetupConfigurationBusy
                        || devSetupApplyInFlight
                      }
                      onClick={() => void onImportDevSetupConfiguration()}
                    >
                      {devSetupConfigurationBusy && !devSetupApplyInFlight
                        ? "가져오는 중..."
                        : devSetupConfigurationReview ? "다시 가져오기" : "구성 가져오기"}
                    </button>
                    {devSetupConfigurationReview && (
                      <>
                        <button
                          className="btn"
                          type="button"
                          disabled={
                            devSetupConfigurationBusy
                            || devSetupApplyInFlight
                            || devSetupConfigurationConsumed
                            || devSetupConfigurationExpired
                            || busy !== null
                            || readBusy
                          }
                          onClick={() => void onExportDevSetupConfiguration()}
                        >
                          정규화된 구성 내보내기
                        </button>
                        <button
                          className="btn"
                          type="button"
                          disabled={devSetupConfigurationBusy || devSetupApplyInFlight}
                          onClick={() => void onDiscardDevSetupConfiguration()}
                        >
                          검토 버리기
                        </button>
                      </>
                    )}
                  </div>
                </div>
                <div className="dev-setup-config-safety" role="note">
                  <strong>안전 경계</strong>
                  <ul>
                    <li>네트워크 연결이 필요합니다.</li>
                    <li>설치 프로그램은 UAC/관리자 권한과 재부팅을 요구할 수 있지만 앱이 자동 재부팅을 예약하거나 실행하지 않습니다.</li>
                    <li>패키지 설치 프로그램이 자체 PATH·registry·파일을 변경할 수 있습니다.</li>
                    <li>상태를 알 수 없는 패키지는 적용을 차단하며 설치를 제안하지 않습니다.</li>
                  </ul>
                </div>
                {devSetupConfigurationError && (
                  <div className="error dev-setup-config-error" role="alert">
                    {devSetupConfigurationError}
                  </div>
                )}
                {devSetupConfigurationNotice && (
                  <div className="notice dev-setup-config-notice" role="status" aria-live="polite">
                    {devSetupConfigurationNotice}
                  </div>
                )}
                {!devSetupConfigurationReview && !devSetupConfigurationBusy && !devSetupConfigurationError && (
                  <div className="dim dev-setup-config-empty" role="status" aria-live="polite">
                    WinGet Configuration 파일을 가져오면 정규화된 package-only 검토가 여기에 표시됩니다.
                  </div>
                )}
                {devSetupConfigurationReview && (
                  <>
                    <div
                      className={`dev-setup-config-review ${devSetupConfigurationExpired ? "expired" : ""} ${devSetupConfigurationConsumed ? "consumed" : ""}`}
                      role="region"
                      aria-label="WinGet Configuration package-only 검토"
                      aria-live="polite"
                    >
                      <div className="dev-setup-config-review-head">
                        <strong>정규화된 검토</strong>
                        <span className="dim">
                          {devSetupConfigurationExpired
                            ? "만료됨"
                            : devSetupConfigurationConsumed ? "적용 토큰 사용됨" : "적용 전 검토 가능"}
                        </span>
                      </div>
                      <dl className="dev-setup-config-facts">
                        <div><dt>schema</dt><dd><code>{devSetupConfigurationReview.schemaVersion}</code></dd></div>
                        <div>
                          <dt>외부 신뢰</dt>
                          <dd>
                            <span>{devSetupConfigurationReview.sourceTrust === "external-restricted" ? "외부 입력 · 제한 처리(신뢰 안 함)" : "제한된 외부 입력"}</span>{" "}
                            <code>{devSetupConfigurationReview.sourceTrust}</code>
                          </dd>
                        </div>
                        <div><dt>digest prefix</dt><dd><code>sha256:{devSetupConfigurationReview.configurationDigest.slice(0, 12)}…</code></dd></div>
                        <div><dt>적용 수명</dt><dd>5분 · 1회 적용</dd></div>
                      </dl>
                      <div className="dev-setup-config-scope dim">
                        mode: <code>{devSetupConfigurationReview.mode}</code> · 패키지 {devSetupConfigurationReview.packages.length}개
                      </div>
                      {devSetupConfigurationHasUnknown && (
                        <div className="dev-setup-config-block" role="alert">
                          확인할 수 없는 패키지 상태가 있어 적용을 차단합니다. 이 상태에서 설치를 제안하지 않습니다.
                        </div>
                      )}
                      {!devSetupConfigurationReview.hasChanges && (
                        <div className="dev-setup-config-block" role="status">
                          적용할 패키지 변경이 없습니다.
                        </div>
                      )}
                      {devSetupConfigurationReview.hasChanges
                        && !devSetupConfigurationReview.canApply
                        && !devSetupConfigurationHasUnknown && (
                        <div className="dev-setup-config-block" role="alert">
                          현재 검토는 적용할 수 없습니다. 패키지 상태를 다시 확인하세요.
                        </div>
                      )}
                      <div className="dev-setup-config-table-wrap">
                        <table className="dev-setup-config-table">
                          <thead>
                            <tr>
                              <th>패키지 ID</th>
                              <th>원하는 상태</th>
                              <th>관찰된 상태</th>
                              <th>조치</th>
                            </tr>
                          </thead>
                          <tbody>
                            {devSetupConfigurationReview.packages.map((packageReview) => (
                              <tr
                                key={packageReview.packageId}
                                className={packageReview.currentState === "unknown" || packageReview.action === "verify" ? "unknown" : undefined}
                              >
                                <td>
                                  <code>{packageReview.packageId}</code>
                                  <div className="dev-setup-config-package-flags">
                                    <span className={packageReview.requestedAgreementAcceptance ? "requested" : "dim"}>
                                      {packageReview.requestedAgreementAcceptance ? "외부 약관 수락 요청" : "외부 약관 수락 없음"}
                                    </span>
                                    <span className={packageReview.declaredElevation ? "requested" : "dim"}>
                                      {packageReview.declaredElevation ? "외부 관리자 선언" : "외부 권한 선언 없음"}
                                    </span>
                                  </div>
                                </td>
                                <td>{devSetupConfigurationDesiredLabel(packageReview)}</td>
                                <td>{devSetupConfigurationStateLabel(packageReview.currentState)}</td>
                                <td>{devSetupConfigurationActionLabel(packageReview.action)}</td>
                              </tr>
                            ))}
                          </tbody>
                        </table>
                      </div>
                      <div className="dev-setup-config-decision-note">
                        외부 파일의 선언을 실행 설정에 복사하지 않습니다. 약관 수락은 별도 확인하고,
                        외부 권한 선언과 무관하게 관리자/UAC·재부팅 가능성을 다시 고지합니다.
                      </div>
                      <div className="dev-setup-config-checks" aria-label="Dev Setup 적용 확인">
                        <label>
                          <input
                            type="checkbox"
                            checked={devSetupReviewAcknowledged}
                            disabled={devSetupConfigurationBusy || devSetupConfigurationConsumed || devSetupConfigurationExpired || devSetupApplyInFlight}
                            onChange={(event) => setDevSetupReviewAcknowledged(event.target.checked)}
                          />
                          정규화된 package-only 검토를 확인했습니다
                        </label>
                        <label>
                          <input
                            type="checkbox"
                            checked={devSetupAgreementsAccepted}
                            disabled={devSetupConfigurationBusy || devSetupConfigurationConsumed || devSetupConfigurationExpired || devSetupApplyInFlight}
                            onChange={(event) => setDevSetupAgreementsAccepted(event.target.checked)}
                          />
                          로컬에 등록된 고정 이름 winget source·패키지 약관 수락을 확인했습니다
                        </label>
                        <label>
                          <input
                            type="checkbox"
                            checked={devSetupAdminRiskAcknowledged}
                            disabled={devSetupConfigurationBusy || devSetupConfigurationConsumed || devSetupConfigurationExpired || devSetupApplyInFlight}
                            onChange={(event) => setDevSetupAdminRiskAcknowledged(event.target.checked)}
                          />
                          관리자/UAC·재부팅 위험을 확인했습니다
                        </label>
                      </div>
                      <div className="dev-setup-config-apply-actions">
                        <button
                          className="btn primary"
                          type="button"
                          disabled={devSetupConfigurationApplyDisabled}
                          aria-busy={devSetupApplyInFlight}
                          onClick={() => void onApplyDevSetupConfiguration()}
                        >
                          {devSetupApplyInFlight ? "적용 중..." : "확인 후 패키지 적용"}
                        </button>
                        <span className="dim">
                          {!devSetupConfigurationReview.canApply || devSetupConfigurationHasUnknown
                            ? "확인할 수 없는 상태는 적용할 수 없습니다."
                            : "세 가지 확인을 모두 선택하면 마지막 확인 창이 열립니다."}
                        </span>
                      </div>
                    </div>
                    {devSetupConfigurationResult && (
                      <div className="dev-setup-config-result" role="status" aria-live="polite">
                        <div className="dev-setup-config-review-head">
                          <strong>패키지 적용 결과</strong>
                          <span>{devSetupApplyStatusLabel(devSetupConfigurationResult.status)}</span>
                        </div>
                        <div className="dev-setup-config-result-list">
                          {devSetupConfigurationResult.results.map((result) => (
                            <div
                              key={result.packageId}
                              className={`dev-setup-config-result-row ${result.status === "failed" || result.status === "timed-out" ? "bad" : result.status === "applied" ? "ok" : ""}`}
                            >
                              <code>{result.packageId}</code>
                              <span>{devSetupApplyStatusLabel(result.status)}</span>
                            </div>
                          ))}
                        </div>
                      </div>
                    )}
                  </>
                )}
              </section>
              <p className="dim related-tools-note">
                이 capability 계획은 읽기 전용 확인 순서입니다. 아래 package-only apply는 별도 검토·약관·권한 위험 확인 경계를 사용합니다.
              </p>
            </>
          )}
        </section>
      ) : tab === "related-tools" ? (
        <section
          className="related-tools"
          aria-busy={relatedBusy || readBusy}
          aria-labelledby="related-tools-heading"
        >
          <div className="related-tools-head">
            <div>
              <h2 id="related-tools-heading">관련 도구</h2>
              <p className="dim">
                개발 흐름을 보완하는 작은 공식 도구 목록입니다. 로컬 실행 가능성과 설치 근거를 구분하며 경로와 버전은 표시하지 않습니다.
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
                <article key={tool.id} className={`related-tool-card ${tool.installState === "present" ? "installed" : ""}`}>
                  <div className="related-tool-card-head">
                    <div>
                      <h3>{tool.displayName}</h3>
                      <p className="dim">{tool.summary}</p>
                    </div>
                    <span className={`related-tool-state ${tool.installState === "present" ? "ok" : tool.installState === "unknown" ? "warning" : "dim"}`}>
                      {installCapabilityLabel(tool.installState)}
                    </span>
                  </div>
                  <dl className="related-tool-facts">
                    <div><dt>감지</dt><dd>{relatedDetectionDescription(tool)}</dd></div>
                    <div><dt>Manager 실행</dt><dd>{availabilityCapabilityLabel(tool.launchState)}</dd></div>
                    {tool.dockerCapability && (
                      <>
                        <div><dt>Windows CLI</dt><dd>{availabilityCapabilityLabel(tool.dockerCapability.windowsCli)}</dd></div>
                        <div><dt>WSL backend</dt><dd>{backendCapabilityLabel(tool.dockerCapability.wslBackend)}</dd></div>
                      </>
                    )}
                    <div><dt>WinGet ID</dt><dd><code>{tool.wingetId}</code></dd></div>
                    <div><dt>라이선스</dt><dd>{tool.license}</dd></div>
                  </dl>
                  {tool.dockerCapability && (
                    <div className="related-tool-evidence" aria-label="Docker capability 근거">
                      {tool.dockerCapability.evidence.map((evidence) => (
                        <span key={`${evidence.source}:${evidence.result}`}>
                          {evidenceLabel(evidence.source, evidence.result)}
                        </span>
                      ))}
                      <span>이번 감지 {new Date(tool.dockerCapability.observedAtMs).toLocaleTimeString("ko-KR")}</span>
                    </div>
                  )}
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
                    {tool.launchState === "available" ? (
                      <button
                        className="btn"
                        type="button"
                        aria-busy={busy === `related:${tool.id}:launch`}
                        disabled={batchBusy || busy !== null || installRootBusy || readBusy}
                        onClick={() => void onRelatedLaunch(tool)}
                      >
                        {busy === `related:${tool.id}:launch` ? "실행 중..." : "실행"}
                      </button>
                    ) : tool.installState === "absent" ? (
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
                    ) : (
                      <button
                        className="btn"
                        type="button"
                        disabled
                        title="설치 여부를 확정할 근거가 없어 자동 설치를 제안하지 않습니다. Dev Setup에서 근거를 확인하세요."
                      >
                        {tool.installState === "present"
                          ? "실행 경로 확인 필요"
                          : "설치 상태 확인 필요"}
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
              <h2 id="install-root-heading">설치 루트 지정</h2>
              <p className="dim">
                기존 설치는 이동하지 않으며, 비어 있고 검증된 디렉터리만 다음 설치에 적용합니다.
              </p>
            </div>
            <span className="read-only-tag">미리 보기 후 확인</span>
          </div>
          <div className="install-root-form">
            <label htmlFor="install-root-path">설치 루트 경로</label>
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
                  확인 후 이 루트 적용
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
              <dt>실행 파일</dt>
              <dd><code>{installPathDetails.executable ?? "Manager가 실제 설치 위치를 추적하지 않습니다."}</code></dd>
              <dt>설치 루트</dt>
              <dd><code>{installPathDetails.installRoot ?? "Manager가 실제 설치 위치를 추적하지 않습니다."}</code></dd>
              <dt>원본 매니페스트</dt>
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
              <th>앱</th>
              <th>설치됨</th>
              <th>최신 버전</th>
              <th>작업</th>
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
                  onContextMenu={appContextMenu.triggerProps.onContextMenu}
                  onKeyDown={(event) => {
                    appContextMenu.triggerProps.onKeyDown?.(event);
                    if (
                      event.defaultPrevented
                      || event.target !== event.currentTarget
                      || !isKeyboardActivation(event)
                    ) return;
                    event.preventDefault();
                    setSelectedAppId(a.id);
                  }}
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
                          <span className="dim"> ← 이전 버전 {cur?.previousVersion}</span>
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
                        {busy === `${a.id}:path` ? "..." : "경로 정보"}
                      </button>
                    )}
                    {inst?.mode === "portable" && (
                      <button className="btn" disabled={isAppBusy(a.id)} onClick={() => void onLaunch(a.id)}>
                        실행
                      </button>
                    )}
                    {canRollback && (
                      <button className="btn" disabled={isAppBusy(a.id)} onClick={() => void onRollback(a.id)}>
                        {busy === `${a.id}:rollback` ? "..." : "롤백"}
                      </button>
                    )}
                    {!upToDate && app && (
                      <>
                        <button className="btn" disabled={isAppBusy(a.id)} onClick={() => void onInstall(a.id, "portable")}>
                          {busy === `${a.id}:portable` ? "..." : inst ? "업데이트 (portable)" : "설치 (portable)"}
                        </button>
                        <button className="btn" disabled={isAppBusy(a.id)} onClick={() => void onInstall(a.id, "installer")}>
                          {busy === `${a.id}:installer` ? "..." : inst ? "업데이트 (setup)" : "설치 (setup)"}
                        </button>
                      </>
                    )}
                    {upToDate && <span className="dim tag">최신 상태</span>}
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
