import { isProductHosted } from "../transport";
import { toolsCall } from "../calls";
import { openUrl } from "@tauri-apps/plugin-opener";
import catalogJson from "../../../../apps/products.json";
import { isTauri } from "./lib/isTauri";
import type {
  CapabilityEvidence,
  DevSetupAudit,
  DevSetupCapability,
  DevSetupConfigurationAction,
  DevSetupConfigurationApplyResult,
  DevSetupConfigurationApplyStatus,
  DevSetupConfigurationCurrentState,
  DevSetupConfigurationDesired,
  DevSetupConfigurationExport,
  DevSetupConfigurationPackageApplyResult,
  DevSetupConfigurationPackageApplyStatus,
  DevSetupConfigurationPackageReview,
  DevSetupConfigurationReview,
  DevSetupPlanItem,
  DockerCapability,
  RelatedTool,
  RelatedToolActionResult,
  SupportBundleExport,
  SupportBundlePreview,
} from "./types";
const MOCK_OBSERVED_AT_MS = Date.now();
const MAX_JAVASCRIPT_TIMESTAMP_MS = 8_640_000_000_000_000;
const DEV_SETUP_CONFIGURATION_SCHEMA =
  "https://raw.githubusercontent.com/PowerShell/DSC/main/schemas/2023/08/config/document.json";
const DEV_SETUP_CONFIGURATION_MAX_BYTES = 256 * 1024;
const DEV_SETUP_CONFIGURATION_MAX_PACKAGES = 16;
const DEV_SETUP_CONFIGURATION_PREVIEW_TTL_MS = 5 * 60 * 1_000;
const DEV_SETUP_PREVIEW_ID_PATTERN = /^devsetup-[0-9a-f]{64}$/;
const DEV_SETUP_SHA256_PATTERN = /^[0-9a-f]{64}$/;
const DEV_SETUP_CONFIGURATION_REVIEW_ERROR = "Dev Setup 구성 검토 응답이 올바르지 않습니다.";
const DEV_SETUP_CONFIGURATION_EXPORT_ERROR = "Dev Setup 구성 내보내기 응답이 올바르지 않습니다.";
const DEV_SETUP_CONFIGURATION_APPLY_ERROR = "Dev Setup 구성 적용 결과가 올바르지 않습니다.";
const DEV_SETUP_CONFIGURATION_REQUEST_ERROR = "Dev Setup 구성 요청이 올바르지 않습니다.";
const DEV_SETUP_CONFIGURATION_COMMAND_ERROR = "Dev Setup 구성 작업을 완료할 수 없습니다.";
const DEV_SETUP_CONFIRMATION_ERROR = "Dev Setup 적용에는 세 가지 확인이 모두 필요합니다.";
const MOCK_DEV_SETUP_PREVIEW_ID = `devsetup-${"b".repeat(64)}`;
const MOCK_UNKNOWN_TOOL_STATE = {
  installState: "unknown",
  launchState: "unknown",
  dockerCapability: null,
} as const;

const MOCK_RELATED_TOOLS: RelatedTool[] = [
  {
    id: "power-toys",
    displayName: "PowerToys",
    summary: "Windows 생산성 유틸리티 모음",
    wingetId: "Microsoft.PowerToys",
    officialUrl: "https://learn.microsoft.com/windows/powertoys/",
    licenseUrl: "https://github.com/microsoft/PowerToys/blob/main/LICENSE",
    license: "MIT (소스)",
    platformSupported: false,
    installed: false,
    detection: "unavailable",
    ...MOCK_UNKNOWN_TOOL_STATE,
  },
  {
    id: "windows-terminal",
    displayName: "Windows Terminal",
    summary: "탭·프로필을 지원하는 Windows 터미널",
    wingetId: "Microsoft.WindowsTerminal",
    officialUrl: "https://github.com/microsoft/terminal",
    licenseUrl: "https://github.com/microsoft/terminal/blob/main/LICENSE",
    license: "MIT",
    platformSupported: false,
    installed: false,
    detection: "unavailable",
    ...MOCK_UNKNOWN_TOOL_STATE,
  },
  {
    id: "vs-code",
    displayName: "Visual Studio Code",
    summary: "경량 코드 편집기",
    wingetId: "Microsoft.VisualStudioCode",
    officialUrl: "https://code.visualstudio.com/",
    licenseUrl: "https://code.visualstudio.com/License",
    license: "Microsoft 배포 약관 · 소스 MIT",
    platformSupported: false,
    installed: false,
    detection: "unavailable",
    ...MOCK_UNKNOWN_TOOL_STATE,
  },
  {
    id: "bruno",
    displayName: "Bruno",
    summary: "오프라인 우선 API 클라이언트",
    wingetId: "Bruno.Bruno",
    officialUrl: "https://www.usebruno.com/",
    licenseUrl: "https://github.com/usebruno/bruno/blob/main/LICENSE.md",
    license: "MIT",
    platformSupported: false,
    installed: false,
    detection: "unavailable",
    ...MOCK_UNKNOWN_TOOL_STATE,
  },
  {
    id: "dbeaver",
    displayName: "DBeaver Community",
    summary: "관계형 데이터베이스 탐색기",
    wingetId: "DBeaver.DBeaver.Community",
    officialUrl: "https://dbeaver.io/",
    licenseUrl: "https://github.com/dbeaver/dbeaver/blob/devel/LICENSE",
    license: "Apache-2.0",
    platformSupported: false,
    installed: false,
    detection: "unavailable",
    ...MOCK_UNKNOWN_TOOL_STATE,
  },
  {
    id: "db-browser",
    displayName: "DB Browser for SQLite",
    summary: "SQLite 데이터베이스 브라우저",
    wingetId: "DBBrowserForSQLite.DBBrowserForSQLite",
    officialUrl: "https://sqlitebrowser.org/",
    licenseUrl: "https://github.com/sqlitebrowser/sqlitebrowser/blob/master/LICENSE",
    license: "MPL-2.0",
    platformSupported: false,
    installed: false,
    detection: "unavailable",
    ...MOCK_UNKNOWN_TOOL_STATE,
  },
  {
    id: "github-desktop",
    displayName: "GitHub Desktop",
    summary: "GitHub 저장소용 데스크톱 클라이언트",
    wingetId: "GitHub.GitHubDesktop",
    officialUrl: "https://desktop.github.com/",
    licenseUrl: "https://github.com/desktop/desktop/blob/development/LICENSE",
    license: "MIT",
    platformSupported: false,
    installed: false,
    detection: "unavailable",
    ...MOCK_UNKNOWN_TOOL_STATE,
  },
  {
    id: "podman-desktop",
    displayName: "Podman Desktop",
    summary: "컨테이너와 Pod를 관리하는 데스크톱 앱",
    wingetId: "RedHat.Podman-Desktop",
    officialUrl: "https://podman-desktop.io/",
    licenseUrl: "https://github.com/containers/podman-desktop/blob/main/LICENSE",
    license: "Apache-2.0",
    platformSupported: false,
    installed: false,
    detection: "unavailable",
    ...MOCK_UNKNOWN_TOOL_STATE,
  },
  {
    id: "docker-desktop",
    displayName: "Docker Desktop",
    summary: "Docker 컨테이너 개발 환경",
    wingetId: "Docker.DockerDesktop",
    officialUrl: "https://www.docker.com/products/docker-desktop/",
    licenseUrl: "https://www.docker.com/legal/docker-software-license/",
    license: "Docker Software License",
    platformSupported: false,
    installed: false,
    detection: "unavailable",
    installState: "unknown",
    launchState: "unknown",
    dockerCapability: {
      desktopInstall: "unknown",
      desktopLaunch: "unknown",
      windowsCli: "unknown",
      wslBackend: "unknown",
      evidence: [
        { source: "desktop-executable", result: "unavailable" },
        { source: "windows-cli", result: "unavailable" },
        { source: "wsl-registration", result: "unavailable" },
        { source: "wsl-runtime", result: "unavailable" },
      ],
      observedAtMs: MOCK_OBSERVED_AT_MS,
    },
  },
];

const RELATED_TOOL_ID_SET = new Set(MOCK_RELATED_TOOLS.map((tool) => tool.id));
const RELATED_TOOL_ACTION_MESSAGES = {
  installed: "WinGet 설치가 완료되었습니다.",
  launched: "관련 도구를 실행했습니다.",
} as const;
const MAX_RELATED_TOOL_URL_LENGTH = 2048;

function isRelatedToolId(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length <= 64 &&
    /^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(value) &&
    RELATED_TOOL_ID_SET.has(value)
  );
}

function isRelatedDetection(value: unknown): value is RelatedTool["detection"] {
  return value === "path" || value === "known-location" || value === "not-found" || value === "unavailable";
}

function isInstallCapabilityState(value: unknown): value is RelatedTool["installState"] {
  return value === "present" || value === "absent" || value === "unknown";
}

function isAvailabilityCapabilityState(value: unknown): value is RelatedTool["launchState"] {
  return value === "available" || value === "unavailable" || value === "unknown";
}

const EVIDENCE_RESULTS: Record<string, ReadonlySet<string>> = {
  "desktop-executable": new Set(["path", "known-location", "not-observed", "unavailable"]),
  "windows-cli": new Set(["path", "known-location", "not-observed", "unavailable", "unrecognized"]),
  "wsl-registration": new Set(["registered", "not-registered", "unavailable"]),
  "wsl-runtime": new Set(["running", "stopped", "not-observed", "unavailable"]),
  "winget-executable": new Set(["trusted-location", "not-observed", "unavailable"]),
};
const DOCKER_EVIDENCE_SOURCES = ["desktop-executable", "windows-cli", "wsl-registration", "wsl-runtime"] as const;

function validateEvidence(value: unknown, expectedSources: readonly string[]): CapabilityEvidence[] {
  if (!Array.isArray(value) || value.length !== expectedSources.length) {
    throw new Error("환경 capability 응답이 올바르지 않습니다.");
  }
  return value.map((candidate, index) => {
    if (!candidate || typeof candidate !== "object") {
      throw new Error("환경 capability 응답이 올바르지 않습니다.");
    }
    const evidence = candidate as Partial<CapabilityEvidence>;
    if (
      evidence.source !== expectedSources[index] ||
      typeof evidence.result !== "string" ||
      !EVIDENCE_RESULTS[evidence.source]?.has(evidence.result)
    ) {
      throw new Error("환경 capability 응답이 올바르지 않습니다.");
    }
    return { source: evidence.source, result: evidence.result };
  });
}

function validateDockerCapability(value: unknown): DockerCapability {
  if (!value || typeof value !== "object") {
    throw new Error("Docker capability 응답이 올바르지 않습니다.");
  }
  const capability = value as Partial<DockerCapability>;
  if (
    !isInstallCapabilityState(capability.desktopInstall) ||
    !isAvailabilityCapabilityState(capability.desktopLaunch) ||
    !isAvailabilityCapabilityState(capability.windowsCli) ||
    !["running", "stopped", "present", "absent", "unknown"].includes(capability.wslBackend ?? "") ||
    typeof capability.observedAtMs !== "number" ||
    !Number.isSafeInteger(capability.observedAtMs) ||
    (capability.observedAtMs ?? 0) <= 0 ||
    (capability.observedAtMs ?? 0) > MAX_JAVASCRIPT_TIMESTAMP_MS
  ) {
    throw new Error("Docker capability 응답이 올바르지 않습니다.");
  }
  const evidence = validateEvidence(capability.evidence, DOCKER_EVIDENCE_SOURCES);
  const [desktopEvidence, cliEvidence, registrationEvidence, runtimeEvidence] = evidence;
  const expectedDesktop =
    desktopEvidence.result === "path" || desktopEvidence.result === "known-location"
      ? ["present", "available"]
      : desktopEvidence.result === "not-observed"
        ? ["unknown", "unavailable"]
        : ["unknown", "unknown"];
  const expectedCli =
    cliEvidence.result === "path" || cliEvidence.result === "known-location"
      ? "available"
      : cliEvidence.result === "not-observed"
        ? "unavailable"
        : "unknown";
  const backendEvidence = `${registrationEvidence.result}:${runtimeEvidence.result}`;
  const expectedBackend = {
    "registered:running": "running",
    "registered:stopped": "stopped",
    "registered:unavailable": "present",
    "not-registered:not-observed": "absent",
    "unavailable:unavailable": "unknown",
  }[backendEvidence];
  if (
    capability.desktopInstall !== expectedDesktop[0] ||
    capability.desktopLaunch !== expectedDesktop[1] ||
    capability.windowsCli !== expectedCli ||
    expectedBackend === undefined ||
    capability.wslBackend !== expectedBackend
  ) {
    throw new Error("Docker capability 응답이 올바르지 않습니다.");
  }
  return {
    desktopInstall: capability.desktopInstall,
    desktopLaunch: capability.desktopLaunch,
    windowsCli: capability.windowsCli,
    wslBackend: capability.wslBackend,
    evidence,
    observedAtMs: capability.observedAtMs,
  };
}

/**
 * Native returns a deliberately small DTO. Validate it at the API boundary
 * before React renders anything so a stale/tampered response cannot inject a
 * path, credential, arbitrary URL, or action state into the Manager screen.
 */
function validateRelatedTools(value: unknown): RelatedTool[] {
  if (!Array.isArray(value) || value.length !== MOCK_RELATED_TOOLS.length) {
    throw new Error("관련 도구 감지 응답이 올바르지 않습니다.");
  }
  const seen = new Set<string>();
  const result: RelatedTool[] = [];
  for (const candidate of value) {
    if (!candidate || typeof candidate !== "object") {
      throw new Error("관련 도구 감지 응답이 올바르지 않습니다.");
    }
    const tool = candidate as Partial<RelatedTool>;
    const expected = typeof tool.id === "string" ? MOCK_RELATED_TOOLS.find((item) => item.id === tool.id) : undefined;
    const detection = tool.detection;
    const installState = tool.installState;
    const launchState = tool.launchState;
    const isDocker = expected?.id === "docker-desktop";
    const dockerCapability = isDocker ? validateDockerCapability(tool.dockerCapability) : null;
    const ordinaryStates =
      detection === "path" || detection === "known-location"
        ? ["present", "available"]
        : detection === "not-found"
          ? ["absent", "unavailable"]
          : ["unknown", "unknown"];
    if (
      !expected ||
      seen.has(expected.id) ||
      !isRelatedDetection(detection) ||
      tool.displayName !== expected.displayName ||
      tool.summary !== expected.summary ||
      tool.wingetId !== expected.wingetId ||
      tool.officialUrl !== expected.officialUrl ||
      tool.licenseUrl !== expected.licenseUrl ||
      tool.license !== expected.license ||
      typeof tool.platformSupported !== "boolean" ||
      typeof tool.installed !== "boolean" ||
      (!tool.platformSupported && tool.installed) ||
      !isInstallCapabilityState(installState) ||
      !isAvailabilityCapabilityState(launchState) ||
      tool.installed !== (installState === "present") ||
      (isDocker &&
        (installState !== dockerCapability?.desktopInstall || launchState !== dockerCapability.desktopLaunch)) ||
      (!isDocker &&
        (tool.dockerCapability !== null || installState !== ordinaryStates[0] || launchState !== ordinaryStates[1])) ||
      (!tool.platformSupported && (installState !== "unknown" || launchState !== "unknown"))
    ) {
      throw new Error("관련 도구 감지 응답이 올바르지 않습니다.");
    }
    seen.add(expected.id);
    result.push({
      ...expected,
      platformSupported: tool.platformSupported,
      installed: tool.installed,
      detection,
      installState,
      launchState,
      dockerCapability,
    });
  }
  return result;
}

const DEV_SETUP_CAPABILITY_IDS = [
  "docker-desktop-install",
  "docker-desktop-launch",
  "docker-windows-cli",
  "docker-wsl-backend",
  "winget",
] as const;
const DEV_SETUP_SCOPES: Record<DevSetupCapability["id"], DevSetupCapability["scope"]> = {
  "docker-desktop-install": "windows",
  "docker-desktop-launch": "windows",
  "docker-windows-cli": "windows",
  "docker-wsl-backend": "wsl",
  winget: "windows",
};
const DEV_SETUP_EVIDENCE: Record<DevSetupCapability["id"], readonly string[]> = {
  "docker-desktop-install": ["desktop-executable"],
  "docker-desktop-launch": ["desktop-executable"],
  "docker-windows-cli": ["windows-cli"],
  "docker-wsl-backend": ["wsl-registration", "wsl-runtime"],
  winget: ["winget-executable"],
};

function isDevSetupCapabilityId(value: unknown): value is DevSetupCapability["id"] {
  return typeof value === "string" && DEV_SETUP_CAPABILITY_IDS.some((candidate) => candidate === value);
}

function expectedPlan(capability: DevSetupCapability): Pick<DevSetupPlanItem, "status" | "action"> | null {
  switch (capability.id) {
    case "docker-desktop-install":
      if (capability.state === "present") return { status: "satisfied", action: "none" };
      if (capability.state === "absent") return { status: "review", action: "review-install" };
      if (capability.state === "unknown") return { status: "unknown", action: "verify-installation" };
      return null;
    case "docker-desktop-launch":
      if (capability.state === "available") return { status: "satisfied", action: "none" };
      if (capability.state === "unavailable") return { status: "review", action: "review-launch-path" };
      if (capability.state === "unknown") return { status: "unknown", action: "verify-installation" };
      return null;
    case "docker-windows-cli":
      if (capability.state === "available") return { status: "satisfied", action: "none" };
      if (capability.state === "unavailable") return { status: "review", action: "review-cli" };
      if (capability.state === "unknown") return { status: "unknown", action: "verify-installation" };
      return null;
    case "docker-wsl-backend":
      if (capability.state === "running") return { status: "satisfied", action: "none" };
      if (capability.state === "stopped" || capability.state === "present") {
        return { status: "review", action: "start-backend" };
      }
      if (capability.state === "absent") return { status: "review", action: "review-backend" };
      if (capability.state === "unknown") return { status: "unknown", action: "verify-installation" };
      return null;
    case "winget":
      if (capability.state === "available") return { status: "satisfied", action: "none" };
      if (capability.state === "unavailable") return { status: "review", action: "review-winget" };
      if (capability.state === "unknown") return { status: "unknown", action: "verify-installation" };
      return null;
  }
}

function validateDevSetupAudit(value: unknown): DevSetupAudit {
  if (!value || typeof value !== "object") {
    throw new Error("Dev Setup 감사 응답이 올바르지 않습니다.");
  }
  const audit = value as Partial<DevSetupAudit>;
  if (
    audit.schemaVersion !== 1 ||
    audit.mode !== "read-only" ||
    typeof audit.observedAtMs !== "number" ||
    !Number.isSafeInteger(audit.observedAtMs) ||
    (audit.observedAtMs ?? 0) <= 0 ||
    (audit.observedAtMs ?? 0) > MAX_JAVASCRIPT_TIMESTAMP_MS ||
    !Array.isArray(audit.capabilities) ||
    audit.capabilities.length !== DEV_SETUP_CAPABILITY_IDS.length ||
    !Array.isArray(audit.plan) ||
    audit.plan.length !== DEV_SETUP_CAPABILITY_IDS.length
  ) {
    throw new Error("Dev Setup 감사 응답이 올바르지 않습니다.");
  }
  const capabilities = audit.capabilities.map((candidate, index): DevSetupCapability => {
    if (!candidate || typeof candidate !== "object") {
      throw new Error("Dev Setup 감사 응답이 올바르지 않습니다.");
    }
    const capability = candidate as Partial<DevSetupCapability>;
    const id = capability.id;
    if (
      !isDevSetupCapabilityId(id) ||
      id !== DEV_SETUP_CAPABILITY_IDS[index] ||
      capability.scope !== DEV_SETUP_SCOPES[id] ||
      typeof capability.state !== "string"
    ) {
      throw new Error("Dev Setup 감사 응답이 올바르지 않습니다.");
    }
    const normalized = {
      id,
      scope: capability.scope,
      state: capability.state,
      evidence: validateEvidence(capability.evidence, DEV_SETUP_EVIDENCE[id]),
    };
    if (!expectedPlan(normalized)) {
      throw new Error("Dev Setup 감사 응답이 올바르지 않습니다.");
    }
    return normalized;
  });
  const [desktopInstall, desktopLaunch, windowsCli, wslBackend, winget] = capabilities;
  if (
    desktopInstall.evidence[0].result !== desktopLaunch.evidence[0].result ||
    winget.state !==
      (winget.evidence[0].result === "trusted-location"
        ? "available"
        : winget.evidence[0].result === "not-observed"
          ? "unavailable"
          : "unknown")
  ) {
    throw new Error("Dev Setup 감사 응답이 올바르지 않습니다.");
  }
  validateDockerCapability({
    desktopInstall: desktopInstall.state as DockerCapability["desktopInstall"],
    desktopLaunch: desktopLaunch.state as DockerCapability["desktopLaunch"],
    windowsCli: windowsCli.state as DockerCapability["windowsCli"],
    wslBackend: wslBackend.state as DockerCapability["wslBackend"],
    evidence: [desktopInstall.evidence[0], windowsCli.evidence[0], ...wslBackend.evidence],
    observedAtMs: audit.observedAtMs,
  });
  const plan = audit.plan.map((candidate, index): DevSetupPlanItem => {
    if (!candidate || typeof candidate !== "object") {
      throw new Error("Dev Setup 감사 응답이 올바르지 않습니다.");
    }
    const item = candidate as Partial<DevSetupPlanItem>;
    const capability = capabilities[index];
    const expected = expectedPlan(capability);
    if (
      !expected ||
      item.capabilityId !== capability.id ||
      item.status !== expected?.status ||
      item.action !== expected.action
    ) {
      throw new Error("Dev Setup 감사 응답이 올바르지 않습니다.");
    }
    return {
      capabilityId: capability.id,
      status: item.status,
      action: item.action,
    };
  });
  return {
    schemaVersion: 1,
    observedAtMs: audit.observedAtMs,
    mode: "read-only",
    capabilities,
    plan,
  };
}

function mockDevSetupAudit(): DevSetupAudit {
  const capabilities: DevSetupCapability[] = DEV_SETUP_CAPABILITY_IDS.map((id) => ({
    id,
    scope: DEV_SETUP_SCOPES[id],
    state: "unknown",
    evidence: DEV_SETUP_EVIDENCE[id].map((source) => ({ source, result: "unavailable" })),
  }));
  return {
    schemaVersion: 1,
    observedAtMs: MOCK_OBSERVED_AT_MS,
    mode: "read-only",
    capabilities,
    plan: capabilities.map((capability) => ({
      capabilityId: capability.id,
      status: "unknown",
      action: "verify-installation",
    })),
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasExactKeys(value: Record<string, unknown>, keys: readonly string[]): boolean {
  return (
    Object.keys(value).length === keys.length && keys.every((key) => Object.prototype.hasOwnProperty.call(value, key))
  );
}

function isSafeDevSetupTimestamp(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0 && value <= MAX_JAVASCRIPT_TIMESTAMP_MS;
}

function isDevSetupPreviewId(value: unknown): value is string {
  return typeof value === "string" && DEV_SETUP_PREVIEW_ID_PATTERN.test(value);
}

function isDevSetupSha256(value: unknown): value is string {
  return typeof value === "string" && DEV_SETUP_SHA256_PATTERN.test(value);
}

function isDevSetupConfigurationDesired(value: unknown): value is DevSetupConfigurationDesired {
  return typeof value === "string" && (["present", "latest", "version"] as readonly string[]).includes(value);
}

function isDevSetupConfigurationCurrentState(value: unknown): value is DevSetupConfigurationCurrentState {
  return (
    typeof value === "string" &&
    (["present", "absent", "update-available", "unknown"] as readonly string[]).includes(value)
  );
}

function isDevSetupConfigurationAction(value: unknown): value is DevSetupConfigurationAction {
  return (
    typeof value === "string" &&
    (["none", "install", "update", "reconcile-version", "verify"] as readonly string[]).includes(value)
  );
}

function isDevSetupConfigurationApplyStatus(value: unknown): value is DevSetupConfigurationApplyStatus {
  return typeof value === "string" && (["complete", "partial", "cancelled"] as readonly string[]).includes(value);
}

function isDevSetupConfigurationPackageApplyStatus(value: unknown): value is DevSetupConfigurationPackageApplyStatus {
  return (
    typeof value === "string" &&
    (["unchanged", "applied", "failed", "timed-out", "cancelled", "skipped"] as readonly string[]).includes(value)
  );
}

function isDevSetupPackageId(value: unknown): value is string {
  if (typeof value !== "string" || value.length === 0 || value.length > 128) return false;
  const segments = value.split(".");
  return (
    segments.length >= 2 &&
    segments.length <= 8 &&
    segments.every(
      (segment) =>
        segment.length > 0 && segment.length <= 32 && /^[A-Za-z0-9](?:[A-Za-z0-9_-]*[A-Za-z0-9])?$/.test(segment),
    )
  );
}

function isDevSetupPackageVersion(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= 128 && /^[A-Za-z0-9._+-]+$/.test(value);
}

function expectedDevSetupAction(
  desired: DevSetupConfigurationDesired,
  currentState: DevSetupConfigurationCurrentState,
): DevSetupConfigurationAction | null {
  switch (currentState) {
    case "unknown":
      return "verify";
    case "absent":
      return "install";
    case "update-available":
      return desired === "latest" ? "update" : null;
    case "present":
      return desired === "version" ? "reconcile-version" : "none";
  }
}

function devSetupActionChangesSystem(action: DevSetupConfigurationAction): boolean {
  return action === "install" || action === "update" || action === "reconcile-version";
}

function validateDevSetupConfigurationPackage(value: unknown): DevSetupConfigurationPackageReview {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, [
      "packageId",
      "desired",
      "version",
      "currentState",
      "action",
      "requestedAgreementAcceptance",
      "declaredElevation",
    ])
  ) {
    throw new Error(DEV_SETUP_CONFIGURATION_REVIEW_ERROR);
  }
  const packageId = value.packageId;
  const desired = value.desired;
  const version = value.version;
  const currentState = value.currentState;
  const action = value.action;
  if (
    !isDevSetupPackageId(packageId) ||
    !isDevSetupConfigurationDesired(desired) ||
    !isDevSetupConfigurationCurrentState(currentState) ||
    !isDevSetupConfigurationAction(action) ||
    typeof value.requestedAgreementAcceptance !== "boolean" ||
    typeof value.declaredElevation !== "boolean"
  ) {
    throw new Error(DEV_SETUP_CONFIGURATION_REVIEW_ERROR);
  }
  if (desired === "version") {
    if (!isDevSetupPackageVersion(version)) {
      throw new Error(DEV_SETUP_CONFIGURATION_REVIEW_ERROR);
    }
  } else if (version !== null) {
    throw new Error(DEV_SETUP_CONFIGURATION_REVIEW_ERROR);
  }
  if (expectedDevSetupAction(desired, currentState) !== action) {
    throw new Error(DEV_SETUP_CONFIGURATION_REVIEW_ERROR);
  }
  return {
    packageId,
    desired,
    version: version as string | null,
    currentState,
    action,
    requestedAgreementAcceptance: value.requestedAgreementAcceptance,
    declaredElevation: value.declaredElevation,
  };
}

function validateDevSetupConfigurationReview(value: unknown): DevSetupConfigurationReview {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, [
      "schemaVersion",
      "previewId",
      "expiresAtMs",
      "configurationDigest",
      "sourceTrust",
      "mode",
      "canApply",
      "hasChanges",
      "requiresAgreementConfirmation",
      "mayRequireAdmin",
      "mayRequireReboot",
      "packages",
    ])
  ) {
    throw new Error(DEV_SETUP_CONFIGURATION_REVIEW_ERROR);
  }
  const now = Date.now();
  if (
    value.schemaVersion !== "0.3" ||
    !isDevSetupPreviewId(value.previewId) ||
    !isSafeDevSetupTimestamp(value.expiresAtMs) ||
    value.expiresAtMs <= now ||
    value.expiresAtMs > now + DEV_SETUP_CONFIGURATION_PREVIEW_TTL_MS ||
    !isDevSetupSha256(value.configurationDigest) ||
    value.sourceTrust !== "external-restricted" ||
    value.mode !== "package-only" ||
    typeof value.canApply !== "boolean" ||
    typeof value.hasChanges !== "boolean" ||
    typeof value.requiresAgreementConfirmation !== "boolean" ||
    typeof value.mayRequireAdmin !== "boolean" ||
    typeof value.mayRequireReboot !== "boolean" ||
    !Array.isArray(value.packages) ||
    value.packages.length < 1 ||
    value.packages.length > DEV_SETUP_CONFIGURATION_MAX_PACKAGES
  ) {
    throw new Error(DEV_SETUP_CONFIGURATION_REVIEW_ERROR);
  }
  const packages = value.packages.map(validateDevSetupConfigurationPackage);
  const seenPackageIds = new Set<string>();
  for (const packageReview of packages) {
    const normalizedId = packageReview.packageId.toLowerCase();
    if (seenPackageIds.has(normalizedId)) {
      throw new Error(DEV_SETUP_CONFIGURATION_REVIEW_ERROR);
    }
    seenPackageIds.add(normalizedId);
  }
  const hasChanges = packages.some((packageReview) => devSetupActionChangesSystem(packageReview.action));
  const hasVerify = packages.some((packageReview) => packageReview.action === "verify");
  if (
    value.hasChanges !== hasChanges ||
    value.canApply !== (hasChanges && !hasVerify) ||
    value.requiresAgreementConfirmation !== true ||
    value.mayRequireAdmin !== hasChanges ||
    value.mayRequireReboot !== hasChanges
  ) {
    throw new Error(DEV_SETUP_CONFIGURATION_REVIEW_ERROR);
  }
  return {
    schemaVersion: "0.3",
    previewId: value.previewId,
    expiresAtMs: value.expiresAtMs,
    configurationDigest: value.configurationDigest,
    sourceTrust: "external-restricted",
    mode: "package-only",
    canApply: value.canApply,
    hasChanges: value.hasChanges,
    requiresAgreementConfirmation: true,
    mayRequireAdmin: value.mayRequireAdmin,
    mayRequireReboot: value.mayRequireReboot,
    packages,
  };
}

function cloneDevSetupConfigurationReview(review: DevSetupConfigurationReview): DevSetupConfigurationReview {
  return {
    ...review,
    packages: review.packages.map((packageReview) => ({ ...packageReview })),
  };
}

let lastDevSetupConfigurationReview: DevSetupConfigurationReview | null = null;

function requireLastDevSetupConfigurationReview(previewId: string): DevSetupConfigurationReview {
  const review = lastDevSetupConfigurationReview;
  if (!review || review.previewId !== previewId || review.expiresAtMs <= Date.now()) {
    throw new Error(DEV_SETUP_CONFIGURATION_REQUEST_ERROR);
  }
  return review;
}

function renderDevSetupConfigurationExport(packages: readonly DevSetupConfigurationPackageReview[]): string {
  let content = `# yaml-language-server: $schema=https://aka.ms/configuration-dsc-schema/0.3\n$schema: ${DEV_SETUP_CONFIGURATION_SCHEMA}\nmetadata:\n  winget:\n    processor:\n      identifier: dscv3\nresources:\n`;
  for (const [index, packageReview] of packages.entries()) {
    content += `  - type: Microsoft.WinGet/Package\n    name: DevboxPackage${String(index + 1).padStart(2, "0")}\n    properties:\n      id: ${JSON.stringify(packageReview.packageId)}\n      source: winget\n      matchOption: equals\n`;
    if (packageReview.desired === "latest") {
      content += "      useLatest: true\n";
    } else if (packageReview.desired === "version") {
      content += `      version: ${JSON.stringify(packageReview.version)}\n`;
    }
    content +=
      "      installMode: silent\n    metadata:\n      description: Devbox reviewed package-only configuration\n";
  }
  return content;
}

function utf8ByteLength(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

async function sha256Utf8(value: string): Promise<string | null> {
  const subtle = globalThis.crypto?.subtle;
  if (!subtle) return null;
  try {
    const digest = await subtle.digest("SHA-256", new TextEncoder().encode(value));
    return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
  } catch {
    return null;
  }
}

function validateDevSetupConfigurationExportShape(
  value: unknown,
  review: DevSetupConfigurationReview,
): value is {
  filename: string;
  mimeType: string;
  content: string;
  byteCount: number;
  sha256: string;
} {
  if (!isRecord(value) || !hasExactKeys(value, ["filename", "mimeType", "content", "byteCount", "sha256"])) {
    throw new Error(DEV_SETUP_CONFIGURATION_EXPORT_ERROR);
  }
  if (
    value.filename !== "devbox-packages.winget" ||
    value.mimeType !== "application/yaml;charset=utf-8" ||
    typeof value.content !== "string" ||
    value.content.length === 0 ||
    utf8ByteLength(value.content) > DEV_SETUP_CONFIGURATION_MAX_BYTES ||
    typeof value.byteCount !== "number" ||
    !Number.isSafeInteger(value.byteCount) ||
    value.byteCount <= 0 ||
    value.byteCount !== utf8ByteLength(value.content) ||
    !isDevSetupSha256(value.sha256) ||
    value.sha256 !== review.configurationDigest ||
    value.content !== renderDevSetupConfigurationExport(review.packages)
  ) {
    throw new Error(DEV_SETUP_CONFIGURATION_EXPORT_ERROR);
  }
  return true;
}

async function validateDevSetupConfigurationExport(
  value: unknown,
  review: DevSetupConfigurationReview,
): Promise<DevSetupConfigurationExport> {
  if (!validateDevSetupConfigurationExportShape(value, review)) {
    throw new Error(DEV_SETUP_CONFIGURATION_EXPORT_ERROR);
  }
  const digest = await sha256Utf8(value.content);
  if (digest !== null && digest !== value.sha256) {
    throw new Error(DEV_SETUP_CONFIGURATION_EXPORT_ERROR);
  }
  return {
    filename: "devbox-packages.winget",
    mimeType: "application/yaml;charset=utf-8",
    content: value.content,
    byteCount: value.byteCount,
    sha256: value.sha256,
  };
}

function validateDevSetupConfigurationApply(
  value: unknown,
  review: DevSetupConfigurationReview,
): DevSetupConfigurationApplyResult {
  const expectedPackageIds = review.packages.map((packageReview) => packageReview.packageId);
  if (!isRecord(value) || !hasExactKeys(value, ["status", "observedAtMs", "results"])) {
    throw new Error(DEV_SETUP_CONFIGURATION_APPLY_ERROR);
  }
  if (
    !isDevSetupConfigurationApplyStatus(value.status) ||
    !isSafeDevSetupTimestamp(value.observedAtMs) ||
    !Array.isArray(value.results) ||
    value.results.length !== expectedPackageIds.length
  ) {
    throw new Error(DEV_SETUP_CONFIGURATION_APPLY_ERROR);
  }
  const results = value.results.map((candidate, index): DevSetupConfigurationPackageApplyResult => {
    if (!isRecord(candidate) || !hasExactKeys(candidate, ["packageId", "status"])) {
      throw new Error(DEV_SETUP_CONFIGURATION_APPLY_ERROR);
    }
    if (
      candidate.packageId !== expectedPackageIds[index] ||
      !isDevSetupConfigurationPackageApplyStatus(candidate.status)
    ) {
      throw new Error(DEV_SETUP_CONFIGURATION_APPLY_ERROR);
    }
    const action = review.packages[index].action;
    const cancellationStatus = candidate.status === "cancelled" || candidate.status === "skipped";
    if (
      (!cancellationStatus && action === "none" && candidate.status !== "unchanged") ||
      (!cancellationStatus && action !== "none" && candidate.status === "unchanged")
    ) {
      throw new Error(DEV_SETUP_CONFIGURATION_APPLY_ERROR);
    }
    return {
      packageId: candidate.packageId,
      status: candidate.status,
    };
  });
  const statuses = results.map((result) => result.status);
  const complete = statuses.every((status) => status === "applied" || status === "unchanged");
  const hasCancelled = statuses.includes("cancelled");
  const hasSkipped = statuses.includes("skipped");
  const cancellationSignal = hasCancelled || hasSkipped;
  const hasTimedOut = statuses.includes("timed-out");
  const partialFailure = statuses.some((status) => status === "failed" || status === "timed-out");
  if (
    (value.status === "complete" && !complete) ||
    (value.status === "cancelled" && (!cancellationSignal || complete)) ||
    (value.status === "partial" && (complete || hasCancelled || !partialFailure || (hasSkipped && !hasTimedOut)))
  ) {
    throw new Error(DEV_SETUP_CONFIGURATION_APPLY_ERROR);
  }
  return {
    status: value.status,
    observedAtMs: value.observedAtMs,
    results,
  };
}

function mockDevSetupConfigurationReview(): DevSetupConfigurationReview {
  return {
    schemaVersion: "0.3",
    previewId: MOCK_DEV_SETUP_PREVIEW_ID,
    expiresAtMs: Date.now() + DEV_SETUP_CONFIGURATION_PREVIEW_TTL_MS,
    configurationDigest: "aa87f279960f3b6e999bca6d55dacdfbacd71cb83a4f1772b729526e58dc2cf9",
    sourceTrust: "external-restricted",
    mode: "package-only",
    canApply: true,
    hasChanges: true,
    requiresAgreementConfirmation: true,
    mayRequireAdmin: true,
    mayRequireReboot: true,
    packages: [
      {
        packageId: "Git.Git",
        desired: "latest",
        version: null,
        currentState: "absent",
        action: "install",
        requestedAgreementAcceptance: true,
        declaredElevation: false,
      },
      {
        packageId: "Microsoft.VisualStudioCode",
        desired: "present",
        version: null,
        currentState: "present",
        action: "none",
        requestedAgreementAcceptance: false,
        declaredElevation: false,
      },
    ],
  };
}

function mockDevSetupConfigurationExport(review: DevSetupConfigurationReview): DevSetupConfigurationExport {
  const content = renderDevSetupConfigurationExport(review.packages);
  return {
    filename: "devbox-packages.winget",
    mimeType: "application/yaml;charset=utf-8",
    content,
    byteCount: utf8ByteLength(content),
    sha256: review.configurationDigest,
  };
}

function validateRelatedAction(
  value: unknown,
  toolId: string,
  status: RelatedToolActionResult["status"],
): RelatedToolActionResult {
  if (
    !value ||
    typeof value !== "object" ||
    (value as Partial<RelatedToolActionResult>).toolId !== toolId ||
    (value as Partial<RelatedToolActionResult>).status !== status
  ) {
    throw new Error("관련 도구 작업 결과가 올바르지 않습니다.");
  }
  return {
    toolId,
    status,
    // Never render native-provided message text: a future process error must
    // not turn into a path, account name, credential, or package-manager log.
    message: RELATED_TOOL_ACTION_MESSAGES[status],
  };
}

export type DiagnosisItem = import("../generated/DiagnosisItem").DiagnosisItem;

export async function runDiagnosis(): Promise<DiagnosisItem[]> {
  if (!isTauri()) {
    return [
      { name: "wsl", ok: true, detail: "WSL version 2.4.4" },
      { name: "git", ok: true, detail: "git version 2.45.0" },
      { name: "node", ok: true, detail: "v22.22.1" },
      { name: "pnpm", ok: true, detail: "9.0.0" },
      { name: "rustc", ok: true, detail: "rustc 1.97.1" },
      { name: "cargo", ok: true, detail: "cargo 1.97.1" },
      { name: "devbox-data", ok: true, detail: "카탈로그 14개 · 데이터 디렉터리 존재 10개" },
      { name: "catalog-ids", ok: true, detail: "모든 identifier가 com.devbox.*" },
      { name: "runtime-metadata", ok: true, detail: "runtime catalog와 install-root locator 정합" },
    ];
  }
  return toolsCall("run_diagnosis", {});
}

export async function previewSupportBundle(operationId: string): Promise<SupportBundlePreview> {
  if (!isTauri()) {
    return {
      previewId: `browser-support-${operationId}`,
      expiresAtMs: Date.now() + 300_000,
      estimatedBytes: 2048,
      databaseCount: 0,
      includedSections: ["diagnosis", "products", "operation-log"],
      omittedSections: ["raw-database", "raw-logs", "paths", "environment-values", "credentials", "authorization"],
      redactionVersion: "v1",
    };
  }
  return toolsCall("preview_support_bundle", { operationId });
}

export async function cancelSupportBundle(operationId: string): Promise<void> {
  if (!isTauri()) return;
  await toolsCall("cancel_support_bundle", { request: { operationId } });
}

export async function exportSupportBundle(previewId: string): Promise<SupportBundleExport> {
  if (!isTauri()) {
    const content = JSON.stringify(
      {
        schemaVersion: 3,
        products: catalogJson.products.map((product) => ({ id: product.id, version: "0.8.1" })),
        diagnosis: [],
        operations: [],
        redaction: { version: "v1", paths: "omitted", secrets: "omitted", rawLogs: "omitted" },
        omitted: ["raw-database-bytes", "raw-log-lines", "filesystem-paths", "credentials"],
      },
      null,
      2,
    );
    return {
      filename: "devbox-support-bundle.json",
      mimeType: "application/json",
      content,
      byteCount: content.length,
      redactionVersion: "v1",
    };
  }
  return toolsCall("export_support_bundle", { previewId });
}

export async function relatedTools(): Promise<RelatedTool[]> {
  const result = isTauri() ? await toolsCall("related_tools", {}) : MOCK_RELATED_TOOLS.map((tool) => ({ ...tool }));
  return validateRelatedTools(result);
}

export async function devSetupAudit(): Promise<DevSetupAudit> {
  const result = isTauri() ? await toolsCall("dev_setup_audit", {}) : mockDevSetupAudit();
  return validateDevSetupAudit(result);
}

export async function importDevSetupConfiguration(): Promise<DevSetupConfigurationReview | null> {
  // A failed or cancelled import must not leave a previous preview eligible
  // for an unrelated export/apply action.
  lastDevSetupConfigurationReview = null;
  let result: unknown;
  if (!isTauri()) {
    result = mockDevSetupConfigurationReview();
  } else {
    try {
      result = await toolsCall("import_dev_setup_configuration", {});
    } catch {
      throw new Error(DEV_SETUP_CONFIGURATION_COMMAND_ERROR);
    }
  }
  if (result === null) return null;
  const review = validateDevSetupConfigurationReview(result);
  lastDevSetupConfigurationReview = cloneDevSetupConfigurationReview(review);
  return review;
}

export async function discardDevSetupConfiguration(previewId: string): Promise<void> {
  if (!isDevSetupPreviewId(previewId)) {
    throw new Error(DEV_SETUP_CONFIGURATION_REQUEST_ERROR);
  }
  const review = lastDevSetupConfigurationReview;
  if (review && review.previewId !== previewId) {
    throw new Error(DEV_SETUP_CONFIGURATION_REQUEST_ERROR);
  }
  if (isTauri()) {
    try {
      await toolsCall("discard_dev_setup_configuration", {
        request: { previewId },
      });
    } catch {
      throw new Error(DEV_SETUP_CONFIGURATION_COMMAND_ERROR);
    }
  }
  if (lastDevSetupConfigurationReview?.previewId === previewId) {
    lastDevSetupConfigurationReview = null;
  }
}

export async function exportDevSetupConfiguration(previewId: string): Promise<DevSetupConfigurationExport> {
  if (!isDevSetupPreviewId(previewId)) {
    throw new Error(DEV_SETUP_CONFIGURATION_REQUEST_ERROR);
  }
  const review = requireLastDevSetupConfigurationReview(previewId);
  let result: unknown;
  if (!isTauri()) {
    result = mockDevSetupConfigurationExport(review);
  } else {
    try {
      result = await toolsCall("export_dev_setup_configuration", {
        request: { previewId },
      });
    } catch {
      throw new Error(DEV_SETUP_CONFIGURATION_COMMAND_ERROR);
    }
  }
  return validateDevSetupConfigurationExport(result, review);
}

export async function applyDevSetupConfiguration(
  previewId: string,
  confirmed: boolean,
  acceptPackageAgreements: boolean,
  acknowledgeAdminAndReboot: boolean,
): Promise<DevSetupConfigurationApplyResult> {
  if (
    !isDevSetupPreviewId(previewId) ||
    typeof confirmed !== "boolean" ||
    typeof acceptPackageAgreements !== "boolean" ||
    typeof acknowledgeAdminAndReboot !== "boolean"
  ) {
    throw new Error(DEV_SETUP_CONFIGURATION_REQUEST_ERROR);
  }
  if (!confirmed || !acceptPackageAgreements || !acknowledgeAdminAndReboot) {
    throw new Error(DEV_SETUP_CONFIRMATION_ERROR);
  }
  const review = requireLastDevSetupConfigurationReview(previewId);
  if (!review.canApply || !review.hasChanges) {
    throw new Error(DEV_SETUP_CONFIGURATION_APPLY_ERROR);
  }
  const native = isTauri();
  // Native consumes the one-time preview before starting work. Clear the
  // renderer-side copy before invoking so a rejected/failed call cannot be
  // retried with stale package expectations.
  lastDevSetupConfigurationReview = null;
  let result: unknown;
  if (!native) {
    result = {
      status: "complete",
      observedAtMs: Date.now(),
      results: review.packages.map((packageReview) => ({
        packageId: packageReview.packageId,
        status: devSetupActionChangesSystem(packageReview.action) ? "applied" : "unchanged",
      })),
    };
  } else {
    try {
      result = await toolsCall("apply_dev_setup_configuration", {
        request: {
          previewId,
          confirmed,
          acceptPackageAgreements,
          acknowledgeAdminAndReboot,
        },
      });
    } catch {
      throw new Error(DEV_SETUP_CONFIGURATION_COMMAND_ERROR);
    }
  }
  const validated = validateDevSetupConfigurationApply(result, review);
  return validated;
}

export async function cancelDevSetupApply(): Promise<void> {
  if (!isTauri()) return;
  try {
    await toolsCall("cancel_dev_setup_apply", {});
  } catch {
    throw new Error(DEV_SETUP_CONFIGURATION_COMMAND_ERROR);
  }
}

export async function installRelatedTool(toolId: string, confirmed: boolean): Promise<RelatedToolActionResult> {
  if (!isRelatedToolId(toolId)) throw new Error("관련 도구 식별자가 올바르지 않습니다.");
  if (typeof confirmed !== "boolean") throw new Error("관련 도구 설치 확인값이 올바르지 않습니다.");
  if (!isTauri()) {
    if (!confirmed) throw new Error("관련 도구 설치는 사용자 확인이 필요합니다.");
    return validateRelatedAction(
      {
        toolId,
        status: "installed",
        message: "WinGet 설치가 완료되었습니다.",
      },
      toolId,
      "installed",
    );
  }
  const result = await toolsCall("install_related_tool", {
    request: { toolId, confirmed },
  });
  return validateRelatedAction(result, toolId, "installed");
}

export async function launchRelatedTool(toolId: string): Promise<RelatedToolActionResult> {
  if (!isRelatedToolId(toolId)) throw new Error("관련 도구 식별자가 올바르지 않습니다.");
  if (!isTauri()) {
    return validateRelatedAction(
      {
        toolId,
        status: "launched",
        message: "관련 도구를 실행했습니다.",
      },
      toolId,
      "launched",
    );
  }
  const result = await toolsCall("launch_related_tool", { toolId });
  return validateRelatedAction(result, toolId, "launched");
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

function isSafeRelatedToolUrl(value: unknown): value is string {
  try {
    if (typeof value !== "string" || value.length > MAX_RELATED_TOOL_URL_LENGTH) return false;
    const url = new URL(value);
    return (
      url.protocol === "https:" &&
      !url.username &&
      !url.password &&
      !url.port &&
      url.hostname.length > 0 &&
      RELATED_TOOL_OFFICIAL_HOSTS.has(url.hostname)
    );
  } catch {
    return false;
  }
}

/** Open only a URL that passed the Related Tools official-host allowlist. */
export async function openRelatedToolUrl(url: string): Promise<void> {
  if (!isSafeRelatedToolUrl(url)) throw new Error("공식 링크가 올바르지 않습니다.");
  if (!isTauri()) {
    window.open(url, "_blank", "noopener,noreferrer");
    return;
  }
  if (isProductHosted()) await toolsCall("open_related_url", { url });
  else await openUrl(url);
}
