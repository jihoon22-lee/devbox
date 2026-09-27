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
