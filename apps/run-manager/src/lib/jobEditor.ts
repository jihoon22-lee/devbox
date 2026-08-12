import type {
  EnvironmentDraft,
  Job,
  JobFieldErrors,
  JobInput,
  TargetKind,
} from "../types";

export const CRON_PRESETS = [
  { id: "every-minute", label: "매분", expression: "0 * * * * *" },
  { id: "hourly", label: "매시간", expression: "0 0 * * * *" },
  { id: "daily-at-nine", label: "매일 오전 9시", expression: "0 0 9 * * *" },
  { id: "weekdays-at-nine", label: "평일 오전 9시", expression: "0 0 9 * * 1-5" },
  { id: "weekly-monday-at-nine", label: "매주 월요일 오전 9시", expression: "0 0 9 * * 1" },
] as const;

export interface JobDraft {
  name: string;
  command: string;
  cwd: string;
  targetKind: TargetKind;
  targetDistro: string;
  cronExpr: string;
  enabled: boolean;
  catchUp: boolean;
  overlapPolicy: JobInput["overlapPolicy"];
  environment: EnvironmentDraft[];
}

export const EMPTY_JOB_DRAFT: JobDraft = {
  name: "",
  command: "",
  cwd: "",
  targetKind: "windows",
  targetDistro: "",
  cronExpr: CRON_PRESETS[1].expression,
  enabled: false,
  catchUp: false,
  overlapPolicy: "skip",
  environment: [],
};

export function draftFromJob(job: Job): JobDraft {
  return {
    name: job.name,
    command: job.command,
    cwd: job.cwd ?? "",
    targetKind: job.targetKind,
    targetDistro: job.targetDistro ?? "",
    cronExpr: job.cronExpr ?? CRON_PRESETS[1].expression,
    enabled: job.enabled,
    catchUp: job.catchUp,
    overlapPolicy: job.overlapPolicy,
    // The read API intentionally gives us only configured/masked state. Keep
    // that state visible without ever copying ciphertext or plaintext into a
    // browser DTO.
    environment: job.envConfigured
      ? [{ id: "persisted", key: "", value: "", persisted: true }]
      : [],
  };
}

export function toJobInput(draft: JobDraft): JobInput {
  return {
    name: draft.name.trim(),
    command: draft.command,
    cwd: draft.cwd.trim() || null,
    targetKind: draft.targetKind,
    targetDistro: draft.targetKind === "wsl" ? draft.targetDistro.trim() || null : null,
    cronExpr: draft.cronExpr.trim(),
    enabled: draft.enabled,
    catchUp: draft.catchUp,
    overlapPolicy: draft.overlapPolicy,
  };
}

function validateCronShape(expression: string): string | null {
  const value = expression.trim();
  if (!value) return "cron 표현식을 입력하세요.";
  if (value.startsWith("@")) return "@reboot 같은 cron 매크로는 지원하지 않습니다.";
  const fieldCount = value.split(/\s+/).length;
  if (fieldCount !== 5 && fieldCount !== 6) {
    return "cron은 5개 또는 6개 필드(초 선택)를 사용해야 합니다.";
  }
  return null;
}

export function validateJobDraft(draft: JobDraft): JobFieldErrors {
  const errors: JobFieldErrors = {};
  if (!draft.name.trim()) errors.name = "작업 이름을 입력하세요.";
  if (!draft.command.trim()) errors.command = "실행할 명령을 입력하세요.";
  const cronError = validateCronShape(draft.cronExpr);
  if (cronError) errors.cronExpr = cronError;

  if (draft.targetKind === "wsl" && !draft.targetDistro.trim()) {
    errors.targetDistro = "WSL 배포판을 입력하세요.";
  }
  if (draft.targetKind === "windows" && draft.targetDistro.trim()) {
    errors.targetDistro = "Windows 대상에는 WSL 배포판을 지정할 수 없습니다.";
  }

  const keys = new Set<string>();
  const hasNewEnvironment = draft.environment.some((entry) => {
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
    if (key === "WSLENV" || key === "DEVBOX_RUN_MARKER") {
      errors.env = `${key}는 Run Manager가 관리하는 예약 키입니다.`;
      return true;
    }
    if (keys.has(key)) {
      errors.env = "환경변수 이름은 중복될 수 없습니다.";
      return true;
    }
    keys.add(key);
    // Values remain in the form only. The DPAPI adapter is the only allowed
    // path to persistence, so this is an intentional fail-safe until it is
    // available.
    errors.env = "환경변수 저장에는 Windows DPAPI 어댑터가 필요합니다.";
    return true;
  });
  if (hasNewEnvironment && !errors.env) {
    errors.env = "환경변수 저장에는 Windows DPAPI 어댑터가 필요합니다.";
  }
  return errors;
}

export function fieldErrorFromBackend(message: string): JobFieldErrors {
  const normalized = message.toLowerCase();
  if (normalized.includes("cron_expr")) return { cronExpr: message };
  if (normalized.includes("target_distro")) return { targetDistro: message };
  if (normalized.includes("command")) return { command: message };
  if (normalized.includes("name")) return { name: message };
  if (normalized.includes("environment") || normalized.includes("dpapi")) return { env: message };
  return {};
}

export function formatPreviewItem(item: { wallTime: string; datetime: string }): string {
  // wallTime is already resolved by Rust using the system-local timezone. It
  // is preferred over converting the RFC3339 instant in JavaScript so the UI
  // displays the same wall-clock tuple the scheduler will claim.
  return item.wallTime || new Date(item.datetime).toLocaleString();
}
