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
    return parseStore(localStorage.getItem(ENVIRONMENT_LS_KEY)) ?? emptyStore();
  } catch {
    // Storage access can be denied by the host WebView; treat it like a
    // corrupted store without surfacing the raw browser error.
    return emptyStore();
  }
}

/** Parse only the bounded environment wire shape used by localStorage. */
export function parseStore(raw: string | null): EnvironmentStore | null {
  try {
    const parsed = JSON.parse(raw ?? "null") as Partial<EnvironmentStore> | null;
    if (!parsed || parsed.version !== ENVIRONMENT_VERSION || !Array.isArray(parsed.environments)) return null;
    if (!parsed.environments.every(isEnvironment)) return null;
    return {
      version: ENVIRONMENT_VERSION,
      environments: parsed.environments.map((environment) => ({
        id: environment.id,
        name: environment.name,
        variables: environment.variables.map((variable) => ({
          key: variable.key,
          value: variable.value,
          secret: variable.secret,
        })),
      })),
    };
  } catch {
    return null;
  }
}

/** Persist with read-back verification and restore the previous value on failure. */
export function saveStore(store: EnvironmentStore, storage: Storage = localStorage): EnvironmentStore {
  const previous = storage.getItem(ENVIRONMENT_LS_KEY);
  const serialized = JSON.stringify(store);
  try {
    storage.setItem(ENVIRONMENT_LS_KEY, serialized);
    const readBack = parseStore(storage.getItem(ENVIRONMENT_LS_KEY));
    if (!readBack) throw new Error("Environment 안전 저장을 확인할 수 없습니다");
    return readBack;
  } catch (cause) {
    try {
      if (previous === null) storage.removeItem(ENVIRONMENT_LS_KEY);
      else storage.setItem(ENVIRONMENT_LS_KEY, previous);
    } catch {
      // Preserve the original persistence failure; callers must not treat a
      // failed rollback as a successful environment mutation.
    }
    throw cause;
  }
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

function isEnvironment(value: unknown): value is Environment {
  if (!value || typeof value !== "object") return false;
  const environment = value as Partial<Environment>;
  return typeof environment.id === "string"
    && typeof environment.name === "string"
    && Array.isArray(environment.variables)
    && environment.variables.every(isEnvironmentVariable);
}

function isEnvironmentVariable(value: unknown): value is EnvVariable {
  if (!value || typeof value !== "object") return false;
  const variable = value as Partial<EnvVariable>;
  return typeof variable.key === "string"
    && typeof variable.value === "string"
    && typeof variable.secret === "boolean";
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
