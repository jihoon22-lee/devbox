/**
 * 컬렉션 요청 파일 형식(07b S5-3): `{ version: 1, name, request, assertions, captures, requiresSecretReview?, masked? }`.
 * 모르는 최상위·`request` 하위 필드는 읽은 그대로 두었다가 쓸 때 다시 넣는다(`{ ...known, ...unknown }`).
 * 쓰기는 항상 `sanitizeForStorage`를 지난다. 데몬 재검토 판정과의 계약은 `reviewContract.test.ts`.
 */
import type { CollectionRoot } from "../../../../rpc/gen/rpc";
import { type Assertion, cleanAssertions } from "../lib/assertions";
import { type Capture, cleanCaptures } from "../lib/captures";
import { looksLikeSecret } from "../lib/secretPatterns";
import { REDACTED, sanitizeForStorage, valueAt } from "../lib/storage";
import { normalizeStoredRequest, type StoredRequest } from "./stored";

export const MAX_REQUEST_FILE_BYTES = 1024 * 1024;
export const REQUEST_FILE_SUFFIX = ".request.json";
const MAX_NAME_CHARS = 256;
const MAX_MASKED = 500;
const MAX_DEPTH = 9;
const TOP_KEYS = ["version", "name", "request", "assertions", "captures", "requiresSecretReview", "masked"];
const REQUEST_KEYS = [
  "method",
  "url",
  "query",
  "headers",
  "cookies",
  "body",
  "auth",
  "tls",
  "timeoutMs",
  "followRedirects",
];

export interface RequestFile {
  version: 1;
  name: string;
  request: StoredRequest;
  assertions: Assertion[];
  captures: Capture[];
  /** 가져오기 등이 표시한 검토 필요. 데몬 digest 검토와 별개로 실행기가 건너뛴다. */
  requiresSecretReview?: boolean;
  /** 저장 때 가린 필드 위치(파일 기준 경로, 예 `request.headers.0.value`). 송신 차단은 이 위치로만 판정한다. */
  masked?: string[];
}

export interface ParsedRequestFile {
  file: RequestFile;
  /** 읽은 JSON 원본(모르는 필드 보존용). */
  raw: Record<string, unknown>;
  /** 버린 assertion·캡처 수. */
  dropped: number;
}

export class RequestFileError extends Error {}
const invalid = (): never => {
  throw new RequestFileError("요청 파일 형식이 올바르지 않습니다");
};
const bytes = (text: string) => new TextEncoder().encode(text).length;
const isObject = (value: unknown): value is Record<string, unknown> =>
  !!value && typeof value === "object" && !Array.isArray(value);

export function parseRequestFile(text: string): ParsedRequestFile {
  if (bytes(text) > MAX_REQUEST_FILE_BYTES) invalid();
  let raw: unknown;
  try {
    raw = JSON.parse(text.replace(/^﻿/, ""));
  } catch {
    return invalid();
  }
  if (!isObject(raw) || raw.version !== 1 || typeof raw.name !== "string" || raw.name.length > MAX_NAME_CHARS)
    return invalid();
  let request: StoredRequest;
  try {
    request = normalizeStoredRequest(raw.request);
  } catch {
    return invalid();
  }
  const assertions = cleanAssertions(raw.assertions);
  const captures = cleanCaptures(raw.captures);
  const masked = Array.isArray(raw.masked)
    ? raw.masked.filter((path): path is string => typeof path === "string" && /^(request|assertions)\./.test(path))
    : [];
  const file: RequestFile = {
    version: 1,
    name: raw.name,
    request,
    assertions: assertions.assertions,
    captures: captures.captures,
    ...(raw.requiresSecretReview === true ? { requiresSecretReview: true } : {}),
    ...(masked.length ? { masked: masked.slice(0, MAX_MASKED) } : {}),
  };
  return { file, raw, dropped: assertions.dropped + captures.dropped };
}

function unknownFields(raw: unknown, known: readonly string[]): Record<string, unknown> {
  if (!isObject(raw)) return {};
  return Object.fromEntries(Object.entries(raw).filter(([key]) => !known.includes(key)));
}

/** 정화한 파일 텍스트와 이번 저장에서 새로 가린 위치(저장 전 안내용). */
export function serializeRequestFile(
  file: RequestFile,
  root: CollectionRoot,
  raw?: Record<string, unknown>,
): { text: string; masked: string[] } {
  const clean = sanitizeForStorage(file.request, root);
  const masked = clean.masked.map((path) => `request.${path}`);
  const assertions = file.assertions.map((assertion, index) => {
    if (!looksLikeSecret(assertion.expected)) return assertion;
    masked.push(`assertions.${index}.expected`);
    return { ...assertion, expected: REDACTED };
  });
  const known = {
    version: 1,
    name: file.name,
    request: { ...clean.stored, ...unknownFields(raw?.request, REQUEST_KEYS) },
    assertions,
    captures: file.captures,
  };
  // 이전에 가린 위치는 값을 다시 입력하기 전까지 남긴다(다시 열어도 송신을 막음).
  const kept = (file.masked ?? []).filter((path) => valueAt(known, path)?.includes("REDACTED"));
  const recorded = [...new Set([...kept, ...masked])].slice(0, MAX_MASKED);
  const out = {
    ...known,
    ...(file.requiresSecretReview ? { requiresSecretReview: true } : {}),
    ...(recorded.length ? { masked: recorded } : {}),
    ...unknownFields(raw, TOP_KEYS),
  };
  const text = `${JSON.stringify(out, null, 2)}\n`;
  if (bytes(text) > MAX_REQUEST_FILE_BYTES) invalid();
  return { text, masked };
}

const RESERVED = /^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)/i;
const FORBIDDEN = /[<>:"/\\|?*\u0000-\u001f\u007f]/g;

/** Windows에서도 쓸 수 있는 파일 이름 조각(v0.9.0 `safeFileName`, 80자). */
export function safeFileName(name: string): string {
  let value = name
    .normalize("NFC")
    .replace(FORBIDDEN, "-")
    .trim()
    .slice(0, 80)
    .replace(/[. ]+$/, "");
  if (RESERVED.test(value)) value = `_${value}`.slice(0, 80);
  return value || "untitled";
}

/** `folder/이름.request.json`. `used`(소문자 경로)와 겹치지 않게 ` (2)`를 붙이고 `used`에 더한다. */
export function requestFilePath(name: string, folder: string, used: Set<string>): string {
  const folders = folder ? folder.split("/").filter(Boolean).map(safeFileName) : [];
  if (folders.length >= MAX_DEPTH) invalid();
  const base = Array.from(safeFileName(name));
  for (let sequence = 1; ; sequence++) {
    const suffix = `${sequence > 1 ? ` (${sequence})` : ""}${REQUEST_FILE_SUFFIX}`;
    const characters = [...base];
    while (bytes(characters.join("") + suffix) > 255) characters.pop();
    const path = [...folders, characters.join("") + suffix].join("/");
    if (!used.has(path.toLowerCase())) {
      used.add(path.toLowerCase());
      return path;
    }
  }
}

/** 컬렉션 루트 기준 상대 경로 검사(데몬 검사보다 먼저, Windows 금지 이름·글자 포함). */
export function isSafeCollectionPath(path: string): boolean {
  const segments = path.split("/");
  if (segments.length > MAX_DEPTH + 1 || !path.endsWith(".json")) return false;
  return segments.every(
    (segment) =>
      segment !== "" &&
      segment !== "." &&
      segment !== ".." &&
      !new RegExp(FORBIDDEN.source).test(segment) &&
      !/[. ]$/.test(segment) &&
      !RESERVED.test(segment) &&
      bytes(segment) <= 255,
  );
}
