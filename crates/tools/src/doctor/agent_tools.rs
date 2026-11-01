//! Agent CLI checks: the executable a task runs, which shell found it and its version.
use super::checks::check;
use super::{AgentCommand, AgentTool, Probed};
use devbox_protocol::system::{Check, Status};

const MISSING_FIX: &str = "설치했다면 셸 환경 다시 읽기";

pub(super) fn tool_check(agent: AgentTool, found: Probed<Option<AgentCommand>>) -> Check {
    let id = format!("tool.{}", agent.binary());
    let tool = agent.binary();
    let command = match found {
        Ok(Some(command)) => command,
        Ok(None) => {
            return check(
                &id,
                Status::Warn,
                format!("{tool}: 로그인 셸과 대화형 셸 모두에서 찾지 못했습니다"),
                Some(MISSING_FIX),
            );
        }
        Err(issue) => return super::extended::unknown(&id, issue, MISSING_FIX),
    };
    let path = &command.path;
    let source = if command.interactive {
        "대화형 셸"
    } else {
        "로그인 셸"
    };
    if let (Some(minimum), Some(version)) = (agent.verified_minimum(), &command.version)
        && older(version, minimum)
    {
        return check(
            &id,
            Status::Warn,
            format!(
                "{tool} {version} · {source} · {path}: devbox가 확인한 가장 낮은 버전 {minimum}보다 낮습니다"
            ),
            Some(&format!("{minimum} 이상으로 업데이트하세요")),
        );
    }
    let version = command.version.as_deref().unwrap_or("버전 확인 못 함");
    let message = if command.interactive && command.login_path.is_some() {
        format!(
            "로그인 셸과 대화형 셸이 다른 {tool} 실행 파일을 가리킵니다. devbox는 대화형 셸의 {path}({version})를 씁니다."
        )
    } else {
        format!("{tool} {version} · {source} · {path}")
    };
    check(&id, Status::Ok, message, None)
}

/// Compares dotted numbers part by part, so `1.10.0` is newer than `1.3.0`.
fn older(version: &str, minimum: &str) -> bool {
    let parts = |text: &str| -> Option<Vec<u64>> {
        text.split('.').map(|part| part.parse().ok()).collect()
    };
    let (Some(mut version), Some(mut minimum)) = (parts(version), parts(minimum)) else {
        return false;
    };
    let len = version.len().max(minimum.len());
    version.resize(len, 0);
    minimum.resize(len, 0);
    version < minimum
}
