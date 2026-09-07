import type { RequestCookie, RequestHeader } from "../types";
import { isHeaderEnabled } from "./headers";
import { isExactVariableReference } from "./references";

export const MAX_REQUEST_COOKIE_ROWS = 100;

const COOKIE_NAME = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
const COOKIE_VALUE = /^[\x21\x23-\x2B\x2D-\x3A\x3C-\x5B\x5D-\x7E]*$/;

export interface CookieValidationIssue {
  index: number;
  message: string;
}

export function isCookieEnabled(cookie: RequestCookie): boolean {
  return cookie.enabled !== false;
}

export function isRequestCookie(value: unknown): value is RequestCookie {
  if (!value || typeof value !== "object") return false;
  const cookie = value as Partial<RequestCookie>;
  return (
    typeof cookie.name === "string" &&
    typeof cookie.value === "string" &&
    (cookie.enabled === undefined || typeof cookie.enabled === "boolean")
  );
}

export function normalizeCookies(cookies: readonly RequestCookie[] | undefined): RequestCookie[] {
  return (cookies ?? []).slice(0, MAX_REQUEST_COOKIE_ROWS).map((cookie) => ({
    name: cookie.name,
    value: cookie.value,
    enabled: isCookieEnabled(cookie),
  }));
}

export function addCookie(cookies: readonly RequestCookie[]): RequestCookie[] {
  if (cookies.length >= MAX_REQUEST_COOKIE_ROWS) return normalizeCookies(cookies);
  return [...normalizeCookies(cookies), { name: "", value: "", enabled: true }];
}

export function updateCookie(
  cookies: readonly RequestCookie[],
  index: number,
  patch: Partial<RequestCookie>,
): RequestCookie[] {
  return normalizeCookies(cookies).map((cookie, candidate) =>
    candidate === index ? { ...cookie, ...patch } : cookie
  );
}

export function removeCookie(cookies: readonly RequestCookie[], index: number): RequestCookie[] {
  return normalizeCookies(cookies).filter((_, candidate) => candidate !== index);
}

export function duplicateCookie(cookies: readonly RequestCookie[], index: number): RequestCookie[] {
  const normalized = normalizeCookies(cookies);
  if (normalized.length >= MAX_REQUEST_COOKIE_ROWS || !normalized[index]) return normalized;
  return [
    ...normalized.slice(0, index + 1),
    { ...normalized[index] },
    ...normalized.slice(index + 1),
  ];
}

export function validateCookies(cookies: readonly RequestCookie[]): CookieValidationIssue[] {
  const issues: CookieValidationIssue[] = [];
  if (cookies.length > MAX_REQUEST_COOKIE_ROWS) {
    issues.push({
      index: MAX_REQUEST_COOKIE_ROWS,
      message: "Cookie는 최대 100행까지 사용할 수 있습니다.",
    });
  }
  normalizeCookies(cookies).forEach((cookie, index) => {
    if (!isCookieEnabled(cookie) || (!cookie.name && !cookie.value)) return;
    if (!cookie.name) {
      issues.push({ index, message: "이름이 필요합니다." });
    } else if (!COOKIE_NAME.test(cookie.name)) {
      issues.push({ index, message: "이름에 Cookie token으로 쓸 수 없는 문자가 있습니다." });
    }
    if (!COOKIE_VALUE.test(cookie.value)) {
      issues.push({ index, message: "값에 공백, 세미콜론, 따옴표 또는 제어 문자를 사용할 수 없습니다." });
    }
  });
  return issues;
}

export function hasActiveCookies(cookies: readonly RequestCookie[]): boolean {
  return normalizeCookies(cookies).some(
    (cookie) => isCookieEnabled(cookie) && Boolean(cookie.name || cookie.value),
  );
}

export function hasActiveCookieHeader(headers: readonly RequestHeader[]): boolean {
  return headers.some(
    (header) => isHeaderEnabled(header) && header.key.trim().toLowerCase() === "cookie",
  );
}

export function hasCookieSourceConflict(
  cookies: readonly RequestCookie[],
  headers: readonly RequestHeader[],
): boolean {
  return hasActiveCookies(cookies) && hasActiveCookieHeader(headers);
}

/** 유효한 활성 행을 RFC Cookie request header 값으로 조립한다. */
export function buildCookieHeader(
  cookies: readonly RequestCookie[],
  maskDirectValues = false,
): string {
  return normalizeCookies(cookies)
    .filter((cookie) =>
      isCookieEnabled(cookie) &&
      Boolean(cookie.name) &&
      COOKIE_NAME.test(cookie.name) &&
      COOKIE_VALUE.test(cookie.value)
    )
    .map((cookie) => `${cookie.name}=${maskDirectValues ? maskCookieValue(cookie.value) : cookie.value}`)
    .join("; ");
}

export function maskCookieValue(value: string): string {
  if (!value || isExactVariableReference(value)) return value;
  return "[REDACTED]";
}

export function cookieSecretReference(name: string): string {
  return /^[a-zA-Z0-9_.-]+$/.test(name) ? `\${${name}}` : "";
}
