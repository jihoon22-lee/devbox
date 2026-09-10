import type {
  EnvironmentAction,
  EnvironmentDraft,
  Job,
  RestartPolicy,
  ServiceFieldErrors,
  ServiceInput,
  TargetKind,
} from "../types";

export interface ServiceDraft {
  name: string;
  command: string;
  cwd: string;
  targetKind: TargetKind;
  targetDistro: string;
  restartPolicy: RestartPolicy;
  autoStart: boolean;
  healthTcpEnabled: boolean;
  healthTcpAddress: string;
  healthTcpPort: string;
  environment: EnvironmentDraft[];
  environmentAction: EnvironmentAction;
}

export const EMPTY_SERVICE_DRAFT: ServiceDraft = {
  name: "",
  command: "",
  cwd: "",
  targetKind: "windows",
  targetDistro: "",
  restartPolicy: "never",
  autoStart: false,
  healthTcpEnabled: false,
  healthTcpAddress: "127.0.0.1",
  healthTcpPort: "",
  environment: [],
  environmentAction: "keep",
};

export function draftFromService(service: Job): ServiceDraft {
  const healthTcpEnabled = service.healthTcpAddress !== null || service.healthTcpPort !== null;
  return {
    name: service.name,
    command: service.command,
    cwd: service.cwd ?? "",
    targetKind: service.targetKind,
    targetDistro: service.targetDistro ?? "",
    restartPolicy: service.restartPolicy ?? "never",
    autoStart: service.autoStart ?? false,
    healthTcpEnabled,
    healthTcpAddress: service.healthTcpAddress ?? "127.0.0.1",
    healthTcpPort: service.healthTcpPort === null ? "" : String(service.healthTcpPort),
    environment: service.envConfigured
      ? [{ id: "persisted", key: "", value: "", persisted: true }]
      : [],
    environmentAction: "keep",
  };
}

export function toServiceInput(draft: ServiceDraft): ServiceInput {
  const values = Object.fromEntries(
    draft.environment
      .filter((entry) => !entry.persisted && entry.key.trim())
      .map((entry) => [entry.key.trim(), entry.value]),
  );
  const environment =
    draft.environmentAction === "clear"
      ? { action: "clear" as const }
      : draft.environmentAction === "replace"
        ? { action: "replace" as const, values }
        : { action: "keep" as const };
  return {
    name: draft.name.trim(),
    command: draft.command,
    cwd: draft.cwd.trim() || null,
    targetKind: draft.targetKind,
    targetDistro: draft.targetKind === "wsl" ? draft.targetDistro.trim() || null : null,
    restartPolicy: draft.restartPolicy,
    autoStart: draft.autoStart,
    healthTcpAddress: draft.healthTcpEnabled ? draft.healthTcpAddress.trim() || null : null,
    healthTcpPort: draft.healthTcpEnabled ? Number(draft.healthTcpPort) : null,
    environment,
  };
}

export function isLocalTcpAddress(value: string): boolean {
  const address = value.trim().toLowerCase();
  if (address === "localhost" || address === "::1") return true;
  const octets = address.split(".");
  if (octets.length !== 4 || octets.some((octet) => !/^\d+$/.test(octet))) return false;
  const values = octets.map(Number);
  return values.every((octet) => octet >= 0 && octet <= 255) && values[0] === 127;
}

export function validateServiceDraft(draft: ServiceDraft): ServiceFieldErrors {
  const errors: ServiceFieldErrors = {};
  if (!draft.name.trim()) errors.name = "서비스 이름을 입력하세요.";
  if (!draft.command.trim()) errors.command = "실행할 명령을 입력하세요.";

  if (draft.targetKind === "wsl" && !draft.targetDistro.trim()) {
    errors.targetDistro = "WSL 배포판을 입력하세요.";
  }
  if (draft.targetKind === "windows" && draft.targetDistro.trim()) {
    errors.targetDistro = "Windows 대상에는 WSL 배포판을 지정할 수 없습니다.";
  }

  if (draft.healthTcpEnabled) {
    if (!draft.healthTcpAddress.trim()) {
      errors.healthTcpAddress = "TCP 헬스체크 주소를 입력하세요.";
    } else if (!isLocalTcpAddress(draft.healthTcpAddress)) {
      errors.healthTcpAddress = "localhost 또는 루프백 IP만 사용할 수 있습니다.";
    }

    const port = Number(draft.healthTcpPort);
    if (!/^\d+$/.test(draft.healthTcpPort.trim()) || !Number.isInteger(port) || port < 1 || port > 65535) {
      errors.healthTcpPort = "TCP 포트는 1에서 65535 사이의 정수여야 합니다.";
    }
  }

  if (draft.environmentAction !== "replace") return errors;

  const keys = new Set<string>();
  draft.environment.some((entry) => {
    if (entry.persisted) return false;
    const key = entry.key.trim();
    const valuePresent = entry.value.length > 0;
    if (!key && !valuePresent) return false;
    if (!key) {
      errors.env = "환경변수 이름을 입력하세요.";
      return true;
    }
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(key)) {
      errors.env = "환경변수 이름은 영문자·숫자·밑줄만 사용할 수 있습니다.";
      return true;
    }
    const foldedKey = key.toUpperCase();
    if (foldedKey === "WSLENV" || foldedKey === "DEVBOX_RUN_MARKER") {
      errors.env = `${key}는 Run Manager가 관리하는 예약 키입니다.`;
      return true;
    }
    if (keys.has(foldedKey)) {
      errors.env = "환경변수 이름은 중복될 수 없습니다.";
      return true;
    }
    keys.add(foldedKey);
    return false;
  });
  return errors;
}

export function serviceFieldErrorFromBackend(message: string): ServiceFieldErrors {
  const normalized = message.toLowerCase();
  if (normalized.includes("health_tcp_address")) return { healthTcpAddress: "TCP 헬스체크 주소가 올바르지 않습니다." };
  if (normalized.includes("health_tcp_port")) return { healthTcpPort: "TCP 헬스체크 포트가 올바르지 않습니다." };
  if (normalized.includes("environment") || normalized.includes("dpapi")) return { env: "환경변수를 안전하게 저장하지 못했습니다." };
  if (normalized.includes("target_distro")) return { targetDistro: "WSL 대상 배포판이 올바르지 않습니다." };
  if (normalized.includes("command")) return { command: "실행 명령이 올바르지 않습니다." };
  if (normalized.includes("name")) return { name: "서비스 이름이 올바르지 않습니다." };
  return {};
}
