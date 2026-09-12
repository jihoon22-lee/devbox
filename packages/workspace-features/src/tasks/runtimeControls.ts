import { productInstallationId, WorkspaceOperationError } from "../transport";

type Call = <T>(method: string, args?: Record<string, unknown>) => Promise<T>;
interface Pending { operationId: string; args: Record<string, unknown> }
const METHODS = new Set(["run_job_now", "stop_active_run", "start_service", "stop_service", "restart_service", "run_workspace_task_operation", "stop_workspace_task_operation"]);
export const isRuntimeControl = (method: string): boolean => METHODS.has(method);
const prefix = () => `devbox-runtime-pending:${productInstallationId()}:`;
function read(key: string): Pending | null {
  const raw = localStorage.getItem(key);
  if (raw === null) return null;
  if (raw.length > 1_024) throw new Error("저장된 실행 요청을 확인할 수 없습니다.");
  const value = JSON.parse(raw) as Pending;
  if (!value || typeof value.operationId !== "string" || !/^[a-f0-9-]{36}$/.test(value.operationId)
    || !value.args || typeof value.args !== "object" || Array.isArray(value.args)
    || Object.keys(value).sort().join(",") !== "args,operationId") throw new Error("저장된 실행 요청을 확인할 수 없습니다.");
  return value;
}
export async function submitRuntimeControl<T>(call: Call, method: string, args: Record<string, unknown>): Promise<T> {
  if (!isRuntimeControl(method)) throw new Error("지원하지 않는 실행 요청입니다.");
  const idKey = method === "stop_workspace_task_operation" ? "operationId" : "id";
  const id = args[idKey];
  const fields = method === "run_workspace_task_operation" ? ["failFast", "id"] : [idKey];
  if (typeof id !== "string" || !/^[A-Za-z0-9_-]{1,128}$/.test(id)
    || Object.keys(args).sort().join(",") !== fields.sort().join(",")
    || ("failFast" in args && typeof args.failFast !== "boolean")) throw new Error("실행 요청을 확인할 수 없습니다.");
  const key = `${prefix()}${method}:${id}`;
  let pending = read(key);
  if (pending && JSON.stringify(pending.args) !== JSON.stringify(args)) throw new Error("이전 실행 요청의 상태를 먼저 확인해 주세요.");
  if (!pending) {
    let count = 0;
    for (let index = 0; index < localStorage.length; index++) if (localStorage.key(index)?.startsWith(prefix())) count++;
    if (count >= 64) throw new Error("완료되지 않은 실행 요청을 먼저 확인해 주세요.");
    pending = {operationId: crypto.randomUUID(), args};
    // Persist before IPC. Storage failure must not turn reload into a new run.
    localStorage.setItem(key, JSON.stringify(pending));
  }
  const submitted = pending;
  const remove = () => { if (read(key)?.operationId === submitted.operationId) localStorage.removeItem(key); };
  try {
    const result = await call<T>("runtime_control", {operationId:submitted.operationId,method,args:submitted.args});
    remove();
    return result;
  } catch (error) {
    // A confirmed terminal failure permits a later explicit new attempt.
    // Transport loss, pending and interrupted results retain the same ID.
    if (error instanceof WorkspaceOperationError && error.code === "runtime_control_failed") remove();
    throw error;
  }
}
export function forgetReviewedControl(operationId: string): void {
  const keys: string[] = [];
  for (let index = 0; index < localStorage.length; index++) {
    const key = localStorage.key(index);
    if (key?.startsWith(prefix()) && read(key)?.operationId === operationId) keys.push(key);
  }
  keys.forEach(key => localStorage.removeItem(key));
}
