#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[tokio::test]
    async fn failed_report_stdout_survives_generic_stderr_and_becomes_actionable() {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.args(["-c",r#"printf '%s' '{"checks":[{"id":"tmux","status":"fail","message":"tmux 3.2 이상이 필요합니다","fix":"sudo apt install tmux"}]}'; printf 'doctor found failed checks' >&2; exit 1"#]);
        let error = crate::core::process::run(
            cmd,
            vec![],
            std::time::Duration::from_secs(2),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let text = describe(error, ReportKind::Doctor).to_string();
        assert!(text.contains("tmux 3.2 이상"));
        assert!(text.contains("sudo apt install tmux"));
        assert!(!text.contains("checks"));
    }
    #[test]
    fn setup_names_failed_step_and_invalid_reports_keep_plain_diagnostic() {
        let error = ShellError::Exit {
            code: Some(1),
            stdout: r#"{"steps":[{"id":"restart","ok":false,"message":"unit unavailable"}]}"#
                .into(),
            stderr: "setup failed".into(),
        };
        assert!(
            describe(error, ReportKind::Setup)
                .to_string()
                .contains("서비스 다시 시작")
        );
        let error = ShellError::Exit {
            code: Some(1),
            stdout: "WSL_E_ACCESS_DENIED".into(),
            stderr: "wsl failed".into(),
        };
        assert!(
            describe(error, ReportKind::Setup)
                .to_string()
                .contains("WSL_E_ACCESS_DENIED")
        );
    }
}
use super::process::ShellError;
use devbox_protocol::system::{DoctorReport, Status};
use serde::Deserialize;
pub enum ReportKind {
    Setup,
    Doctor,
}
#[derive(Deserialize)]
struct SetupReport {
    steps: Vec<Step>,
}
#[derive(Deserialize)]
struct Step {
    id: String,
    ok: bool,
    message: String,
}
fn step_name(id: &str) -> &str {
    match id {
        "env" => "로그인 환경 읽기",
        "units" => "서비스 설정 저장",
        "reload" => "서비스 설정 반영",
        "enable" => "서비스 시작",
        "restart" => "서비스 다시 시작",
        _ => id,
    }
}
pub fn describe(error: ShellError, kind: ReportKind) -> ShellError {
    let ShellError::Exit { stdout, .. } = &error else {
        return error;
    };
    let messages = match kind {
        ReportKind::Setup => serde_json::from_str::<SetupReport>(stdout)
            .ok()
            .map(|report| {
                report
                    .steps
                    .into_iter()
                    .filter(|step| !step.ok)
                    .map(|step| format!("{} 실패: {}", step_name(&step.id), step.message))
                    .collect::<Vec<_>>()
            }),
        ReportKind::Doctor => serde_json::from_str::<DoctorReport>(stdout)
            .ok()
            .map(|report| {
                report
                    .checks
                    .into_iter()
                    .filter(|check| check.status == Status::Fail)
                    .map(|check| {
                        format!(
                            "진단 실패 ({}): {}{}",
                            check.id,
                            check.message,
                            check
                                .fix
                                .map(|fix| format!(". 해결 방법: {fix}"))
                                .unwrap_or_default()
                        )
                    })
                    .collect::<Vec<_>>()
            }),
    };
    match messages {
        Some(messages) if !messages.is_empty() => ShellError::Invalid(messages.join("\n")),
        _ => error,
    }
}
