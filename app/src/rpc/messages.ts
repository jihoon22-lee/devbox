import { AGENT_MESSAGES } from "./agentMessages";
import { API_MESSAGES } from "./apiMessages";
import type { ErrorCode } from "./client";
import type { RpcErrorCode } from "./gen/rpc";
import { KNOWLEDGE_MESSAGES } from "./knowledgeMessages";
import type { Entry, Message } from "./messageDetail";
import { SOURCE_MESSAGES } from "./sourceMessages";
import { SYSTEM_MESSAGES } from "./systemMessages";

export type { Message } from "./messageDetail";

/** Codes any domain can return. They have no domain prefix. */
const GENERAL: Record<Exclude<RpcErrorCode, `${string}.${string}`>, Entry> = {
  internal: { title: "예상하지 못한 문제가 생겼습니다.", action: "진단 ID를 복사해 두면 원인을 찾을 수 있습니다." },
  invalid_params: { title: "요청 형식이 맞지 않습니다.", action: "앱과 데몬 버전을 확인해 주세요." },
  method_not_found: { title: "이 기능을 사용할 수 없습니다.", action: "앱을 다시 시작해 주세요." },
  hello_required: { title: "연결 준비가 끝나지 않았습니다.", action: "잠시 뒤 다시 시도해 주세요." },
  cancelled: { title: "요청이 취소되었습니다." },
  daemon_restarting: { title: "다시 시작하는 중이라 실행하지 않았습니다.", action: "잠시 뒤 다시 시도해 주세요." },
  not_found: { title: "대상을 찾을 수 없습니다." },
  secrets_locked: { title: "비밀 값을 아직 열지 못했습니다.", action: "앱을 연 상태에서 다시 시도해 주세요." },
};
const TABLE: Record<RpcErrorCode, Entry> = {
  ...GENERAL,
  ...AGENT_MESSAGES,
  ...API_MESSAGES,
  ...KNOWLEDGE_MESSAGES,
  ...SOURCE_MESSAGES,
  ...SYSTEM_MESSAGES,
};
export function messageFor(code: ErrorCode, detail?: unknown): Message {
  if (code === "connection_lost")
    return {
      title: "연결이 끊겨 실행 결과를 확인하지 못했습니다.",
      action: "다시 연결되면 결과를 확인한 뒤 재시도해 주세요.",
    };
  const entry = TABLE[code];
  return typeof entry === "function" ? entry(detail) : entry;
}
