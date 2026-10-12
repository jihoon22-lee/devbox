// 이 파일은 `pnpm gen`이 만든다. 직접 고치지 않는다.
/* eslint-disable */

export type AddProject = { path: string, };
export type Candidate = { path: string, name: string, isGit: boolean, registered: boolean, };
export type Check = { id: string, status: Status, message: string, fix: string | null, };
export type CommonError = { "code": "internal" } | { "code": "invalid_params", "detail": { reason: string, } } | { "code": "method_not_found", "detail": { method: string, } } | { "code": "hello_required" } | { "code": "cancelled" } | { "code": "daemon_restarting" } | { "code": "not_found" } | { "code": "secrets_locked" };
export type DaemonInfo = { version: string, instance: string, pid: number, startedMs: number, dataDir: string, };
export type DebugError = { "code": "debug.unsupported" };
export type DoctorReport = { checks: Array<Check>, };
export type Done = { rev: string, };
export type EchoParams = { bytes: number, };
export type ListProjects = Record<symbol, never>;
export type PingParams = Record<symbol, never>;
export type Pong = { daemonId: string, version: string, uptimeMs: number, };
export type Project = { id: string, name: string, path: string, favorite: boolean, addedMs: number, };
export type ProjectList = { rev: string, items: Array<Project>, };
export type ProjectsChanged = { rev: string, };
export type ProjectsError = { "code": "projects.path_invalid", "detail": { path: string, } } | { "code": "projects.not_directory", "detail": { path: string, } } | { "code": "projects.already_exists", "detail": { path: string, } } | { "code": "projects.not_found", "detail": { id: string, } };
export type RemoveProject = { id: string, };
export type ScanProjects = Record<symbol, never>;
export type ScanResult = { candidates: Array<Candidate>, };
export type Status = "ok" | "warn" | "fail";
export type StreamOpened = { streamId: number, };
export type SystemError = { "code": "system.unavailable" };

export interface Methods {
  "debug.echo": { kind: "stream"; confirm: "none"; params: EchoParams; result: StreamOpened; error: DebugError };
  "projects.add": { kind: "mutation"; confirm: "none"; params: AddProject; result: Project; error: ProjectsError };
  "projects.list": { kind: "query"; confirm: "none"; params: ListProjects; result: ProjectList; error: ProjectsError };
  "projects.remove": { kind: "mutation"; confirm: "always"; params: RemoveProject; result: Done; error: ProjectsError };
  "projects.scan": { kind: "query"; confirm: "none"; params: ScanProjects; result: ScanResult; error: ProjectsError };
  "system.doctor": { kind: "query"; confirm: "none"; params: PingParams; result: DoctorReport; error: SystemError };
  "system.info": { kind: "query"; confirm: "none"; params: PingParams; result: DaemonInfo; error: SystemError };
  "system.ping": { kind: "query"; confirm: "none"; params: PingParams; result: Pong; error: SystemError };
}

export interface Topics {
  "projects.changed": ProjectsChanged;
}

export const METHOD_INFO = {
  "debug.echo": { kind: "stream", confirm: "none" },
  "projects.add": { kind: "mutation", confirm: "none" },
  "projects.list": { kind: "query", confirm: "none" },
  "projects.remove": { kind: "mutation", confirm: "always" },
  "projects.scan": { kind: "query", confirm: "none" },
  "system.doctor": { kind: "query", confirm: "none" },
  "system.info": { kind: "query", confirm: "none" },
  "system.ping": { kind: "query", confirm: "none" },
} as const;

export const RPC_ERROR_CODES = [
  "cancelled",
  "daemon_restarting",
  "debug.unsupported",
  "hello_required",
  "internal",
  "invalid_params",
  "method_not_found",
  "not_found",
  "projects.already_exists",
  "projects.not_directory",
  "projects.not_found",
  "projects.path_invalid",
  "secrets_locked",
  "system.unavailable",
] as const;
export type RpcErrorCode = (typeof RPC_ERROR_CODES)[number];
