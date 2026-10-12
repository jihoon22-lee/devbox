// 이 파일은 `pnpm gen`이 만든다. 직접 고치지 않는다.
/* eslint-disable */

export type CommonError = { "code": "internal" } | { "code": "invalid_params", "detail": { reason: string, } } | { "code": "method_not_found", "detail": { method: string, } } | { "code": "hello_required" } | { "code": "cancelled" } | { "code": "daemon_restarting" } | { "code": "not_found" } | { "code": "secrets_locked" };
export type DaemonInfo = { version: string, instance: string, pid: number, startedMs: number, dataDir: string, };
export type PingParams = Record<symbol, never>;
export type Pong = { daemonId: string, version: string, uptimeMs: number, };
export type SystemError = { "code": "system.unavailable" };

export interface Methods {
  "system.info": { kind: "query"; confirm: "none"; params: PingParams; result: DaemonInfo; error: SystemError };
  "system.ping": { kind: "query"; confirm: "none"; params: PingParams; result: Pong; error: SystemError };
}

export interface Topics {
}

export const METHOD_INFO = {
  "system.info": { kind: "query", confirm: "none" },
  "system.ping": { kind: "query", confirm: "none" },
} as const;

export const RPC_ERROR_CODES = [
  "cancelled",
  "daemon_restarting",
  "hello_required",
  "internal",
  "invalid_params",
  "method_not_found",
  "not_found",
  "secrets_locked",
  "system.unavailable",
] as const;
export type RpcErrorCode = (typeof RPC_ERROR_CODES)[number];
