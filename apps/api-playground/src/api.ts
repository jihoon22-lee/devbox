import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";
import { applyToRequest, type EnvVariable } from "./lib/environments";
import type { ApiResponse, RequestTemplate } from "./types";

/** HTTP 요청 전송. 브라우저 미리보기에서는 fetch(CORS 제약 존재)로 대체한다. */
export async function sendRequest(
  req: RequestTemplate,
  environment: EnvVariable[],
): Promise<ApiResponse> {
  if (!isTauri()) return browserFetch(req, environment);
  return invoke<ApiResponse>("send_request", { req, environment });
}

/** 값을 봉인해 base64 blob을 반환한다. */
export async function sealSecret(value: string): Promise<string> {
  if (!isTauri()) throw new Error("secret 봉인은 데스크톱 앱에서만 사용할 수 있습니다");
  return invoke<string>("seal_secret", { value });
}

/** 저장 후보를 backend secret 경계에서 한 번 더 정화한다. */
export async function sanitizePersistedJson(
  serialized: string,
  environment: EnvVariable[],
): Promise<string> {
  if (!isTauri()) {
    if (environment.some((variable) => variable.secret)) {
      throw new Error("secret 검증은 데스크톱 앱에서만 사용할 수 있습니다");
    }
    return serialized;
  }
  return invoke<string>("sanitize_persisted_json", { serialized, environment });
}

/** 확인 뒤 한 번만 원문 cURL을 만들어 반환한다. 호출자는 즉시 사용하고 저장하지 않는다. */
export async function buildRevealedCurl(
  req: RequestTemplate,
  environment: EnvVariable[],
): Promise<string> {
  if (!isTauri()) throw new Error("원문 cURL 복사는 데스크톱 앱에서만 사용할 수 있습니다");
  return invoke<string>("build_revealed_curl", { req, environment });
}

async function browserFetch(req: RequestTemplate, environment: EnvVariable[]): Promise<ApiResponse> {
  if (environment.some((variable) => variable.secret)) {
    throw new Error("secret 포함 요청은 데스크톱 앱에서만 전송할 수 있습니다");
  }
  const variables = new Map(environment.map((variable) => [variable.key, variable.value]));
  const resolved = applyToRequest(req, variables);
  const start = performance.now();
  const headers: Record<string, string> = {};
  for (const h of resolved.headers) if (h.key) headers[h.key] = h.value;
  if (resolved.auth?.kind === "basic") {
    headers["Authorization"] = "Basic " + btoa(`${resolved.auth.username}:${resolved.auth.password}`);
  } else if (resolved.auth?.kind === "bearer") {
    headers["Authorization"] = "Bearer " + resolved.auth.token;
  } else if (resolved.auth?.kind === "apikey") {
    headers[resolved.auth.api_key] = resolved.auth.api_value;
  }
  const params = new URLSearchParams();
  for (const p of resolved.params) if (p.key) params.append(p.key, p.value);
  const sep = resolved.url.includes("?") ? "&" : "?";
  const url = params.size ? resolved.url + sep + params.toString() : resolved.url;

  let body: string | undefined;
  if (resolved.body_kind === "json" && resolved.body.trim()) {
    headers["Content-Type"] = "application/json";
    body = resolved.body;
  } else if (resolved.body_kind === "raw" && resolved.body) {
    body = resolved.body;
  }

  const resp = await fetch(url, { method: req.method, headers, body });
  const duration_ms = Math.round(performance.now() - start);
  const text = await resp.text();
  const respHeaders: { key: string; value: string }[] = [];
  resp.headers.forEach((v, k) =>
    respHeaders.push({ key: k, value: isSensitiveName(k) ? "[REDACTED]" : v }),
  );

  return {
    status: resp.status,
    status_text: resp.statusText,
    headers: respHeaders,
    duration_ms,
    size_bytes: text.length,
    body: redactBrowserText(text, resolved),
    is_json: (resp.headers.get("content-type") ?? "").includes("json"),
    final_url: redactUrl(resp.url),
    redirects: [],
  };
}

function isSensitiveName(name: string): boolean {
  return /(authorization|cookie|api[-_]?key|token|secret|password)/i.test(name);
}

function redactUrl(value: string): string {
  try {
    const url = new URL(value);
    for (const key of [...url.searchParams.keys()]) {
      if (isSensitiveName(key)) url.searchParams.set(key, "[REDACTED]");
    }
    if (url.username) url.username = "REDACTED";
    if (url.password) url.password = "REDACTED";
    return url.toString();
  } catch {
    return value;
  }
}

function redactBrowserText(text: string, req: RequestTemplate): string {
  const directSecrets = [
    req.auth?.username,
    req.auth?.password,
    req.auth?.token,
    req.auth?.api_value,
    ...req.headers.filter((header) => isSensitiveName(header.key)).map((header) => header.value),
    ...req.params.filter((param) => isSensitiveName(param.key)).map((param) => param.value),
  ].filter((value): value is string => Boolean(value));
  const exactRedacted = directSecrets.sort((a, b) => b.length - a.length).reduce(
    (result, secret) => result.split(secret).join("[REDACTED]"),
    text,
  );
  try {
    return JSON.stringify(redactBrowserJson(JSON.parse(exactRedacted) as unknown));
  } catch {
    return exactRedacted.replace(
      /((?:authorization|cookie|api[-_]?key|token|secret|password)\s*[=:]\s*)([^\s,;&]+)/gi,
      "$1[REDACTED]",
    );
  }
}

function redactBrowserJson(value: unknown, key = ""): unknown {
  if (isSensitiveName(key)) return "[REDACTED]";
  if (Array.isArray(value)) return value.map((item) => redactBrowserJson(item));
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(([childKey, child]) => [
        childKey,
        redactBrowserJson(child, childKey),
      ]),
    );
  }
  return value;
}
