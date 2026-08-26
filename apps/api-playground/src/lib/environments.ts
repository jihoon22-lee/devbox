// environment 정의와 {{variable}} 치환 (순수 로직).

export const ENVIRONMENT_LS_KEY = "apip-environments";

export interface EnvVariable {
  key: string;
  /** secret이면 봉인된 base64 blob을 저장한다 (평문 미보관). */
  value: string;
  secret: boolean;
}

export interface Environment {
  id: string;
  name: string;
  variables: EnvVariable[];
}

export interface EnvironmentStore {
  version: number;
  environments: Environment[];
}

export const ENVIRONMENT_VERSION = 1;

export function emptyStore(): EnvironmentStore {
  return { version: ENVIRONMENT_VERSION, environments: [] };
}

export function loadStore(): EnvironmentStore {
  try {
    const parsed = JSON.parse(localStorage.getItem(ENVIRONMENT_LS_KEY) ?? "null") as EnvironmentStore | null;
    if (parsed && parsed.version === ENVIRONMENT_VERSION && Array.isArray(parsed.environments)) {
      return parsed;
    }
  } catch {
    // 손상은 빈 스토어
  }
  return emptyStore();
}

export function saveStore(store: EnvironmentStore): void {
  localStorage.setItem(ENVIRONMENT_LS_KEY, JSON.stringify(store));
}

export function addEnvironment(
  store: EnvironmentStore,
  name: string,
  makeId: () => string,
): EnvironmentStore {
  const env: Environment = { id: makeId(), name: name.trim() || "새 환경", variables: [] };
  return { ...store, environments: [...store.environments, env] };
}

export function removeEnvironment(store: EnvironmentStore, id: string): EnvironmentStore {
  return { ...store, environments: store.environments.filter((e) => e.id !== id) };
}

export function setVariable(
  store: EnvironmentStore,
  envId: string,
  key: string,
  value: string,
  secret = false,
): EnvironmentStore {
  return {
    ...store,
    environments: store.environments.map((env) => {
      if (env.id !== envId) return env;
      const exists = env.variables.some((v) => v.key === key);
      return {
        ...env,
        variables: exists
          ? env.variables.map((v) => (v.key === key ? { ...v, value, secret } : v))
          : [...env.variables, { key, value, secret }],
      };
    }),
  };
}

/// 문자열의 `{{name}}` 또는 `${name}`을 치환한다. 알 수 없는 변수는 그대로 둔다.
export function applyVariables(template: string, variables: Map<string, string>): string {
  return template.replace(/\{\{\s*([a-zA-Z0-9_.-]+)\s*\}\}|\$\{\s*([a-zA-Z0-9_.-]+)\s*\}/g, (match, moustache: string, dollar: string) => {
    const name = moustache ?? dollar;
    return variables.get(name) ?? match;
  });
}

/// 요청의 URL·헤더·cookie·text multipart·body·params에 environment를 적용한다 (원본 template 불변).
export function applyToRequest<T>(request: T, variables: Map<string, string>): T {
  const out = { ...request } as Record<string, unknown>;
  if (typeof out.url === "string") out.url = applyVariables(out.url, variables);
  if (typeof out.body === "string") {
    out.body = out.body_kind === "multipart" ? "" : applyVariables(out.body, variables);
  }
  if (Array.isArray(out.headers)) {
    out.headers = (out.headers as Array<{ key: string; value: string; enabled?: boolean }>).map((h) => ({
      ...h,
      key: h.key,
      value: applyVariables(h.value, variables),
    }));
  }
  if (Array.isArray(out.cookies)) {
    out.cookies = (out.cookies as Array<{ name: string; value: string; enabled?: boolean }>).map((cookie) => ({
      ...cookie,
      name: cookie.name,
      value: applyVariables(cookie.value, variables),
    }));
  }
  if (Array.isArray(out.multipart)) {
    out.multipart = (out.multipart as Array<{
      kind: "text" | "file";
      name: string;
      value: string;
      file_path: string;
      file_name: string;
      content_type: string;
      enabled?: boolean;
    }>).map((part) => ({
      ...part,
      value: part.kind === "text" ? applyVariables(part.value, variables) : "",
    }));
  }
  if (Array.isArray(out.params)) {
    out.params = (out.params as Array<{ key: string; value: string }>).map((p) => ({
      key: p.key,
      value: applyVariables(p.value, variables),
    }));
  }
  if (out.auth && typeof out.auth === "object") {
    const auth = out.auth as Record<string, unknown>;
    out.auth = Object.fromEntries(
      Object.entries(auth).map(([key, value]) => [
        key,
        typeof value === "string" ? applyVariables(value, variables) : value,
      ]),
    );
  }
  return out as T;
}
