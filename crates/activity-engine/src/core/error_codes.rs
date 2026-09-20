//! Stable source error-code contract shared by digest, export, and handoff.
//! Keeping one allowlist prevents a newly introduced bounded-runner code from
//! invalidating an otherwise partial Life Log response at a later boundary.

pub fn is_git(value: &str) -> bool {
    matches!(
        value,
        "git_invalid_arguments"
            | "git_invalid_target"
            | "git_spawn_failed"
            | "git_wsl_unavailable"
            | "git_wsl_failed"
            | "git_process_tree_unavailable"
            | "git_stdout_unavailable"
            | "git_wait_failed"
            | "git_timeout"
            | "git_reader_failed"
            | "git_output_read_failed"
            | "git_failed"
            | "git_output_invalid_utf8"
            | "git_output_too_large"
            | "git_output_invalid"
    )
}

pub fn is_snapshot(value: &str) -> bool {
    matches!(
        value,
        "snapshot_unavailable"
            | "snapshot_invalid"
            | "snapshot_schema_unsupported"
            | "snapshot_payload_invalid"
            | "snapshot_changed_during_read"
            | "snapshot_stale"
            | "snapshot_range_partial"
            | "snapshot_range_unavailable"
            | "snapshot_boundary_mismatch"
    )
}

pub fn is_source(value: &str) -> bool {
    value == "no_safe_project_paths" || is_git(value) || is_snapshot(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_process_tree_and_wsl_errors_once() {
        for code in [
            "git_process_tree_unavailable",
            "git_invalid_target",
            "git_wsl_unavailable",
            "git_wsl_failed",
        ] {
            assert!(is_git(code));
            assert!(is_source(code));
            assert!(!is_snapshot(code));
        }
        assert!(!is_source("raw OS error with /private/path"));
        for code in [
            "snapshot_range_partial",
            "snapshot_range_unavailable",
            "snapshot_boundary_mismatch",
        ] {
            assert!(is_snapshot(code));
            assert!(is_source(code));
        }
    }
}
