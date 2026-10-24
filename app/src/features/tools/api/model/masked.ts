/**
 * 가린 위치(`masked`)를 초안의 행 id로 기억한다. 파일은 행 번호 경로(`request.headers.2.value`)를 쓰지만
 * 편집 중 행을 더하거나 지우면 번호가 바뀌므로, 읽을 때 id로 바꾸고 저장·판정할 때 현재 번호로 되돌린다.
 */
import { valueAt } from "../lib/storage";
import type { RequestDraft } from "./draft";
import { toStored } from "./stored";

type Lists = Record<string, { id: string }[]>;

function lists(draft: RequestDraft): Lists {
  return {
    "request.query": draft.query,
    "request.headers": draft.headers,
    "request.cookies": draft.cookies,
    "request.body.fields": draft.bodyKind === "form" ? (draft.bodies.form?.fields ?? []) : [],
    "request.body.parts": draft.bodyKind === "multipart" ? (draft.bodies.multipart?.parts ?? []) : [],
    assertions: draft.assertions,
  };
}

const LIST_PATH = /^(request\.(?:query|headers|cookies|body\.fields|body\.parts)|assertions)\.(\d+|#[^.]+)\.(.+)$/;

/** 파일 경로를 id 경로로 바꾼다(그 번호의 행이 없으면 버린다). */
export function draftMasks(draft: RequestDraft, paths: readonly string[]): string[] {
  const all = lists(draft);
  return paths.flatMap((path) => {
    const match = LIST_PATH.exec(path);
    if (!match) return [path];
    const [, list, at, field] = match;
    if (at!.startsWith("#")) return [path];
    const item = all[list!]?.[Number(at)];
    return item ? [`${list}.#${item.id}.${field}`] : [];
  });
}

/** 현재 행 번호로 되돌린 파일 경로. 지운 행의 위치는 빠진다. */
export function maskedPaths(draft: RequestDraft): string[] {
  const all = lists(draft);
  return draft.masked.flatMap((path) => {
    const match = LIST_PATH.exec(path);
    if (!match) return [path];
    const [, list, at, field] = match;
    if (!at!.startsWith("#")) return [];
    const index = all[list!]?.findIndex((item) => item.id === at!.slice(1)) ?? -1;
    return index < 0 ? [] : [`${list}.${index}.${field}`];
  });
}

/** 기록한 가린 위치 중 아직 가린 값이 남은 곳(송신·실행 차단). 다른 곳의 `[REDACTED]` 글자는 보지 않는다. */
export function redactedFields(draft: RequestDraft): string[] {
  const file = { request: toStored(draft), assertions: draft.assertions };
  return maskedPaths(draft).filter((path) => valueAt(file, path)?.includes("REDACTED"));
}
