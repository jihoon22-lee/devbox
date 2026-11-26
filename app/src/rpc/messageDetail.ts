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
/** `text` as one word of a shell command the user copies: in single quotes, each `'` as `'\''`. */
export const shellQuote = (text: string) => `'${text.replaceAll("'", "'\\''")}'`;
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
/** The tool's label from a `work_folder_required` detail. */
const toolLabel = (d: unknown) => {
  const value = field(d, "tool");
  return typeof value === "string" && value ? value : "이 도구";
};
/** A new task of a tool that keeps its connection beside the work folder, started without one. */
export const workFolderRequired = (d: unknown) => ({
  title: `${toolLabel(d)}${toolLabel(d) === "이 도구" ? "의" : ""} 작업에는 작업 폴더가 필요합니다.`,
  action: "[작업 폴더 만들기]를 켜고 다시 시작하세요.",
});
/** A first instruction devbox would paste into the tool, which the tool would run as a command. */
export const firstInstructionCommand = (d: unknown) => ({
  title: `\`!\`나 \`/\`로 시작하는 지시는 ${toolLabel(d)}가 셸 명령이나 도구 명령으로 실행할 수 있어 첫 지시로 보내지 않습니다.`,
  action: "앞에 다른 말을 붙이거나, 작업을 만든 뒤 터미널에서 직접 입력하세요.",
});
/** A prompt a `promptArg` tool gets as an argument of its own, which it could read as an option. */
export const promptOption = (d: unknown) => ({
  title: `\`-\`로 시작하는 지시는 ${toolLabel(d)}가 옵션으로 읽을 수 있어 인자로 넘기지 않습니다.`,
  action:
    '앞에 다른 말을 붙여 다시 시작하세요. 도구가 받는다면 도구 프로필의 promptArg를 "--prompt="처럼 =로 끝나게 적거나 "--"로 바꿔도 됩니다.',
});
/** A folder of the hooks folder's name beside the work folder that is not this task's. */
export const hooksFolderExists = (d: unknown) => ({
  title: `작업 폴더 옆에 이 작업의 것이 아닌 폴더가 이미 있어 시작하지 못했습니다: ${path(d)}`,
  action: "그 폴더를 지우거나 이름을 바꾼 뒤 다시 시도하세요.",
});
/** The same for an agent entry of a project's start configuration. */
export const launchWorkFolderRequired = (d: unknown) => ({
  title: `시작 구성의 ${toolLabel(d)} 에이전트에는 작업 폴더가 필요합니다.`,
  action: ".devbox/devbox.toml의 [launch] agents에서 그 항목에 worktree = true를 적은 뒤 다시 시작하세요.",
});
export const check = "입력란을 확인해 주세요.";
export const configFile = (d: unknown) => {
  const file = field(d, "file");
  return typeof file === "string" && file ? file : "config.toml";
};
/** Settings are read when the daemon starts, so a fixed file applies after a restart. */
export const restart = "설정 → 앱 진단 → [다시 설정]을 눌러 주세요.";
