import type {
  EnvironmentConflict,
  EnvironmentVariableMetadata,
  ProjectEnvironmentPreview,
  ProjectProfile,
} from "../api";

export interface ServiceDraftRow {
  key: string;
  value: string;
}

/**
 * The editor deliberately does not use ProjectProfile as its state.  Keeping
 * the text that the user typed here means invalid ports and half-filled
 * service rows stay visible until the user fixes or cancels them.
 */
export interface ProfileDraft {
  id: string;
  name: string;
  windowsPath: string;
  wslDistro: string;
  wslPath: string;
  gitRoot: string;
  expectedPortsText: string;
  serviceRows: ServiceDraftRow[];
  environmentEnabled: boolean;
  environmentSource: string;
  environmentRevision: string;
  environmentVariables: EnvironmentVariableMetadata[];
  /** Native preview is masked metadata only and is never sent in the profile DTO. */
  environmentPreview: ProjectEnvironmentPreview | null;
}

export interface ProfileDraftErrors {
  id?: string;
  name?: string;
  projectPath?: string;
  wsl?: string;
  gitRoot?: string;
  expectedPorts?: string;
  services?: string;
  environment?: string;
  serviceRows: Record<string, string>;
}

export const MAX_PROFILE_NAME_CHARS = 120;
export const MAX_PROFILE_ID_CHARS = 128;
export const MAX_PROFILE_PATH_BYTES = 4096;
export const MAX_WSL_DISTRO_CHARS = 128;
export const MAX_EXPECTED_PORTS = 128;
export const MAX_EXPECTED_PORTS_INPUT_CHARS = 8192;
export const MAX_SERVICE_ID_CHARS = 128;
export const MAX_SERVICES = 128;
export const MAX_ENVIRONMENT_SOURCE_BYTES = 256;
export const MAX_ENVIRONMENT_VARIABLES = 128;
export const MAX_ENVIRONMENT_NAME_BYTES = 128;
export const ENVIRONMENT_REVISION_BYTES = 64;

export interface ProfileDraftValidation {
  profile: ProjectProfile | null;
  errors: ProfileDraftErrors;
}

let nextServiceRowKey = 0;

export function newServiceDraftRow(value = ""): ServiceDraftRow {
  nextServiceRowKey += 1;
  return { key: `service-row-${nextServiceRowKey}`, value };
}

export function emptyProfileDraft(): ProfileDraft {
  return {
    id: "",
    name: "",
    windowsPath: "",
    wslDistro: "",
    wslPath: "",
    gitRoot: "",
    expectedPortsText: "",
    serviceRows: [],
    environmentEnabled: false,
    environmentSource: "",
    environmentRevision: "",
    environmentVariables: [],
    environmentPreview: null,
  };
}

export function draftFromProfile(profile: ProjectProfile): ProfileDraft {
  return {
    id: profile.id,
    name: profile.name,
    windowsPath: profile.windowsPath ?? "",
    wslDistro: profile.wsl?.distro ?? "",
    wslPath: profile.wsl?.path ?? "",
    gitRoot: profile.gitRoot ?? "",
    expectedPortsText: profile.expectedPorts.join(", "),
    serviceRows: profile.runManagerServiceIds.map((id) => newServiceDraftRow(id)),
    environmentEnabled: profile.environment?.enabled ?? false,
    environmentSource: profile.environment?.source ?? "",
    environmentRevision: profile.environment?.revision ?? "",
    environmentVariables: profile.environment?.variables.map((variable) => ({ ...variable })) ?? [],
    environmentPreview: null,
  };
}

function hasControlCharacter(value: string): boolean {
  return Array.from(value).some((character) => {
    const codePoint = character.codePointAt(0) ?? 0;
    return codePoint < 0x20 || (codePoint >= 0x7f && codePoint <= 0x9f);
  });
}

function hasUnsafeDistroCharacter(value: string): boolean {
  return /[;&|<>`$"'\\(){}*?\[\]!~#%]/u.test(value);
}

function utf8ByteLength(value: string): number {
  let bytes = 0;
  for (const character of value) {
    const codePoint = character.codePointAt(0) ?? 0;
    bytes += codePoint <= 0x7f ? 1 : codePoint <= 0x7ff ? 2 : codePoint <= 0xffff ? 3 : 4;
  }
  return bytes;
}

function isSafeEnvironmentSource(value: string): boolean {
  if (!value || utf8ByteLength(value) > MAX_ENVIRONMENT_SOURCE_BYTES) return false;
  if (hasControlCharacter(value) || value.includes("/") || value.includes("\\") || value.includes(":")) {
    return false;
  }
  if (value === ".env") return true;
  if (!value.startsWith(".env.")) return false;
  const suffix = value.slice(".env.".length);
  return Boolean(suffix)
    && !suffix.startsWith(".")
    && !suffix.includes("..")
    && !suffix.endsWith(".")
    && /^[A-Za-z0-9._-]+$/u.test(suffix);
}

function isValidEnvironmentName(value: string): boolean {
  return utf8ByteLength(value) <= MAX_ENVIRONMENT_NAME_BYTES
    && /^[A-Za-z_][A-Za-z0-9_]*$/u.test(value);
}

function isEnvironmentConflict(value: string): value is EnvironmentConflict {
  return value === "none"
    || value === "duplicate"
    || value === "reserved"
    || value === "duplicateAndReserved";
}

function expectedEnvironmentConflict(name: string, duplicate: boolean): EnvironmentConflict {
  const reserved = new Set([
    "PATH",
    "PATHEXT",
    "COMSPEC",
    "SHELL",
    "WSLENV",
    "LD_PRELOAD",
    "DYLD_INSERT_LIBRARIES",
    "PWD",
    "OLDPWD",
    "SHLVL",
    "HOME",
    "USER",
    "USERNAME",
    "SYSTEMROOT",
    "WINDIR",
    "TEMP",
    "TMP",
    "LOCALAPPDATA",
    "APPDATA",
    "PROGRAMDATA",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "LANG",
    "TERM",
  ]).has(name.toUpperCase());
  if (duplicate && reserved) return "duplicateAndReserved";
  if (duplicate) return "duplicate";
  if (reserved) return "reserved";
  return "none";
}

function isSecretEnvironmentName(name: string): boolean {
  const upper = name.toUpperCase();
  return ["PASSWORD", "PASSWD", "TOKEN", "SECRET", "APIKEY", "API_KEY", "ACCESSKEY", "ACCESS_KEY", "PRIVATEKEY", "PRIVATE_KEY", "CLIENTSECRET", "CLIENT_SECRET", "CREDENTIAL", "AUTH", "BEARER", "COOKIE", "SESSION"]
    .some((marker) => upper.includes(marker));
}

function validateEnvironmentMetadata(
  source: string,
  variables: EnvironmentVariableMetadata[],
): string | undefined {
  if (variables.length > MAX_ENVIRONMENT_VARIABLES) {
    return "환경 변수는 최대 128개까지 등록할 수 있습니다.";
  }
  const occurrences = new Map<string, number>();
  for (const variable of variables) {
    if (!isValidEnvironmentName(variable.name)) {
      return "환경 변수 이름이 올바르지 않습니다.";
    }
    if (variable.source !== source || !isEnvironmentConflict(variable.conflict)) {
      return "환경 변수 메타데이터가 올바르지 않습니다.";
    }
    const folded = variable.name.toUpperCase();
    occurrences.set(folded, (occurrences.get(folded) ?? 0) + 1);
    const expectedSecret = isSecretEnvironmentName(variable.name);
    // Tauri always serializes Option as null, but treat an omitted field as
    // absent too so a malformed IPC payload cannot turn validation into a
    // TypeError (or accidentally become a save authority).
    const reference = variable.secretReference;
    const hasReference = reference !== null && reference !== undefined;
    if (expectedSecret !== hasReference) {
      return "환경 secret reference가 올바르지 않습니다.";
    }
    if (hasReference
      && (reference.kind !== "secret-ref/v1" || reference.name !== variable.name)) {
      return "환경 secret reference가 올바르지 않습니다.";
    }
  }
  // Native parsing marks every occurrence when a later line introduces a
  // duplicate, including the first entry. Count the complete list before
  // checking conflicts so parser-produced metadata can be saved faithfully.
  for (const variable of variables) {
    const duplicate = (occurrences.get(variable.name.toUpperCase()) ?? 0) > 1;
    const expected = expectedEnvironmentConflict(variable.name, duplicate);
    if (variable.conflict !== expected) {
      return "환경 변수 충돌을 해결한 뒤 저장하세요.";
    }
  }
  return undefined;
}

export function parseExpectedPorts(input: string): { ports: number[]; error?: string } {
  if (input.length > MAX_EXPECTED_PORTS_INPUT_CHARS) {
    return { ports: [], error: "예상 포트 입력이 너무 깁니다." };
  }
  const raw = input.trim();
  if (!raw) return { ports: [] };

  const tokens = raw.split(",").map((token) => token.trim());
  if (tokens.length > MAX_EXPECTED_PORTS) {
    return { ports: [], error: "예상 포트는 최대 128개까지 등록할 수 있습니다." };
  }
  if (tokens.some((token) => token.length === 0)) {
    return { ports: [], error: "포트는 쉼표로 구분한 1~65535 사이의 숫자여야 합니다." };
  }

  const ports: number[] = [];
  for (const token of tokens) {
    if (!/^\d+$/.test(token)) {
      return { ports: [], error: "포트는 쉼표로 구분한 1~65535 사이의 숫자여야 합니다." };
    }
    const port = Number(token);
    if (!Number.isSafeInteger(port) || port < 1 || port > 65535) {
      return { ports: [], error: "포트는 1~65535 사이여야 합니다." };
    }
    if (ports.includes(port)) {
      return { ports: [], error: "같은 포트를 두 번 등록할 수 없습니다." };
    }
    ports.push(port);
  }
  return { ports };
}

function emptyErrors(): ProfileDraftErrors {
  return { serviceRows: {} };
}

function hasDraftErrors(errors: ProfileDraftErrors): boolean {
  return Object.entries(errors).some(([key, value]) => {
    if (key === "serviceRows") return Object.keys(value as Record<string, string>).length > 0;
    return Boolean(value);
  });
}

export function validateProfileDraft(draft: ProfileDraft): ProfileDraftValidation {
  const errors = emptyErrors();
  if (draft.id && (
    draft.id !== draft.id.trim()
    || Array.from(draft.id).length > MAX_PROFILE_ID_CHARS
    || hasControlCharacter(draft.id)
  )) {
    errors.id = "프로필 ID가 올바르지 않습니다.";
  }
  const name = draft.name.trim();
  const windowsPath = draft.windowsPath.trim();
  const wslDistro = draft.wslDistro.trim();
  const wslPath = draft.wslPath.trim();
  const gitRoot = draft.gitRoot.trim();
  const environmentSource = draft.environmentSource.trim();

  if (!name) errors.name = "프로필 이름을 입력하세요.";
  else if (Array.from(name).length > MAX_PROFILE_NAME_CHARS) errors.name = "프로필 이름은 120자 이하여야 합니다.";
  else if (hasControlCharacter(name)) errors.name = "프로필 이름에 제어 문자를 넣을 수 없습니다.";

  if (!windowsPath && !wslPath) {
    errors.projectPath = "Windows 경로 또는 WSL 경로를 하나 이상 입력하세요.";
  }
  if (wslPath && !wslDistro) errors.wsl = "WSL 경로를 사용하려면 distro를 입력하세요.";
  if (!wslPath && wslDistro) errors.wsl = "WSL distro를 사용하려면 WSL 경로를 입력하세요.";
  if (windowsPath && hasControlCharacter(windowsPath)) {
    errors.projectPath = "경로와 distro에는 제어 문자를 넣을 수 없습니다.";
  }
  if (wslPath && hasControlCharacter(wslPath)) {
    errors.projectPath = "경로와 distro에는 제어 문자를 넣을 수 없습니다.";
  }
  if (wslDistro && hasControlCharacter(wslDistro)) {
    errors.wsl = "경로와 distro에는 제어 문자를 넣을 수 없습니다.";
  }
  if (wslDistro && hasUnsafeDistroCharacter(wslDistro)) {
    errors.wsl = "WSL distro 이름에 허용되지 않는 문자가 있습니다.";
  }
  if (gitRoot && hasControlCharacter(gitRoot)) {
    errors.gitRoot = "Git root에 제어 문자를 넣을 수 없습니다.";
  }
  if (windowsPath && utf8ByteLength(windowsPath) > MAX_PROFILE_PATH_BYTES) {
    errors.projectPath = "Windows 경로가 너무 깁니다.";
  }
  if (wslPath && utf8ByteLength(wslPath) > MAX_PROFILE_PATH_BYTES) {
    errors.projectPath = "WSL 경로가 너무 깁니다.";
  }
  if (gitRoot && utf8ByteLength(gitRoot) > MAX_PROFILE_PATH_BYTES) {
    errors.gitRoot = "Git root 경로가 너무 깁니다.";
  }
  if (wslDistro && Array.from(wslDistro).length > MAX_WSL_DISTRO_CHARS) {
    errors.wsl = "WSL distro 이름이 너무 깁니다.";
  }

  const ports = parseExpectedPorts(draft.expectedPortsText);
  if (ports.error) errors.expectedPorts = ports.error;

  const serviceIds: string[] = [];
  const seenServices = new Set<string>();
  for (const row of draft.serviceRows.slice(0, MAX_SERVICES)) {
    const id = row.value.trim();
    if (!id) {
      errors.serviceRows[row.key] = "서비스 ID를 입력하거나 이 행을 삭제하세요.";
      continue;
    }
    if (Array.from(id).length > MAX_SERVICE_ID_CHARS) {
      errors.serviceRows[row.key] = "서비스 ID는 128자 이하여야 합니다.";
      continue;
    }
    if (hasControlCharacter(id)) {
      errors.serviceRows[row.key] = "서비스 ID에 제어 문자를 넣을 수 없습니다.";
      continue;
    }
    if (seenServices.has(id)) {
      errors.serviceRows[row.key] = "같은 서비스 ID를 두 번 등록할 수 없습니다.";
      continue;
    }
    seenServices.add(id);
    serviceIds.push(id);
  }
  if (draft.serviceRows.length > MAX_SERVICES) {
    errors.services = "서비스는 최대 128개까지 등록할 수 있습니다.";
  }

  if (!environmentSource) {
    if (draft.environmentEnabled || draft.environmentRevision || draft.environmentVariables.length > 0) {
      errors.environment = "환경 파일을 먼저 확인하거나 선택을 해제하세요.";
    }
  } else if (!isSafeEnvironmentSource(environmentSource)) {
    errors.environment = "환경 파일은 프로젝트 안의 .env 또는 .env.<이름>만 선택할 수 있습니다.";
  } else if (!/^[0-9a-f]{64}$/u.test(draft.environmentRevision)) {
    errors.environment = "환경 파일을 확인한 뒤 저장하세요.";
  } else {
    const metadataError = validateEnvironmentMetadata(environmentSource, draft.environmentVariables);
    if (metadataError) errors.environment = metadataError;
    else if (draft.environmentEnabled && draft.environmentVariables.some((variable) => variable.conflict !== "none")) {
      errors.environment = "환경 파일의 중복·예약 이름 충돌을 해결한 뒤 저장하세요.";
    }
    const previewMetadata = draft.environmentPreview?.variables.map(({
      name,
      source,
      conflict,
      secretReference,
    }) => ({ name, source, conflict, secretReference }));
    if (draft.environmentPreview
      && (draft.environmentPreview.source !== environmentSource
        || draft.environmentPreview.revision !== draft.environmentRevision
        || draft.environmentPreview.hasConflicts !== draft.environmentVariables.some((variable) => variable.conflict !== "none")
        || JSON.stringify(previewMetadata) !== JSON.stringify(draft.environmentVariables))) {
      errors.environment = "환경 파일 미리보기가 현재 선택과 일치하지 않습니다. 다시 확인하세요.";
    }
  }

  if (hasDraftErrors(errors)) return { profile: null, errors };

  return {
    errors,
    profile: {
      id: draft.id,
      name,
      windowsPath: windowsPath || null,
      wsl: wslPath ? { distro: wslDistro, path: wslPath } : null,
      gitRoot: gitRoot || null,
      expectedPorts: ports.ports,
      runManagerServiceIds: serviceIds,
      environment: environmentSource ? {
        enabled: draft.environmentEnabled,
        source: environmentSource,
        revision: draft.environmentRevision,
        variables: draft.environmentVariables.map((variable) => ({ ...variable })),
      } : null,
    },
  };
}
