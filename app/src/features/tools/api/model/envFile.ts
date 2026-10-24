/**
 * 환경 파일(`env/<이름>.json`, 07b S5-3). 값에는 `{{secret:NAME}}` 참조나 민감하지 않은 값만 둔다.
 * 민감 이름·토큰 모양의 직접 입력 값은 저장할 때 비우고 `secret: true`로 표시한다(사용자가 비밀로 옮긴다).
 */
import type { PreviewEnvironment } from "../lib/importers/preview";
import { isExactVariableReference } from "../lib/references";
import { isSensitiveName, looksLikeSecret } from "../lib/secretPatterns";
import { safeFileName } from "./requestFile";

export interface EnvVariable {
  name: string;
  value: string;
  secret?: boolean;
}
export interface EnvFile {
  version: 1;
  name: string;
  variables: EnvVariable[];
}

export const ENV_FOLDER = "env";
const NAME = /^[A-Za-z0-9_.-]{1,128}$/;
const MAX_VARIABLES = 500;
const invalid = (): never => {
  throw new Error("환경 파일 형식이 올바르지 않습니다");
};

export const isEnvPath = (path: string) => path.startsWith(`${ENV_FOLDER}/`) && path.endsWith(".json");

export function parseEnvFile(text: string): EnvFile {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return invalid();
  }
  const value = raw as Partial<EnvFile> | null;
  if (!value || value.version !== 1 || typeof value.name !== "string" || !Array.isArray(value.variables)) invalid();
  if (value!.variables!.length > MAX_VARIABLES) invalid();
  const variables = value!.variables!.map((item): EnvVariable => {
    if (!item || typeof item.name !== "string" || !NAME.test(item.name) || typeof item.value !== "string") invalid();
    return item.secret === true
      ? { name: item.name, value: item.value, secret: true }
      : { name: item.name, value: item.value };
  });
  return { version: 1, name: value!.name!, variables };
}

const literalSecret = (variable: EnvVariable) =>
  variable.value !== "" &&
  !isExactVariableReference(variable.value) &&
  (variable.secret === true || isSensitiveName(variable.name) || looksLikeSecret(variable.value));

export function serializeEnvFile(file: EnvFile): { text: string; cleared: string[] } {
  const cleared: string[] = [];
  const variables = file.variables.map((variable) => {
    if (!NAME.test(variable.name)) invalid();
    if (literalSecret(variable)) {
      cleared.push(variable.name);
      return { name: variable.name, value: "", secret: true };
    }
    return variable.secret
      ? { name: variable.name, value: variable.value, secret: true }
      : { name: variable.name, value: variable.value };
  });
  return { text: `${JSON.stringify({ version: 1, name: file.name, variables }, null, 2)}\n`, cleared };
}

export function envFilePath(name: string, used: Set<string>): string {
  const base = safeFileName(name);
  for (let n = 1; ; n++) {
    const path = `${ENV_FOLDER}/${base}${n > 1 ? ` (${n})` : ""}.json`;
    if (!used.has(path.toLowerCase())) {
      used.add(path.toLowerCase());
      return path;
    }
  }
}

export function environmentFromPreview(environment: PreviewEnvironment): EnvFile {
  return {
    version: 1,
    name: environment.name,
    variables: environment.variables.map((variable) =>
      variable.secret ? { name: variable.key, value: "", secret: true } : { name: variable.key, value: variable.value },
    ),
  };
}
