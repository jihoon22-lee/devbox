import { isProjectContext, type ProjectContext } from "@devbox/product-shell/api";
import type { RuntimeLogOpenRequest } from "@devbox/workspace-features/logs";

function object(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
function keys(value: Record<string, unknown>, expected: string[]): boolean {
  return Object.keys(value).sort().join(",") === [...expected].sort().join(",");
}
export function sameRuntimeContext(value: unknown, current: ProjectContext | null): boolean {
  if (current === null) return value === null;
  return isProjectContext(value) && value.projectId === current.projectId
    && value.worktreeId === current.worktreeId && value.revision === current.revision
    && value.target.kind === current.target.kind
    && (value.target.kind !== "wsl" || (current.target.kind === "wsl" && value.target.distroId === current.target.distroId));
}
/** Late native notifications cannot change a new project or a different view. */
export function runtimeDestination(value: unknown, context: ProjectContext | null, route: string): "tasks" | "overview" | null {
  if (!object(value) || !keys(value, ["route", "context", "fromRoute"])
    || value.fromRoute !== "runtime" || route !== value.fromRoute
    || !sameRuntimeContext(value.context, context)) return null;
  return value.route === "tasks" || value.route === "overview" ? value.route : null;
}
export function runtimeLogRequest(value: unknown, context: ProjectContext | null, route: string): RuntimeLogOpenRequest | null {
  if (!object(value) || !keys(value, ["id", "source", "context", "fromRoute"])
    || !["tasks", "runtime"].includes(String(value.fromRoute)) || route !== value.fromRoute
    || !sameRuntimeContext(value.context, context)
    || typeof value.id !== "string" || !/^[a-f0-9]{32}$/.test(value.id)) return null;
  const source = value.source;
  if (!object(source) || !keys(source, ["kind", "runId", "stream", "revision"])
    || source.kind !== "runtimeRun" || typeof source.runId !== "string"
    || !/^[A-Za-z0-9_-]{1,128}$/.test(source.runId)
    || (source.stream !== "stdout" && source.stream !== "stderr")
    || typeof source.revision !== "string" || !/^[a-f0-9]{64}$/.test(source.revision)) return null;
  return { id: value.id, source: { kind: "runtimeRun", runId: source.runId, stream: source.stream, revision: source.revision } };
}

export function runtimeDiagnostic(value: unknown, context: ProjectContext | null, route: string): {id: string; relativePath: string; line: number; column: number | null} | null {
  if (!object(value) || !keys(value,["id","relativePath","line","column","runId","revision","context","fromRoute"])
    || route !== "tasks" || value.fromRoute !== "tasks" || !context || !sameRuntimeContext(value.context,context)
    || typeof value.id !== "string" || !/^[a-f0-9]{32}$/.test(value.id)
    || typeof value.runId !== "string" || !/^[A-Za-z0-9_-]{1,128}$/.test(value.runId)
    || typeof value.revision !== "string" || !/^[a-f0-9]{64}$/.test(value.revision)
    || typeof value.relativePath !== "string" || value.relativePath.length > 32768 || /[\\:\x00-\x1f\x7f]/.test(value.relativePath)
    || value.relativePath.split("/").some(part => !part || part === "." || part === "..")
    || !Number.isSafeInteger(value.line) || (value.line as number) < 1
    || (value.column !== null && (!Number.isSafeInteger(value.column) || (value.column as number) < 1))) return null;
  return {id:value.id,relativePath:value.relativePath,line:value.line as number,column:value.column as number | null};
}
