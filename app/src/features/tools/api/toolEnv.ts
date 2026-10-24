/**
 * gRPC·MCP 송신의 변수 규칙: 대상·헤더(또는 stdio env)·TLS·본문을 편집기 초안 모양으로 만들어 편집기 보내기와
 * 같은 변수(`sendVariables`)를 고른다. 검토하지 않은 프로젝트 환경의 값을 쓰게 되면 보내지 않는다(ENV_REVIEW_BLOCK).
 */
import type { EnvSnapshot, RequestSnapshot, TlsOptions } from "../../../rpc/gen/rpc";
import { emptyDraft, type Row, toSnapshot } from "./model/draft";
import { sendVariables } from "./sendState";
import { useApiTabs } from "./store";
import { ENV_REVIEW_BLOCK } from "./useSend";

export function toolEnv(input: {
  url: string;
  headers: Row[];
  tls: TlsOptions;
  json?: string;
}): { blocked: string } | { snapshot: RequestSnapshot; env: EnvSnapshot } {
  const draft = {
    ...emptyDraft(),
    url: input.url,
    headers: input.headers,
    tls: input.tls,
    ...(input.json === undefined ? {} : { bodyKind: "json" as const, bodies: { json: { text: input.json } } }),
  };
  const { environment, session } = useApiTabs.getState();
  const variables = sendVariables(draft, environment?.variables ?? [], session);
  if (environment?.unreviewed && variables.some((v) => v.origin === "environment"))
    return { blocked: ENV_REVIEW_BLOCK };
  return { snapshot: toSnapshot(draft), env: { variables } };
}
