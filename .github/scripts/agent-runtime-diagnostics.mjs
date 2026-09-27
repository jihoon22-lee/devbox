const stages = new Set([
  "initialize_host",
  "initialize_components",
  "initialize_engine",
  "initialize_processes",
  "initialize_log_provider",
  "initialize_logs",
  "runtime_query",
  "runtime_dispatch",
]);
const codes = new Set([
  "initialization_failed",
  "component_state_conflict",
  "component_storage_conflict",
  "component_storage_unavailable",
  "component_storage_changed",
  "component_recovery_unavailable",
  "component_window_unavailable",
  "component_shutdown_hook_unavailable",
  "runtime_owner_unavailable",
  "runtime_operation_unavailable",
  "store_unavailable",
  "store_generation_changed",
  "store_pointer_changed",
  "store_owner_busy",
  "setup_required",
  "registry_unavailable",
  "registry_invalid",
  "registry_read_only",
  "request_expired",
  "request_cancelled",
  "invalid_request",
  "agent_request_rejected",
  "busy",
  "stale_context",
  "session_invalid",
  "route_forbidden",
  "component_forbidden",
  "worker_unavailable",
]);
export function projectInitializationDiagnostics(text) {
  const rows = [];
  for (const line of text.slice(-65536).split("\n")) {
    try {
      const row = JSON.parse(line);
      if (
        row.product !== "agent" ||
        row.component !== "runtime" ||
        row.outcome !== "failed" ||
        !stages.has(row.method) ||
        !codes.has(row.code) ||
        !Number.isSafeInteger(row.tsMs) ||
        row.tsMs < 0
      )
        continue;
      rows.push({ tsMs: row.tsMs, method: row.method, code: row.code });
    } catch {}
  }
  return rows.slice(-32);
}

const connectionCodes = new Set([
  "connect_attempt",
  "connect_timeout",
  "connect_missing",
  "connect_unavailable",
  "connect_rejected",
  "launch_attempt",
  "launch_failed",
  "hello_failed",
  "welcome_timeout",
  "welcome_read_failed",
  "welcome_rejected",
  "welcome_mismatch",
  "peer_invalid",
  "connected",
  "scope_invalid",
  "pipe_busy",
  "pipe_denied",
  "pipe_disconnected",
  "pipe_other",
  "peer_capture_failed",
  "peer_verification_failed",
  "agent_spawn_failed",
  "agent_exited",
  "agent_exit_failed",
  "agent_pipe_unavailable",
]);
export function projectConnectionDiagnostics(text) {
  const rows = [];
  for (const line of text.slice(-65536).split("\n")) {
    try {
      const row = JSON.parse(line);
      if (
        !["workspace", "api-studio", "knowledge", "control-center", "mcp", "agent"].includes(row.product) ||
        row.component !== "agent-connection" ||
        !["connect", "start_listener"].includes(row.method) ||
        !connectionCodes.has(row.code) ||
        !["succeeded", "failed"].includes(row.outcome) ||
        !Number.isSafeInteger(row.tsMs) ||
        row.tsMs < 0
      )
        continue;
      rows.push({ tsMs: row.tsMs, product: row.product, code: row.code, outcome: row.outcome });
    } catch {}
  }
  return rows.slice(-96);
}
