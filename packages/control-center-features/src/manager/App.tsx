import { useCallback, useEffect, useRef, useState } from "react";
import {
  applyDevSetupConfiguration,
  cancelDevSetupApply,
  cancelSupportBundle,
  devSetupAudit,
  discardDevSetupConfiguration,
  exportDevSetupConfiguration,
  exportSupportBundle,
  importDevSetupConfiguration,
  installRelatedTool,
  launchRelatedTool,
  openRelatedToolUrl,
  previewSupportBundle,
  relatedTools,
  runDiagnosis,
  type DiagnosisItem,
} from "./api";
import type { DevSetupAudit, DevSetupCapability, DevSetupPlanItem, RelatedTool, SupportBundlePreview } from "./types";
import "./App.css";
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
  const message = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  return RELATED_TOOL_SAFE_ERRORS.has(message)
    ? (RELATED_TOOL_ERROR_DISPLAY[message] ?? message)
    : RELATED_TOOL_GENERIC_ERROR;
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
    case "running":
      return "실행 중";
    case "stopped":
      return "중지됨";
    case "present":
      return "등록됨 · 실행 상태 확인 불가";
    case "absent":
      return "등록되지 않음";
    default:
      return "확인 필요";
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

type DevSetupConfigurationReview = NonNullable<Awaited<ReturnType<typeof importDevSetupConfiguration>>>;
type DevSetupConfigurationPackage = DevSetupConfigurationReview["packages"][number];
type DevSetupConfigurationApplyResult = Awaited<ReturnType<typeof applyDevSetupConfiguration>>;

const DEV_SETUP_CONFIGURATION_IMPORT_ERROR =
  "WinGet Configuration v3 파일을 불러올 수 없습니다. Microsoft.WinGet/Package와 고정된 winget source 이름만 지원합니다.";
const DEV_SETUP_CONFIGURATION_EXPORT_ERROR = "정규화된 WinGet Configuration을 내보낼 수 없습니다.";
const DEV_SETUP_CONFIGURATION_APPLY_ERROR = "Dev Setup 구성을 적용할 수 없습니다. 만료되었거나 최신 검토가 필요합니다.";
const DEV_SETUP_CONFIGURATION_CANCEL_ERROR = "Dev Setup 적용 취소를 완료할 수 없습니다.";
const DEV_SETUP_CONFIGURATION_DISCARD_ERROR = "Dev Setup 구성 검토를 폐기할 수 없습니다.";
const DEV_SETUP_CONFIGURATION_EXPIRED = "Dev Setup 적용 미리 보기가 만료되었습니다. 구성을 다시 가져오세요.";
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
  const label = DEV_SETUP_CONFIGURATION_DESIRED_LABELS[packageReview.desired] ?? packageReview.desired;
  return packageReview.desired === "version" && packageReview.version ? `${label} ${packageReview.version}` : label;
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
    return backendCapabilityLabel(capability.state as NonNullable<RelatedTool["dockerCapability"]>["wslBackend"]);
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
      url.protocol !== "https:" ||
      url.username ||
      url.password ||
      url.port ||
      url.hostname.length === 0 ||
      !RELATED_TOOL_OFFICIAL_HOSTS.has(url.hostname)
    ) {
      return null;
    }
    return url.toString();
  } catch {
    return null;
  }
}

export type ToolsMode = "doctor" | "dev-setup" | "related-tools";
export default function App({ mode }: { mode?: ToolsMode } = {}) {
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [tab, setTab] = useState<ToolsMode>(mode ?? "doctor");
  const [diagnosis, setDiagnosis] = useState<DiagnosisItem[]>([]);
  const [supportPreview, setSupportPreview] = useState<SupportBundlePreview | null>(null);
  const [supportBusy, setSupportBusy] = useState(false);
  const [relatedToolList, setRelatedToolList] = useState<RelatedTool[]>([]);
  const [relatedBusy, setRelatedBusy] = useState(false);
  const [relatedError, setRelatedError] = useState<string | null>(null);
  const [devSetupSnapshot, setDevSetupSnapshot] = useState<DevSetupAudit | null>(null);
  const [devSetupBusy, setDevSetupBusy] = useState(false);
  const [devSetupError, setDevSetupError] = useState<string | null>(null);
  const [devSetupConfigurationReview, setDevSetupConfigurationReview] = useState<DevSetupConfigurationReview | null>(
    null,
  );
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
  const [readBusy, setReadBusy] = useState(false);
  const operationBusyRef = useRef(false);
  const readBusyRef = useRef(false);
  const mountedRef = useRef(true);
  const refreshRequestIdRef = useRef(0);
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

  const onPreviewSupport = useCallback(async () => {
    if (operationBusyRef.current || readBusyRef.current || supportBusy) return;
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
  }, [supportBusy]);

  const onCancelSupport = useCallback(() => {
    const id = supportOperationIdRef.current;
    if (id) void cancelSupportBundle(id).catch(() => undefined);
  }, []);

  const onExportSupport = useCallback(async () => {
    const preview = supportPreview;
    if (!preview || operationBusyRef.current || readBusyRef.current || supportBusy) return;
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
  }, [supportBusy, supportPreview]);

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
      operationBusyRef.current ||
      readBusyRef.current ||
      devSetupConfigurationBusy ||
      devSetupConfigurationApplyBusyRef.current
    )
      return;
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
      !review ||
      devSetupConfigurationConsumed ||
      devSetupConfigurationBusy ||
      operationBusyRef.current ||
      readBusyRef.current
    )
      return;
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
      !review ||
      devSetupConfigurationBusy ||
      devSetupConfigurationApplyBusyRef.current ||
      operationBusyRef.current ||
      readBusyRef.current
    )
      return;
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
    const hasUnknownPackage =
      review?.packages.some(
        (packageReview) => packageReview.currentState === "unknown" || packageReview.action === "verify",
      ) ?? false;
    if (
      !review ||
      devSetupConfigurationConsumed ||
      devSetupConfigurationBusy ||
      devSetupConfigurationApplyBusyRef.current ||
      operationBusyRef.current ||
      readBusyRef.current
    )
      return;
    if (Date.now() >= review.expiresAtMs) {
      setDevSetupConfigurationError(DEV_SETUP_CONFIGURATION_EXPIRED);
      return;
    }
    if (hasUnknownPackage) {
      setDevSetupConfigurationError("확인할 수 없는 패키지 상태가 있어 적용할 수 없습니다. 설치를 제안하지 않습니다.");
      return;
    }
    if (
      !review.canApply ||
      !review.hasChanges ||
      !devSetupReviewAcknowledged ||
      !devSetupAgreementsAccepted ||
      !devSetupAdminRiskAcknowledged
    )
      return;
    if (
      !window.confirm(
        "정규화된 package-only 구성의 패키지 변경을 적용할까요? 네트워크와 UAC/관리자 권한이 필요할 수 있으며 자동 재부팅은 실행하지 않습니다.",
      )
    )
      return;

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
      const result = await applyDevSetupConfiguration(review.previewId, true, true, true);
      if (mountedRef.current && requestId === devSetupConfigurationApplyRequestIdRef.current) {
        setDevSetupConfigurationResult(result);
        setDevSetupConfigurationNotice(
          result.status === "complete"
            ? "Dev Setup 패키지 적용이 완료되었습니다."
            : result.status === "cancelled"
              ? DEV_SETUP_CONFIGURATION_CANCELLED
              : "Dev Setup 패키지 적용이 일부 완료되었습니다. 결과를 확인하세요.",
        );
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

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      readBusyRef.current = false;
      refreshRequestIdRef.current++;
      supportRequestIdRef.current++;
      const pending = supportOperationIdRef.current;
      if (pending) void cancelSupportBundle(pending).catch(() => undefined);
      relatedRequestIdRef.current++;
      relatedActionIdRef.current++;
      devSetupRequestIdRef.current++;
      devSetupConfigurationRequestIdRef.current++;
      devSetupConfigurationApplyRequestIdRef.current++;
      if (devSetupConfigurationApplyBusyRef.current) void cancelDevSetupApply().catch(() => undefined);
    };
  }, []);

  const onRelatedInstall = async (tool: RelatedTool) => {
    if (tool.installState !== "absent" || operationBusyRef.current || readBusyRef.current) return;
    if (!window.confirm(`'${tool.displayName}'을 WinGet으로 설치할까요? WinGet이 공식 패키지 설치를 진행합니다.`))
      return;
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
        setNotice(
          result.message === "WinGet 설치가 완료되었습니다." ? result.message : "WinGet 설치가 완료되었습니다.",
        );
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
        setNotice(result.message === "관련 도구를 실행했습니다." ? result.message : "관련 도구를 실행했습니다.");
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
  const devSetupConfigurationExpired =
    devSetupConfigurationReview != null && devSetupConfigurationClockMs >= devSetupConfigurationReview.expiresAtMs;
  const embeddedLoadRef = useRef({
    doctor: onDiagnose,
    "related-tools": refreshRelatedTools,
    "dev-setup": refreshDevSetup,
  });
  embeddedLoadRef.current = {
    doctor: onDiagnose,
    "related-tools": refreshRelatedTools,
    "dev-setup": refreshDevSetup,
  };
  useEffect(() => {
    if (!mode) return;
    setTab(mode);
    void embeddedLoadRef.current[mode]();
  }, [mode]);

  const devSetupConfigurationHasUnknown =
    devSetupConfigurationReview?.packages.some(
      (packageReview) => packageReview.currentState === "unknown" || packageReview.action === "verify",
    ) ?? false;
  const devSetupConfigurationApplyDisabled =
    !devSetupConfigurationReview ||
    devSetupConfigurationConsumed ||
    devSetupConfigurationExpired ||
    devSetupConfigurationBusy ||
    devSetupApplyInFlight ||
    busy !== null ||
    operationBusyRef.current ||
    readBusyRef.current ||
    devSetupConfigurationHasUnknown ||
    !devSetupConfigurationReview.canApply ||
    !devSetupConfigurationReview.hasChanges ||
    !devSetupReviewAcknowledged ||
    !devSetupAgreementsAccepted ||
    !devSetupAdminRiskAcknowledged;

  return (
    <div className="app manager-tools">
      {!mode && (
        <header className="toolbar">
          <h1 className="title">Control Center 도구</h1>
          <button
            type="button"
            className="btn"
            aria-current={tab === "doctor" ? "page" : undefined}
            disabled={busy !== null || readBusy}
            onClick={() => {
              setTab("doctor");
              void onDiagnose();
            }}
          >
            환경 진단
          </button>
          <button
            type="button"
            className="btn"
            aria-current={tab === "related-tools" ? "page" : undefined}
            disabled={busy !== null || readBusy}
            onClick={() => {
              setTab("related-tools");
              void refreshRelatedTools();
            }}
          >
            관련 도구
          </button>
          <button
            type="button"
            className="btn"
            aria-current={tab === "dev-setup" ? "page" : undefined}
            disabled={busy !== null || readBusy}
            onClick={() => {
              setTab("dev-setup");
              void refreshDevSetup();
            }}
          >
            Dev Setup
          </button>
        </header>
      )}

      {error && (
        <div className="error" role="alert">
          {error}
        </div>
      )}
      {notice && (
        <div className="notice" role="status" aria-live="polite">
          {notice}
        </div>
      )}

      {tab === "doctor" ? (
        <div className="doctor">
          <div className="doctor-head">
            <span className="dim">읽기 전용 진단 · 자동 설치·수정 없음</span>
            <button className="btn" disabled={readBusy} onClick={() => void onDiagnose()}>
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

          <section className="diagnostic-tool support-tool" aria-labelledby="support-bundle-heading">
            <div className="diagnostic-tool-head">
              <div>
                <h2 id="support-bundle-heading">비식별화된 지원 번들</h2>
                <p className="dim">
                  제품 목록·진단·운영 로그 요약을 포함합니다. 원본 DB·로그·경로·비밀값은 포함하지 않습니다.
                </p>
              </div>
              <div className="diagnostic-tool-actions">
                {supportBusy && supportOperationIdRef.current && (
                  <button className="btn" type="button" onClick={onCancelSupport}>
                    취소
                  </button>
                )}
                <button className="btn" type="button" disabled={supportBusy} onClick={() => void onPreviewSupport()}>
                  {supportBusy ? "준비 중..." : "번들 미리 확인"}
                </button>
              </div>
            </div>
            {supportPreview && (
              <div className="support-preview" role="status" aria-live="polite">
                <div className="query-result-head">
                  <strong>내보내기 미리 보기 · 비식별화 {supportPreview.redactionVersion}</strong>
                  <span className="dim">{formatBytes(supportPreview.estimatedBytes)} · 5분 이내 1회 내보내기</span>
                </div>
                <div className="support-sections">
                  <div>
                    <strong>포함</strong>
                    {supportPreview.includedSections.map((section) => (
                      <span key={section}>{section}</span>
                    ))}
                  </div>
                  <div>
                    <strong>제외</strong>
                    {supportPreview.omittedSections.map((section) => (
                      <span key={section}>{section}</span>
                    ))}
                  </div>
                </div>
                <div className="query-export-actions">
                  <span className="dim">미리 확인한 내용을 그대로 내보냅니다. 만료되면 다시 미리 확인해 주세요.</span>
                  <button
                    className="btn primary"
                    type="button"
                    disabled={supportBusy}
                    onClick={() => void onExportSupport()}
                  >
                    확인 후 JSON 내보내기
                  </button>
                  <button className="btn" type="button" disabled={supportBusy} onClick={() => setSupportPreview(null)}>
                    취소
                  </button>
                </div>
              </div>
            )}
            {!supportPreview && (
              <div className="dim diagnostic-empty">번들 미리 확인 후 포함/제외 범위를 검토할 수 있습니다.</div>
            )}
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
                위 capability 감사는 계속 읽기 전용이며 설치·실행·registry·PATH 변경은 수행하지 않습니다. 아래
                package-only 경로는 별도의 명시적 검토·확인 뒤에만 WinGet 패키지 적용을 수행합니다.
              </p>
            </div>
            <button
              className="btn"
              type="button"
              disabled={busy !== null || readBusy}
              onClick={() => void refreshDevSetup()}
            >
              {devSetupBusy ? "감사 중..." : "다시 감사"}
            </button>
          </div>
          <div className="diagnostic-safety-note">
            읽기 전용 감사 · 고정된 실행 파일과 WSL 목록만 조회 · 원본 경로/환경변수/프로세스 출력 비공개 · 아래 패키지
            적용과 분리됨
          </div>
          {devSetupError && (
            <div className="error related-tools-error" role="alert">
              {devSetupError}
            </div>
          )}
          {!devSetupSnapshot && !devSetupBusy && !devSetupError && (
            <div className="dim related-tools-empty" role="status" aria-live="polite">
              개발 환경 감사를 실행해 주세요.
            </div>
          )}
          {devSetupSnapshot && (
            <>
              <div className="dev-setup-meta dim">
                schema v{devSetupSnapshot.schemaVersion} · {devSetupSnapshot.mode} · 이번 감사{" "}
                {new Date(devSetupSnapshot.observedAtMs).toLocaleTimeString("ko-KR")}
              </div>
              <div className="dev-setup-grid">
                {devSetupSnapshot.capabilities.map((capability) => {
                  const plan = devSetupSnapshot.plan.find((candidate) => candidate.capabilityId === capability.id);
                  return (
                    <article key={capability.id} className={`dev-setup-card ${plan?.status ?? "unknown"}`}>
                      <div className="related-tool-card-head">
                        <h3>{DEV_SETUP_CAPABILITY_LABELS[capability.id]}</h3>
                        <span className={`related-tool-state ${plan?.status === "satisfied" ? "ok" : "warning"}`}>
                          {devSetupStateLabel(capability)}
                        </span>
                      </div>
                      <div className="dim dev-setup-scope">범위: {capability.scope === "wsl" ? "WSL" : "Windows"}</div>
                      <ul className="dev-setup-evidence">
                        {capability.evidence.map((evidence) => (
                          <li key={`${evidence.source}:${evidence.result}`}>
                            {evidenceLabel(evidence.source, evidence.result)}
                          </li>
                        ))}
                      </ul>
                      {plan && (
                        <div className={`dev-setup-plan ${plan.status}`}>{DEV_SETUP_ACTION_LABELS[plan.action]}</div>
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
                      disabled={busy !== null || readBusy || devSetupConfigurationBusy || devSetupApplyInFlight}
                      onClick={() => void onImportDevSetupConfiguration()}
                    >
                      {devSetupConfigurationBusy && !devSetupApplyInFlight
                        ? "가져오는 중..."
                        : devSetupConfigurationReview
                          ? "다시 가져오기"
                          : "구성 가져오기"}
                    </button>
                    {devSetupConfigurationReview && (
                      <>
                        <button
                          className="btn"
                          type="button"
                          disabled={
                            devSetupConfigurationBusy ||
                            devSetupApplyInFlight ||
                            devSetupConfigurationConsumed ||
                            devSetupConfigurationExpired ||
                            busy !== null ||
                            readBusy
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
                    <li>
                      설치 프로그램은 UAC/관리자 권한과 재부팅을 요구할 수 있지만 앱이 자동 재부팅을 예약하거나 실행하지
                      않습니다.
                    </li>
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
                            : devSetupConfigurationConsumed
                              ? "적용 토큰 사용됨"
                              : "적용 전 검토 가능"}
                        </span>
                      </div>
                      <dl className="dev-setup-config-facts">
                        <div>
                          <dt>schema</dt>
                          <dd>
                            <code>{devSetupConfigurationReview.schemaVersion}</code>
                          </dd>
                        </div>
                        <div>
                          <dt>외부 신뢰</dt>
                          <dd>
                            <span>
                              {devSetupConfigurationReview.sourceTrust === "external-restricted"
                                ? "외부 입력 · 제한 처리(신뢰 안 함)"
                                : "제한된 외부 입력"}
                            </span>{" "}
                            <code>{devSetupConfigurationReview.sourceTrust}</code>
                          </dd>
                        </div>
                        <div>
                          <dt>digest prefix</dt>
                          <dd>
                            <code>sha256:{devSetupConfigurationReview.configurationDigest.slice(0, 12)}…</code>
                          </dd>
                        </div>
                        <div>
                          <dt>적용 수명</dt>
                          <dd>5분 · 1회 적용</dd>
                        </div>
                      </dl>
                      <div className="dev-setup-config-scope dim">
                        mode: <code>{devSetupConfigurationReview.mode}</code> · 패키지{" "}
                        {devSetupConfigurationReview.packages.length}개
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
                      {devSetupConfigurationReview.hasChanges &&
                        !devSetupConfigurationReview.canApply &&
                        !devSetupConfigurationHasUnknown && (
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
                                className={
                                  packageReview.currentState === "unknown" || packageReview.action === "verify"
                                    ? "unknown"
                                    : undefined
                                }
                              >
                                <td>
                                  <code>{packageReview.packageId}</code>
                                  <div className="dev-setup-config-package-flags">
                                    <span className={packageReview.requestedAgreementAcceptance ? "requested" : "dim"}>
                                      {packageReview.requestedAgreementAcceptance
                                        ? "외부 약관 수락 요청"
                                        : "외부 약관 수락 없음"}
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
                        외부 파일의 선언을 실행 설정에 복사하지 않습니다. 약관 수락은 별도 확인하고, 외부 권한 선언과
                        무관하게 관리자/UAC·재부팅 가능성을 다시 고지합니다.
                      </div>
                      <div className="dev-setup-config-checks" aria-label="Dev Setup 적용 확인">
                        <label>
                          <input
                            type="checkbox"
                            checked={devSetupReviewAcknowledged}
                            disabled={
                              devSetupConfigurationBusy ||
                              devSetupConfigurationConsumed ||
                              devSetupConfigurationExpired ||
                              devSetupApplyInFlight
                            }
                            onChange={(event) => setDevSetupReviewAcknowledged(event.target.checked)}
                          />
                          정규화된 package-only 검토를 확인했습니다
                        </label>
                        <label>
                          <input
                            type="checkbox"
                            checked={devSetupAgreementsAccepted}
                            disabled={
                              devSetupConfigurationBusy ||
                              devSetupConfigurationConsumed ||
                              devSetupConfigurationExpired ||
                              devSetupApplyInFlight
                            }
                            onChange={(event) => setDevSetupAgreementsAccepted(event.target.checked)}
                          />
                          로컬에 등록된 고정 이름 winget source·패키지 약관 수락을 확인했습니다
                        </label>
                        <label>
                          <input
                            type="checkbox"
                            checked={devSetupAdminRiskAcknowledged}
                            disabled={
                              devSetupConfigurationBusy ||
                              devSetupConfigurationConsumed ||
                              devSetupConfigurationExpired ||
                              devSetupApplyInFlight
                            }
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
                이 capability 계획은 읽기 전용 확인 순서입니다. 아래 package-only apply는 별도 검토·약관·권한 위험 확인
                경계를 사용합니다.
              </p>
            </>
          )}
        </section>
      ) : tab === "related-tools" ? (
        <section className="related-tools" aria-busy={relatedBusy || readBusy} aria-labelledby="related-tools-heading">
          <div className="related-tools-head">
            <div>
              <h2 id="related-tools-heading">관련 도구</h2>
              <p className="dim">
                개발 흐름을 보완하는 작은 공식 도구 목록입니다. 로컬 실행 가능성과 설치 근거를 구분하며 경로와 버전은
                표시하지 않습니다. 감지와 이미 설치된 도구 실행은 인터넷 없이 가능합니다. WinGet 설치는 Windows와
                네트워크가 필요하고, 공식·라이선스 링크는 플랫폼과 관계없이 네트워크 연결 시 열 수 있습니다.
              </p>
            </div>
            <button
              className="btn"
              type="button"
              disabled={busy !== null || readBusy}
              onClick={() => void refreshRelatedTools()}
            >
              {relatedBusy ? "감지 중..." : "다시 감지"}
            </button>
          </div>
          {relatedError && (
            <div className="error related-tools-error" role="alert">
              {relatedError}
            </div>
          )}
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
                <article
                  key={tool.id}
                  className={`related-tool-card ${tool.installState === "present" ? "installed" : ""}`}
                >
                  <div className="related-tool-card-head">
                    <div>
                      <h3>{tool.displayName}</h3>
                      <p className="dim">{tool.summary}</p>
                    </div>
                    <span
                      className={`related-tool-state ${tool.installState === "present" ? "ok" : tool.installState === "unknown" ? "warning" : "dim"}`}
                    >
                      {installCapabilityLabel(tool.installState)}
                    </span>
                  </div>
                  <dl className="related-tool-facts">
                    <div>
                      <dt>감지</dt>
                      <dd>{relatedDetectionDescription(tool)}</dd>
                    </div>
                    <div>
                      <dt>Manager 실행</dt>
                      <dd>{availabilityCapabilityLabel(tool.launchState)}</dd>
                    </div>
                    {tool.dockerCapability && (
                      <>
                        <div>
                          <dt>Windows CLI</dt>
                          <dd>{availabilityCapabilityLabel(tool.dockerCapability.windowsCli)}</dd>
                        </div>
                        <div>
                          <dt>WSL backend</dt>
                          <dd>{backendCapabilityLabel(tool.dockerCapability.wslBackend)}</dd>
                        </div>
                      </>
                    )}
                    <div>
                      <dt>WinGet ID</dt>
                      <dd>
                        <code>{tool.wingetId}</code>
                      </dd>
                    </div>
                    <div>
                      <dt>라이선스</dt>
                      <dd>{tool.license}</dd>
                    </div>
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
                        disabled={busy !== null || readBusy}
                        onClick={() => void onRelatedLaunch(tool)}
                      >
                        {busy === `related:${tool.id}:launch` ? "실행 중..." : "실행"}
                      </button>
                    ) : tool.installState === "absent" ? (
                      <button
                        className="btn"
                        type="button"
                        aria-busy={busy === `related:${tool.id}:install`}
                        disabled={!tool.platformSupported || busy !== null || readBusy}
                        title={tool.platformSupported ? undefined : "WinGet 설치는 Windows에서만 사용할 수 있습니다."}
                        onClick={() => void onRelatedInstall(tool)}
                      >
                        {busy === `related:${tool.id}:install`
                          ? "설치 중..."
                          : tool.platformSupported
                            ? "확인 후 WinGet 설치"
                            : "WinGet 설치: Windows 전용"}
                      </button>
                    ) : (
                      <button
                        className="btn"
                        type="button"
                        disabled
                        title="설치 여부를 확정할 근거가 없어 자동 설치를 제안하지 않습니다. Dev Setup에서 근거를 확인하세요."
                      >
                        {tool.installState === "present" ? "실행 경로 확인 필요" : "설치 상태 확인 필요"}
                      </button>
                    )}
                  </div>
                </article>
              );
            })}
          </div>
          <p className="dim related-tools-note">
            Manager의 native 기능이 항상 기본 동작이며, 외부 도구는 선택적 보완재입니다. 자동 업데이트·제거·광범위한
            WinGet 검색은 지원하지 않습니다.
          </p>
        </section>
      ) : null}
    </div>
  );
}
