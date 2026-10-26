/**
 * 편집기 보내기: 실행기와 같은 규칙으로 변수를 고르고(이름당 하나, 세션 캡처 값이 이기고 `capture` 출처),
 * 파일에서 가린 값이 남았으면 보내지 않는다. 응답에는 만든 요청의 출처(AR-F08)를 붙인다.
 */
import type { CollectionRoot, EnvVar, Pair, RequestOrigin, SendRequest } from "../../../rpc/gen/rpc";
import { isSensitiveName, looksLikeSecret } from "./lib/secretPatterns";
import { type RequestDraft, toSnapshot } from "./model/draft";
import { redactedFields } from "./model/masked";
import { variableRefs } from "./runner/plan";
import type { Session } from "./runner/session";

export interface ResponseOriginInfo {
  name: string;
  method: string;
  url: string;
  sentAtMs: number;
  snapshotHash: string;
}

export const MASKED_BLOCK = "가린 값을 다시 입력해 주세요";

export function snapshotHash(draft: RequestDraft): string {
  return JSON.stringify(toSnapshot(draft));
}

export function originOf(name: string, draft: RequestDraft, sentAtMs: number): ResponseOriginInfo {
  return { name, method: draft.method, url: draft.url, sentAtMs, snapshotHash: snapshotHash(draft) };
}

export function editedSince(origin: ResponseOriginInfo, draft: RequestDraft): boolean {
  return origin.snapshotHash !== snapshotHash(draft);
}

/** 쓰는 변수만, 이름당 하나. 데몬은 출처가 섞인 같은 이름을 거절한다(`mixed_origin`). */
export function sendVariables(draft: RequestDraft, environment: Pair[], session: Session): EnvVar[] {
  const out: EnvVar[] = [];
  for (const { name } of variableRefs(toSnapshot(draft))) {
    const captured = session.get(name);
    if (captured) {
      out.push({ name, value: captured.value, sensitive: true, origin: "capture" });
      continue;
    }
    if (session.isDiscarded(name)) continue;
    const env = environment.findLast((pair) => pair.name === name);
    if (env)
      out.push({
        name,
        value: env.value,
        sensitive: isSensitiveName(name) || looksLikeSecret(env.value) || env.value.includes("{{secret:"),
        origin: "environment",
      });
  }
  return out;
}

export function prepareSend(
  draft: RequestDraft,
  environment: Pair[],
  session: Session,
  origin: RequestOrigin | null,
  sourceRoot: CollectionRoot = origin?.root ?? { kind: "personal" },
): { blocked: string } | { request: SendRequest } {
  if (redactedFields(draft).length) return { blocked: MASKED_BLOCK };
  return {
    request: {
      snapshot: toSnapshot(draft),
      env: { variables: sendVariables(draft, environment, session) },
      runId: null,
      origin,
      sourceRoot,
      cookieJar: origin ? `${origin.root.kind === "project" ? origin.root.projectId : "personal"}` : null,
    },
  };
}
