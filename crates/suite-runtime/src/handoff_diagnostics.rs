//! Fixed handoff failure vocabulary. Never project arbitrary peer errors.

pub(crate) fn code(error: &str) -> &'static str {
    match error {
        "suite_review_required" => "suite_review_required",
        "suite_busy" => "suite_busy",
        "suite_product_invalid" => "suite_product_invalid",
        "suite_activation_timeout" => "suite_activation_timeout",
        "suite_activation_pending" => "suite_activation_pending",
        "suite_activation_unavailable" => "suite_activation_unavailable",
        "suite_window_unavailable" => "suite_window_unavailable",
        "suite_route_owner_mismatch" => "suite_route_owner_mismatch",
        "peer_provider_unavailable" => "peer_provider_unavailable",
        "peer_owner_rejected" => "peer_owner_rejected",
        "peer_deadline_expired" => "peer_deadline_expired",
        "peer_read_failed" => "peer_read_failed",
        "peer_handshake_denied" => "peer_handshake_denied",
        "peer_product_mismatch" => "peer_product_mismatch",
        "command_metadata_invalid" => "command_metadata_invalid",
        "command_target_invalid" => "command_target_invalid",
        "command_request_stale" => "command_request_stale",
        "navigation_pending_limit" => "navigation_pending_limit",
        "navigation_receipt_limit" => "navigation_receipt_limit",
        "navigation_operation_conflict" => "navigation_operation_conflict",
        _ => "handoff_unavailable",
    }
}

#[cfg(windows)]
pub(crate) fn record(app: &tauri::AppHandle, method: &'static str, error: &str) {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        product_shell_tauri::begin_operation(&window, "suite-handoff", method).finish(
            &product_contract::OperationState::Failed {
                code: product_contract::ProblemCode::Unavailable,
            },
            Some(code(error)),
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn diagnostic_codes_are_fixed_and_unknown_errors_are_not_logged() {
        assert_eq!(super::code("peer_owner_rejected"), "peer_owner_rejected");
        assert_eq!(super::code("private-secret"), "handoff_unavailable");
        assert_eq!(super::code("C:\\private\\data"), "handoff_unavailable");
    }
}
