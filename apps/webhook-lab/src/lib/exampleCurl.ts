import type { ResponseRule } from "../api";

export const REDACTED = "[REDACTED]";
export const MAX_EXAMPLE_PATH_CHARS = 4_096;
export const MAX_EXAMPLE_HEADER_COUNT = 100;
export const MAX_EXAMPLE_HEADER_NAME_CHARS = 256;
export const MAX_EXAMPLE_HEADER_VALUE_CHARS = 16_384;
export const MAX_EXAMPLE_HEADER_TOTAL_CHARS = 64_000;
export const MAX_EXAMPLE_BODY_CHARS = 256_000;
export const MAX_EXAMPLE_JSON_DEPTH = 32;
export const MAX_EXAMPLE_JSON_NODES = 10_000;
export const MAX_EXAMPLE_JSON_STRING_CHARS = 64_000;
export const MAX_EXAMPLE_OUTPUT_CHARS = 512_000;
export const MIN_EXAMPLE_STATUS = 100;
export const MAX_EXAMPLE_STATUS = 599;
export const MAX_EXAMPLE_DELAY_MS = 60_000;

const CONTROL_CHARACTERS = /[\u0000-\u001f\u007f]/;
const BODY_UNSAFE_CONTROL_CHARACTERS = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/;
const HTTP_TOKEN = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
const SENSITIVE_NAME = /(authorization|proxy-authorization|cookie|set-cookie|api[-_]?key|access[-_]?token|refresh[-_]?token|token|secret|password|passwd|private[-_]?key)/i;
const REFERENCE = /\{\{\s*[a-zA-Z0-9_.-]+\s*\}\}|\$\{\s*[a-zA-Z0-9_.-]+\s*\}/;
const WHOLE_REFERENCE = /^(?:\{\{\s*[a-zA-Z0-9_.-]+\s*\}\}|\$\{\s*[a-zA-Z0-9_.-]+\s*\})$/;
const KNOWN_TOKEN = /(?:sk-|ghp_|github_pat_|glpat-|xox[baprs]-)[A-Za-z0-9_.-]{12,}|AKIA[A-Z0-9]{16}|(?:[A-Za-z0-9_-]{10,}\.){2}[A-Za-z0-9_-]{10,}/;
const PRIVATE_KEY = /-----BEGIN [A-Z ]*PRIVATE KEY-----/;
const URI_SAFE_CHARACTERS = /^[A-Za-z0-9\-._~!$&'()*+,;=:@/?%\[\]*]*$/;

export type CurlShell = "powershell" | "posix";

/** POSIX sh single-quote escaping. A single quote closes and reopens the string. */
export function posixShellQuote(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

/** PowerShell single-quote escaping. A single quote is represented by two quotes. */
export function powershellQuote(value: string): string {
  return `'${value.replace(/'/g, "''")}'`;
}

/** Backwards-compatible alias for the POSIX formatter. */
export function shellQuote(value: string): string {
  return posixShellQuote(value);
}

function containsReference(value: string): boolean {
  return REFERENCE.test(value);
}

function isWholeReference(value: string): boolean {
  return WHOLE_REFERENCE.test(value);
}

function containsMixedReference(value: string): boolean {
  return containsReference(value) && !isWholeReference(value);
}

function isSensitiveName(name: string): boolean {
  return SENSITIVE_NAME.test(name);
}

function containsKnownToken(value: string): boolean {
  return KNOWN_TOKEN.test(value) || PRIVATE_KEY.test(value);
}

function redactKnownTokenPatterns(value: string): string {
  if (!value) return value;
  if (PRIVATE_KEY.test(value)) return REDACTED;

  let output = value;
  const candidates = value.split(/[\s"'=:,&]+/).filter(Boolean);
  for (const candidate of candidates) {
    if (KNOWN_TOKEN.test(candidate)) output = output.split(candidate).join(REDACTED);
  }
  return output
    .replace(/\b(Bearer|Basic)\s+([^\s,;]+)/gi, (_match, scheme: string, token: string) =>
      `${scheme} ${isWholeReference(token) ? token : REDACTED}`)
    .replace(
      /((?:authorization|cookie|api[-_]?key|token|secret|password)\s*[:=]\s*)([^\s,;&]+)/gi,
      (_match, prefix: string, token: string) => `${prefix}${isWholeReference(token) ? token : REDACTED}`,
    );
}

interface JsonBudget {
  nodes: number;
}

function sanitizeJsonValue(
  value: unknown,
  key: string,
  depth: number,
  budget: JsonBudget,
): unknown {
  if (depth > MAX_EXAMPLE_JSON_DEPTH || budget.nodes >= MAX_EXAMPLE_JSON_NODES) {
    throw new Error("json budget exceeded");
  }
  budget.nodes += 1;

  if (typeof value === "string") {
    if (value.length > MAX_EXAMPLE_JSON_STRING_CHARS) throw new Error("json string exceeded");
    if (isSensitiveName(key)) return isWholeReference(value) ? value : REDACTED;
    if (containsMixedReference(value)) return REDACTED;
    return redactKnownTokenPatterns(value);
  }
  if (Array.isArray(value)) {
    return value.map((item) => sanitizeJsonValue(item, "", depth + 1, budget));
  }
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(([childKey, child]) => {
        // Placeholders are only meaningful as values. Keeping one in a key would
        // make the emitted response metadata look like a template expression.
        if (
          childKey.length > MAX_EXAMPLE_JSON_STRING_CHARS
          || containsReference(childKey)
        ) throw new Error("json key is outside the safe metadata boundary");
        return [childKey, sanitizeJsonValue(child, childKey, depth + 1, budget)];
      }),
    );
  }
  return value;
}

function sanitizeMalformedBody(value: string): string {
  if (containsMixedReference(value)) return REDACTED;
  let sensitiveAssignment = false;
  const sanitized = redactKnownTokenPatterns(value).replace(
    /(["']?[^\s"'=,:;]+["']?\s*[:=]\s*)(["']?)([^\s,;&"']+)\2/gi,
    (match, prefix: string, quote: string, rawValue: string) => {
      const key = prefix.split(/[:=]/, 1)[0].replace(/["']/g, "").trim();
      if (!isSensitiveName(key) || isWholeReference(rawValue)) return match;
      sensitiveAssignment = true;
      return `${prefix}${quote}${REDACTED}${quote}`;
    },
  );
  // A malformed value has no reliable quoting boundary. Once a sensitive
  // assignment is found, do not leave a space-delimited token suffix behind.
  return sensitiveAssignment ? REDACTED : sanitized;
}

function sanitizeBody(body: string): string | null {
  if (body.length > MAX_EXAMPLE_BODY_CHARS || BODY_UNSAFE_CONTROL_CHARACTERS.test(body)) return null;
  if (!body) return body;

  let parsed: unknown;
  try {
    parsed = JSON.parse(body) as unknown;
  } catch {
    return sanitizeMalformedBody(body);
  }

  try {
    return JSON.stringify(sanitizeJsonValue(parsed, "", 0, { nodes: 0 })) ?? REDACTED;
  } catch {
    return null;
  }
}

function hasInvalidPercentEncoding(value: string): boolean {
  return /%(?![0-9a-f]{2})/i.test(value);
}

function decodeUriComponentStrict(value: string): string | null {
  if (hasInvalidPercentEncoding(value)) return null;
  try {
    return decodeURIComponent(value);
  } catch {
    return null;
  }
}

function sanitizePath(path: string): string | null {
  if (
    !path
    || path.length > MAX_EXAMPLE_PATH_CHARS
    || path !== path.trim()
    || !path.startsWith("/")
    || path.startsWith("//")
    || path.includes("#")
    || CONTROL_CHARACTERS.test(path)
    || !URI_SAFE_CHARACTERS.test(path)
    || hasInvalidPercentEncoding(path)
  ) {
    return null;
  }

  const queryStart = path.indexOf("?");
  const pathname = queryStart < 0 ? path : path.slice(0, queryStart);
  // URL interpolation is not part of this command. A placeholder in a path or
  // query would either leak a token or stop matching the exact backend route.
  const decodedPathname = decodeUriComponentStrict(pathname);
  if (
    decodedPathname === null
    || /\s/.test(decodedPathname)
    || CONTROL_CHARACTERS.test(decodedPathname)
    || decodedPathname.startsWith("//")
    || containsReference(path)
    || containsReference(decodedPathname)
    || containsKnownToken(pathname)
    || containsKnownToken(decodedPathname)
  ) return null;
  if (queryStart < 0) return pathname;

  const query = path.slice(queryStart + 1).split("&").map((part) => {
    if (!part) return part;
    const separator = part.indexOf("=");
    const rawKey = separator < 0 ? part : part.slice(0, separator);
    const rawValue = separator < 0 ? "" : part.slice(separator + 1);
    const key = decodeUriComponentStrict(rawKey);
    const decodedValue = decodeUriComponentStrict(rawValue);
    if (key === null || decodedValue === null) throw new Error("unsafe URI encoding");
    if (
      /\s/.test(key)
      || /\s/.test(decodedValue)
      || CONTROL_CHARACTERS.test(key)
      || CONTROL_CHARACTERS.test(decodedValue)
      || containsMixedReference(rawValue)
      || containsReference(key)
      || containsReference(decodedValue)
      || containsKnownToken(key)
      || containsKnownToken(decodedValue)
    ) {
      throw new Error("unsafe URI token");
    }
    if (isSensitiveName(key) && rawValue && !isWholeReference(rawValue)) {
      // Masking a query value changes the exact route that the webhook matcher sees.
      throw new Error("sensitive query cannot be masked without changing route");
    }
    const safeValue = redactKnownTokenPatterns(rawValue);
    if (safeValue !== rawValue) throw new Error("query normalization changes route");
    return separator < 0 ? rawKey : `${rawKey}=${safeValue}`;
  }).join("&");
  return `${pathname}?${query}`;
}

interface Destination {
  host: "127.0.0.1" | "[::1]";
  port: number;
}

function normalizeAddress(address: string | null): Destination | null {
  if (!address || address.length > 64 || CONTROL_CHARACTERS.test(address) || address !== address.trim()) {
    return null;
  }

  const ipv4 = address.match(/^(127\.0\.0\.1|0\.0\.0\.0|localhost):(\d{1,5})$/i);
  if (ipv4) {
    const port = Number(ipv4[2]);
    return port >= 1 && port <= 65535 ? { host: "127.0.0.1", port } : null;
  }

  const ipv6 = address.match(/^\[(::1|::)\]:(\d{1,5})$/i);
  if (ipv6) {
    const port = Number(ipv6[2]);
    return port >= 1 && port <= 65535 ? { host: "[::1]", port } : null;
  }

  // Unbracketed IPv6 and non-loopback addresses are ambiguous or external.
  return null;
}

function concreteRequestPath(path: string): { value: string; wildcard: boolean } {
  if (!path.endsWith("*")) return { value: path, wildcard: false };
  return { value: `${path.slice(0, -1)}example`, wildcard: true };
}

function buildExampleCurlUnsafe(
  rule: ResponseRule,
  address: string | null,
  shell: CurlShell,
): string | null {
  if (
    !rule
    || typeof rule.path !== "string"
    || (rule.method !== null && typeof rule.method !== "string")
    || !Array.isArray(rule.headers)
    || typeof rule.body !== "string"
    || (shell !== "powershell" && shell !== "posix")
  ) return null;

  const destination = normalizeAddress(address);
  const safeRulePath = sanitizePath(rule.path);
  const method = rule.method === null ? "POST" : rule.method;
  const normalizedMethod = method.toUpperCase();
  if (
    !destination
    || !safeRulePath
    || method !== method.trim()
    || !/^[A-Z][A-Z0-9-]{0,15}$/.test(normalizedMethod)
    || !Number.isInteger(rule.status)
    || rule.status < MIN_EXAMPLE_STATUS
    || rule.status > MAX_EXAMPLE_STATUS
    || !Number.isInteger(rule.delayMs)
    || rule.delayMs < 0
    || rule.delayMs > MAX_EXAMPLE_DELAY_MS
  ) return null;

  const headers = sanitizeHeaders(rule.headers);
  const body = sanitizeBody(rule.body);
  if (!headers || body === null) return null;

  const requestPath = concreteRequestPath(safeRulePath);
  const url = `http://${destination.host}:${destination.port}${requestPath.value}`;
  const quote = shell === "powershell" ? powershellQuote : posixShellQuote;
  const command = shell === "powershell" ? "curl.exe" : "curl";
  const lines = [
    `${command} --globoff --path-as-is --include --request ${normalizedMethod} ${quote(url)}`,
    "",
    `# Webhook Lab response metadata (not request data): status ${rule.status}, delay ${rule.delayMs}ms`,
    "# Response headers:",
  ];
  if (headers.length === 0) lines.push("# (none)");
  else for (const [name, value] of headers) lines.push(`# ${name}: ${value}`);
  lines.push(`# Response body: ${JSON.stringify(body)}`);
  if (rule.method === null) lines.push("# Rule method is any; this example uses POST.");
  if (requestPath.wildcard) lines.push(`# Concrete trailing-* sample path: ${requestPath.value}`);

  const output = lines.join("\n");
  return output.length <= MAX_EXAMPLE_OUTPUT_CHARS ? output : null;
}

function sanitizeHeaders(headers: Array<[string, string]>): Array<[string, string]> | null {
  if (headers.length > MAX_EXAMPLE_HEADER_COUNT) return null;
  const safe: Array<[string, string]> = [];
  let totalChars = 0;
  for (const header of headers) {
    if (!Array.isArray(header) || header.length !== 2) return null;
    const [rawName, rawValue] = header;
    if (typeof rawName !== "string" || typeof rawValue !== "string") return null;
    const name = rawName.trim();
    if (
      !name
      || name.length > MAX_EXAMPLE_HEADER_NAME_CHARS
      || rawValue.length > MAX_EXAMPLE_HEADER_VALUE_CHARS
      || !HTTP_TOKEN.test(name)
      || CONTROL_CHARACTERS.test(rawValue)
    ) return null;

    totalChars += name.length + rawValue.length;
    if (totalChars > MAX_EXAMPLE_HEADER_TOTAL_CHARS) return null;

    let value: string;
    if (containsMixedReference(rawValue)) value = REDACTED;
    else if (isSensitiveName(name) && rawValue && !isWholeReference(rawValue)) value = REDACTED;
    else value = redactKnownTokenPatterns(rawValue);
    safe.push([name, value]);
  }
  return safe;
}

/**
 * Builds a runnable request that triggers the selected response rule.
 *
 * The response rule's status/headers/body/delay are emitted as safe comments;
 * they are response metadata, not request headers or request body. `--include`
 * makes curl print the response returned by the local server. A trailing `*`
 * rule gets a concrete `/example` suffix so the sample follows the backend's
 * prefix matcher instead of asking curl to expand a glob.
 */
export function buildExampleCurl(
  rule: ResponseRule,
  address: string | null,
  shell: CurlShell = "posix",
): string | null {
  try {
    return buildExampleCurlUnsafe(rule, address, shell);
  } catch {
    // Builder failures must never put parser/URI/path details into the UI.
    return null;
  }
}
