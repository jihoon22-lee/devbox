/**
 * 저장 대화상자의 [비밀로 옮기기]: 직접 입력한 값을 이 PC 비밀로 저장하고 필드를 `{{secret:NAME}}`로 바꾼다.
 * 이름은 `요청이름_필드이름`(대문자·`_`), 이미 있는 비밀은 덮지 않고 `_2`, `_3`…을 붙인다.
 * 행 값과 인증 필드만 옮긴다(URL·본문 안 일부 값은 통째로 바꿀 수 없어 가린 채 저장한다).
 */
import type { ApiBackend } from "./backend";
import type { AuthDrafts, RequestDraft, Row } from "./model/draft";

const ROW = /^request\.(query|headers|cookies|body\.fields|body\.parts)\.(\d+)\.value$/;
const AUTH = /^request\.auth\.(\w+)$/;

type ListKey = "query" | "headers" | "cookies" | "fields" | "parts";

function listOf(draft: RequestDraft, key: ListKey): (Row | { name: string; value: string })[] {
  if (key === "fields") return draft.bodies.form?.fields ?? [];
  if (key === "parts") return draft.bodies.multipart?.parts ?? [];
  return draft[key];
}

function withList(draft: RequestDraft, key: ListKey, index: number, value: string): RequestDraft {
  const patch = <T extends { value: string }>(rows: T[]) => rows.map((r, i) => (i === index ? { ...r, value } : r));
  if (key === "fields" && draft.bodies.form)
    return { ...draft, bodies: { ...draft.bodies, form: { fields: patch(draft.bodies.form.fields) } } };
  if (key === "parts" && draft.bodies.multipart)
    return { ...draft, bodies: { ...draft.bodies, multipart: { parts: patch(draft.bodies.multipart.parts) } } };
  if (key === "query" || key === "headers" || key === "cookies") return { ...draft, [key]: patch(draft[key]) };
  return draft;
}

export function secretBase(requestName: string, field: string): string {
  const part = (text: string) =>
    text
      .toUpperCase()
      .replace(/[^A-Z0-9]+/g, "_")
      .replace(/^_+|_+$/g, "");
  const name = [part(requestName) || "API", part(field) || "VALUE"].join("_").slice(0, 120);
  // 비밀 이름 규칙(`[A-Za-z_][A-Za-z0-9_]*`): 숫자로 시작하면 `_`를 붙인다.
  return /^[0-9]/.test(name) ? `_${name}` : name;
}

export function uniqueSecretName(base: string, taken: Set<string>): string {
  let name = base;
  for (let n = 2; taken.has(name); n++) name = `${base}_${n}`;
  taken.add(name);
  return name;
}

interface Movable {
  path: string;
  field: string;
  value: string;
  apply(draft: RequestDraft, reference: string): RequestDraft;
}

function movable(draft: RequestDraft, path: string): Movable | null {
  const row = ROW.exec(path);
  if (row) {
    const key = (row[1] === "body.fields" ? "fields" : row[1] === "body.parts" ? "parts" : row[1]) as ListKey;
    const index = Number(row[2]);
    const item = listOf(draft, key)[index];
    if (!item?.value) return null;
    return { path, field: item.name, value: item.value, apply: (d, ref) => withList(d, key, index, ref) };
  }
  const auth = AUTH.exec(path);
  const kind = draft.authKind;
  if (auth && kind !== "none") {
    const fields = draft.auths[kind] as Record<string, string> | undefined;
    const field = auth[1]!;
    const value = fields?.[field];
    if (!value) return null;
    return {
      path,
      field,
      value,
      apply: (d, ref) => ({
        ...d,
        auths: { ...d.auths, [kind]: { ...(d.auths[kind] as AuthDrafts[typeof kind]), [field]: ref } },
      }),
    };
  }
  return null;
}

/**
 * 중간에 비밀 저장이 실패함. `created`는 이미 만든 비밀 이름, `draft`는 그 비밀들만 참조로 바꾼 초안이다.
 * 비밀 저장에 "없을 때만" 조건이 없어(protocol 동결) 목록 조회와 저장 사이의 같은 이름 경쟁은 막지 못한다(후속).
 */
export interface SecretMove {
  path: string;
  value: string;
  reference: string;
}

/**
 * 옮긴 참조를 지금 초안에 반영한다. 그 위치의 값이 옮길 때와 같을 때만 바꾸므로
 * 옮기는 동안 사용자가 고친 다른 내용(또는 그 값 자체)을 덮지 않는다.
 */
export function applyMoves(draft: RequestDraft, moves: readonly SecretMove[]): RequestDraft {
  let next = draft;
  for (const move of moves) {
    const target = movable(next, move.path);
    if (target && target.value === move.value) next = target.apply(next, move.reference);
  }
  return next;
}

export class MoveSecretsError extends Error {
  constructor(
    readonly created: string[],
    readonly moves: SecretMove[],
    readonly cause: unknown,
  ) {
    super("move_secrets_failed");
    this.name = "MoveSecretsError";
  }
}

/** 옮긴 뒤의 초안과 옮긴 위치. 비밀 저장이 실패하면 `MoveSecretsError`로 이미 만든 비밀을 알린다. */
export async function moveToSecrets(
  backend: Pick<ApiBackend, "secretNames" | "setSecret">,
  draft: RequestDraft,
  requestName: string,
  paths: readonly string[],
): Promise<{ draft: RequestDraft; moved: string[]; moves: SecretMove[] }> {
  const taken = new Set(await backend.secretNames());
  let next = draft;
  const moved: string[] = [];
  const created: string[] = [];
  const moves: SecretMove[] = [];
  for (const path of paths) {
    const target = movable(next, path);
    if (!target) continue;
    const name = uniqueSecretName(secretBase(requestName, target.field), taken);
    try {
      await backend.setSecret(name, target.value);
    } catch (cause) {
      throw new MoveSecretsError(created, moves, cause);
    }
    const reference = `{{secret:${name}}}`;
    next = target.apply(next, reference);
    moved.push(path);
    created.push(name);
    moves.push({ path, value: target.value, reference });
  }
  return { draft: next, moved, moves };
}
