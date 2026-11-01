import type { RpcErrorCode } from "./gen/rpc";

export interface Message {
  title: string;
  action?: string;
}
/** A code's message, or how to build it from the error's detail. */
export type Entry = Message | ((detail: unknown) => Message);
/** The message of every generated code in the given domains. */
export type Messages<Domain extends string> = Record<Extract<RpcErrorCode, `${Domain}.${string}`>, Entry>;
export const path = (d: unknown) =>
  d && typeof d === "object" && "path" in d && typeof d.path === "string" ? d.path : "";
export const files = (detail: unknown) =>
  detail && typeof detail === "object" && "files" in detail && Array.isArray(detail.files)
    ? detail.files.filter((file): file is string => typeof file === "string").join(", ")
    : "";
export const cloneMessage = (detail: unknown) => {
  if (!detail || typeof detail !== "object" || !("message" in detail) || typeof detail.message !== "string") return "";
  return ` ${detail.message
    .replace(/([a-z]+:\/\/)[^\s/]*@/gi, "$1[인증 정보 숨김]@")
    .replace(/([?&](?:token|password|access_token)=)[^&\s]+/gi, "$1[숨김]")}`;
};
export const field = (d: unknown, key: string) =>
  d && typeof d === "object" && key in d ? (d as Record<string, unknown>)[key] : undefined;
export const row = (d: unknown) =>
  typeof field(d, "index") === "number" ? `${(field(d, "index") as number) + 1}행 ` : "";
export const names = (d: unknown) => {
  const list = field(d, "names");
  return Array.isArray(list) ? list.filter((n) => typeof n === "string").join(", ") : "";
};
export const command = (d: unknown) => {
  const value = field(d, "command");
  return typeof value === "string" && value ? value : "에이전트 도구";
};
export const toolMissing = (d: unknown) => ({
  title: `${command(d)} 명령을 찾을 수 없습니다.`,
  action: "설정 → 앱 진단 → 환경 점검에서 설치 방법을 확인하세요.",
});
export const check = "입력란을 확인해 주세요.";
export const configFile = (d: unknown) => {
  const file = field(d, "file");
  return typeof file === "string" && file ? file : "config.toml";
};
/** Settings are read when the daemon starts, so a fixed file applies after a restart. */
export const restart = "설정 → 앱 진단 → [다시 설정]을 눌러 주세요.";
