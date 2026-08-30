import type { ProfileTemplate } from "../api";
import {
  MAX_EXPECTED_PORTS_INPUT_CHARS,
  MAX_PROFILE_PATH_BYTES,
  MAX_SERVICES,
  MAX_SERVICE_ID_CHARS,
  MAX_WSL_DISTRO_CHARS,
  parseExpectedPorts,
} from "./profileEditor";
import { newServiceDraftRow, type ProfileDraft } from "./profileEditor";

export interface ProfileTemplateDraft {
  id: string;
  name: string;
  windowsPath: string;
  wslDistro: string;
  wslPath: string;
  gitRoot: string;
  expectedPortsText: string;
  serviceIdsText: string;
}

export interface ProfileTemplateDraftErrors {
  name?: string;
  projectPath?: string;
  wsl?: string;
  gitRoot?: string;
  expectedPorts?: string;
  services?: string;
}

export interface ProfileTemplateDraftValidation {
  template: ProfileTemplate | null;
  errors: ProfileTemplateDraftErrors;
}

export function emptyProfileTemplateDraft(): ProfileTemplateDraft {
  return {
    id: "",
    name: "",
    windowsPath: "",
    wslDistro: "",
    wslPath: "",
    gitRoot: "",
    expectedPortsText: "",
    serviceIdsText: "",
  };
}

export function templateDraftFromTemplate(template: ProfileTemplate): ProfileTemplateDraft {
  return {
    id: template.id,
    name: template.name,
    windowsPath: template.windowsPath ?? "",
    wslDistro: template.wsl?.distro ?? "",
    wslPath: template.wsl?.path ?? "",
    gitRoot: template.gitRoot ?? "",
    expectedPortsText: template.expectedPorts.join(", "),
    serviceIdsText: template.runManagerServiceIds.join(", "),
  };
}

/** Convert defaults into the existing stable profile editor buffer. */
export function profileDraftFromTemplate(template: ProfileTemplate | null): ProfileDraft {
  const draft = template ? templateDraftFromTemplate(template) : emptyProfileTemplateDraft();
  return {
    id: "",
    name: "",
    windowsPath: draft.windowsPath,
    wslDistro: draft.wslDistro,
    wslPath: draft.wslPath,
    gitRoot: draft.gitRoot,
    expectedPortsText: draft.expectedPortsText,
    serviceRows: splitServiceIds(draft.serviceIdsText).map((id) => newServiceDraftRow(id)),
    environmentEnabled: false,
    environmentSource: "",
    environmentRevision: "",
    environmentVariables: [],
    environmentPreview: null,
  };
}

export function validateProfileTemplateDraft(
  draft: ProfileTemplateDraft,
): ProfileTemplateDraftValidation {
  const errors: ProfileTemplateDraftErrors = {};
  const name = draft.name.trim();
  const windowsPath = draft.windowsPath.trim();
  const wslDistro = draft.wslDistro.trim();
  const wslPath = draft.wslPath.trim();
  const gitRoot = draft.gitRoot.trim();

  if (!name) errors.name = "템플릿 이름을 입력하세요.";
  else if (Array.from(name).length > 120) errors.name = "템플릿 이름은 120자 이하여야 합니다.";
  else if (hasControlCharacter(name)) errors.name = "템플릿 이름에 제어 문자를 넣을 수 없습니다.";

  if (windowsPath && !isSafePath(windowsPath)) errors.projectPath = "프로젝트 경로가 올바르지 않습니다.";
  if (gitRoot && !isSafePath(gitRoot)) errors.gitRoot = "Git 루트 경로가 올바르지 않습니다.";
  if (wslPath && !wslDistro) errors.wsl = "WSL 경로를 사용하려면 배포판을 입력하세요.";
  if (!wslPath && wslDistro) errors.wsl = "WSL 배포판을 사용하려면 WSL 경로를 입력하세요.";
  if (wslPath && !isSafePath(wslPath)) errors.projectPath = "WSL 경로가 올바르지 않습니다.";
  if (wslDistro && Array.from(wslDistro).length > MAX_WSL_DISTRO_CHARS) errors.wsl = "WSL 배포판 이름이 너무 깁니다.";
  if (wslDistro && hasControlCharacter(wslDistro)) errors.wsl = "WSL 배포판에 제어 문자를 넣을 수 없습니다.";

  const ports = parseExpectedPorts(draft.expectedPortsText);
  if (draft.expectedPortsText.length > MAX_EXPECTED_PORTS_INPUT_CHARS) errors.expectedPorts = "예상 포트 입력이 너무 깁니다.";
  else if (ports.error) errors.expectedPorts = ports.error;

  const services = splitServiceIds(draft.serviceIdsText);
  if (services.length > MAX_SERVICES) errors.services = "서비스는 최대 128개까지 등록할 수 있습니다.";
  else if (services.some((id) => !isValidServiceId(id))) errors.services = "서비스 ID는 비어 있지 않은 안전한 값이어야 합니다.";
  else if (new Set(services).size !== services.length) errors.services = "같은 서비스 ID를 두 번 등록할 수 없습니다.";

  if (Object.values(errors).some(Boolean)) return { template: null, errors };
  return {
    errors,
    template: {
      id: draft.id,
      name,
      windowsPath: windowsPath || null,
      wsl: wslPath ? { distro: wslDistro, path: wslPath } : null,
      gitRoot: gitRoot || null,
      expectedPorts: ports.ports,
      runManagerServiceIds: services,
    },
  };
}

function splitServiceIds(input: string): string[] {
  const trimmed = input.trim();
  return trimmed ? trimmed.split(",").map((id) => id.trim()) : [];
}

function isValidServiceId(value: string): boolean {
  return Boolean(value)
    && value.length <= MAX_SERVICE_ID_CHARS
    && value === value.trim()
    && !hasControlCharacter(value);
}

function isSafePath(value: string): boolean {
  if (!value || utf8ByteLength(value) > MAX_PROFILE_PATH_BYTES || hasControlCharacter(value)) return false;
  if (/^(?:\\\\[?.]\\|\/\/[?.]\/)/u.test(value)) return false;

  const isDrive = /^[A-Za-z]:[\\/]/u.test(value);
  const isUnc = /^(?:\\\\|\/\/)[^\\/]+[\\/][^\\/]+(?:[\\/]|$)/u.test(value);
  const isPosix = value.startsWith("/") && !isUnc;
  if (!isPosix && !isDrive && !isUnc) return false;

  const componentSource = isDrive ? value.slice(3) : value;
  const components = componentSource.split(/[\\/]/u).filter(Boolean);
  const minimumComponents = isUnc ? 3 : 1;
  if (components.length < minimumComponents || components.some((component) => component === "." || component === "..")) {
    return false;
  }
  if (isPosix) return true;

  return components.every((component) => {
    if (component.endsWith(" ") || component.endsWith(".") || /[<>:"|?*]/u.test(component)) return false;
    const stem = (component.split(".")[0] ?? "").toUpperCase();
    return !["CON", "PRN", "AUX", "NUL", "CLOCK$", "CONIN$", "CONOUT$"].includes(stem)
      && !/^(?:COM|LPT)[1-9]$/u.test(stem);
  });
}

function utf8ByteLength(value: string): number {
  let bytes = 0;
  for (const character of value) {
    const codePoint = character.codePointAt(0) ?? 0;
    bytes += codePoint <= 0x7f ? 1 : codePoint <= 0x7ff ? 2 : codePoint <= 0xffff ? 3 : 4;
  }
  return bytes;
}

function hasControlCharacter(value: string): boolean {
  return Array.from(value).some((character) => {
    const codePoint = character.codePointAt(0) ?? 0;
    return codePoint < 0x20 || (codePoint >= 0x7f && codePoint <= 0x9f);
  });
}
