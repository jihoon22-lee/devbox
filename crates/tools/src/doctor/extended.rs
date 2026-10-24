use super::checks::check;
use super::{AgentTool, Probe, ProbeIssue, Probed};
use devbox_protocol::system::{Check, Status};

pub(super) fn unknown(id: &str, issue: ProbeIssue, fix: &str) -> Check {
    let message = match issue {
        ProbeIssue::Timeout => "확인하지 못함: 2초 시한을 초과했습니다",
        ProbeIssue::Missing => "확인하지 못함: 필요한 파일이나 명령을 찾지 못했습니다",
        ProbeIssue::Unavailable => "확인하지 못함: 읽기 또는 명령 실행에 실패했습니다",
        ProbeIssue::UnsupportedCapability => {
            "확인하지 못함: 설치된 CLI가 읽기 전용 확인 옵션을 지원하지 않습니다"
        }
        ProbeIssue::InvalidData => "확인하지 못함: 응답 형식이나 크기가 올바르지 않습니다",
    };
    check(id, Status::Warn, message, Some(fix))
}
/// Render only the numeric version core, never arbitrary CLI output/paths.
fn version(text: Option<&str>) -> Option<String> {
    text?.split_whitespace().find_map(|word| {
        let number: String = word
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let parts: Vec<_> = number.split('.').collect();
        (number.len() <= 32 && parts.len() >= 2 && parts.iter().all(|part| !part.is_empty()))
            .then_some(number)
    })
}
fn evidence(id: &str, value: Probed<bool>, ok: &str, missing: &str, fix: &str) -> Check {
    match value {
        Ok(true) => check(id, Status::Ok, ok, None),
        Ok(false) => check(id, Status::Warn, missing, Some(fix)),
        Err(issue) => unknown(id, issue, fix),
    }
}
pub(super) fn checks(p: &dyn Probe) -> Vec<Check> {
    let idle_fix = "앱 연결을 유지하세요. 선택적으로 Windows .wslconfig의 [general] instanceIdleTimeout=-1을 검토하세요(자동 변경 안 함)";
    let idle = match p
        .wsl_config()
        .and_then(|text| super::wsl::idle_settings(text.as_deref()))
    {
        Ok(value) => check(
            "wsl.idle",
            if value.instance_ms == -1 {
                Status::Ok
            } else {
                Status::Warn
            },
            format!(
                "instanceIdleTimeout={}ms · vmIdleTimeout={}ms (설정 파일 기준, 적용 여부는 확인하지 않음)",
                value.instance_ms, value.vm_ms
            ),
            (value.instance_ms != -1).then_some(idle_fix),
        ),
        Err(issue) => unknown("wsl.idle", issue, idle_fix),
    };
    let rust_fix = "rustup 공식 설치 안내를 확인하고 cargo·rustc가 PATH에 있는지 확인하세요";
    let rust = match p.rust_versions() {
        Ok(value) => match (
            version(value.cargo.as_deref()),
            version(value.rustc.as_deref()),
        ) {
            (Some(cargo), Some(rustc)) => check(
                "tool.rust",
                Status::Ok,
                format!("cargo {cargo} · rustc {rustc}"),
                None,
            ),
            _ => check(
                "tool.rust",
                Status::Warn,
                "cargo·rustc 버전을 확인하지 못했습니다",
                Some(rust_fix),
            ),
        },
        Err(issue) => unknown("tool.rust", issue, rust_fix),
    };
    let docker_fix = "Docker Desktop 설치·실행 상태와 이 배포판의 WSL 연동을 확인하세요";
    let docker = match p.docker() {
        Ok(value) => match (version(value.version.as_deref()), value.responsive) {
            (Some(version), true) => check(
                "tool.docker",
                Status::Ok,
                format!("Docker {version} · 데몬 응답 확인"),
                None,
            ),
            _ => check(
                "tool.docker",
                Status::Warn,
                "Docker CLI 또는 데몬 응답을 확인하지 못했습니다",
                Some(docker_fix),
            ),
        },
        Err(issue) => unknown("tool.docker", issue, docker_fix),
    };
    let gh = p.gh_authenticated();
    let gh_fix = if matches!(gh, Err(ProbeIssue::UnsupportedCapability)) {
        "GitHub CLI를 업데이트해 --no-refresh 지원을 확인하세요. 자동 인증 조회는 생략했습니다"
    } else {
        "gh auth login으로 로그인 상태를 확인하세요"
    };
    let mut out = vec![
        idle,
        rust,
        docker,
        evidence(
            "auth.gh",
            gh,
            "GitHub CLI 인증 상태 확인",
            "GitHub CLI 인증 상태를 확인하지 못했습니다",
            gh_fix,
        ),
    ];
    for agent in [AgentTool::Claude, AgentTool::Codex] {
        let id = format!("auth.{}", agent.binary());
        let fix = "CLI에서 로그인 상태를 확인하세요. 키링·환경 변수 인증은 파일 존재만으로 판단하지 않습니다";
        out.push(match p.agent_auth(agent) {
            Ok(value) => match (version(value.version.as_deref()), value.file_present) {
                (Some(version), true) => check(
                    &id,
                    Status::Ok,
                    format!(
                        "{} {version} · 인증 파일 존재 확인(인증 유효성은 확인하지 않음)",
                        agent.binary()
                    ),
                    None,
                ),
                _ => check(
                    &id,
                    Status::Warn,
                    "CLI 버전 또는 인증 파일을 확인하지 못했습니다(인증 유효성은 확인하지 않음)",
                    Some(fix),
                ),
            },
            Err(issue) => unknown(&id, issue, fix),
        });
        out.push(evidence(&format!("hook.{}", agent.binary()), p.agent_hook(agent),
            "설정에서 devbox hook 연결을 확인했습니다(실행 확인은 아님)", "전역 설정에서 devbox hook 연결을 찾지 못했습니다",
            "전역 hook 설정을 확인하세요. devbox가 시작한 작업은 실행 인자로 hook을 연결할 수 있습니다"));
    }
    out
}
