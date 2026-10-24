import type { RpcClient } from "../../rpc/client";
import type { LogSource } from "../../rpc/gen/rpc";
import { validCodeKey } from "./editor/location";
export async function resolveSourceLocation(
  client: Pick<RpcClient, "call">,
  projectId: string,
  location: { path: string; line: number; column?: number; source: LogSource },
) {
  const resolved = await client.call("observe.resolve_location", {
    projectId,
    ...location,
    column: location.column ?? null,
  });
  const projects = await client.call("projects.list", {});
  const root = projects.items
    .filter((p) => resolved.path.startsWith(`${p.path.replace(/\/$/, "")}/`))
    .sort((a, b) => b.path.length - a.path.length)[0];
  if (!root) throw new Error("프로젝트 밖의 파일입니다. 외부 편집기로 열어 주세요.");
  const key = { root: `project:${root.id}`, path: resolved.path.slice(root.path.replace(/\/$/, "").length + 1) };
  if (!validCodeKey(key)) throw new Error("파일 경로가 올바르지 않습니다.");
  const stat = await client.call("files.stat", { key });
  if (stat.kind !== "file") throw new Error("텍스트 파일 위치가 아닙니다.");
  return { key: stat.key, location: { line: resolved.line, column: resolved.column } };
}
