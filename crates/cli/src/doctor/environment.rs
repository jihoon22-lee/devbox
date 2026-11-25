//! Which environment a `devbox doctor` run looks tools up in, and how the run says so.
use super::runner;
use devbox_tools::doctor::ProbeIssue;
use serde::Serialize;
use std::path::Path;

/// Why a run used the caller's environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Reason {
    /// `--startup`: the app's check, made in the environment the app runs it in.
    Startup,
    Missing,
    /// A link, a folder or anything else that is not a regular file: never followed or read.
    NotRegular,
    /// No usable `PATH`, over 64 KiB or not UTF-8.
    Invalid,
    Unreadable,
}

/// The environment of one doctor run.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Lookup {
    /// The instance's env file, set over the caller's environment. systemd gives that file to
    /// the daemon, its tmux server and every run (`EnvironmentFile=`).
    Instance(Vec<(String, String)>),
    Caller(Reason),
}

/// A hand-run doctor answers "will devbox find this tool?", so it uses the instance's saved
/// environment when that file is usable. The read is bounded like every other probe's, and
/// anything but a file with a usable `PATH` falls back to the caller's environment.
pub(super) fn select(env_file: &Path, startup: bool) -> Lookup {
    if startup {
        return Lookup::Caller(Reason::Startup);
    }
    match runner::run(runner::read_text(env_file.to_owned())) {
        Ok(text) if crate::setup::env_path(&text).is_some() => Lookup::Instance(
            crate::setup::env_entries(&text)
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        ),
        Ok(_) => Lookup::Caller(Reason::Invalid),
        Err(ProbeIssue::Missing) => Lookup::Caller(Reason::Missing),
        // The read refuses a link (it does not follow one) and a folder; say that, not the
        // read's own error.
        Err(_) if runner::run(runner::is_regular_file(env_file.to_owned())) == Ok(false) => {
            Lookup::Caller(Reason::NotRegular)
        }
        Err(ProbeIssue::InvalidData) => Lookup::Caller(Reason::Invalid),
        Err(_) => Lookup::Caller(Reason::Unreadable),
    }
}

/// What a report says about its environment: the `environment` field of `--json`.
#[derive(Debug, Serialize)]
#[serde(tag = "source", rename_all = "camelCase")]
pub(super) enum Used {
    Instance { file: String },
    Caller { file: String, reason: Reason },
}

impl Used {
    pub(super) fn of(lookup: &Lookup, env_file: &Path) -> Self {
        let file = env_file.display().to_string();
        match lookup {
            Lookup::Instance(_) => Self::Instance { file },
            Lookup::Caller(reason) => Self::Caller {
                file,
                reason: *reason,
            },
        }
    }

    /// The first line of the text output. The agent CLI rows ask the user's shell, which starts
    /// from the caller's environment whichever `PATH` the other rows use.
    pub(super) fn line(&self) -> String {
        let differs = "데몬이 찾는 도구와 다를 수 있으니";
        let (file, reason) = match self {
            Self::Instance { file } => {
                return format!(
                    "점검 환경: 데몬 환경 파일 {file}의 PATH로 도구를 찾았습니다(에이전트 CLI의 셸 조회는 이 셸에서 합니다)"
                );
            }
            Self::Caller { file, reason } => (file, reason),
        };
        let why = match reason {
            Reason::Startup => "앱이 시작할 때 하는 점검이라 실행한 환경 그대로 봅니다".to_owned(),
            Reason::Missing => format!(
                "데몬 환경 파일이 없습니다: {file}. {differs} devbox setup으로 만든 뒤 다시 점검하세요"
            ),
            Reason::NotRegular => format!(
                "데몬 환경 파일이 일반 파일이 아닙니다(링크·폴더 등은 읽지 않습니다): {file}. {differs} 그것을 옮기거나 지우고 devbox setup으로 다시 만든 뒤 다시 점검하세요"
            ),
            Reason::Invalid => format!(
                "데몬 환경 파일에 쓸 수 있는 PATH가 없거나 형식·크기가 올바르지 않습니다: {file}. {differs} devbox setup으로 다시 만든 뒤 다시 점검하세요"
            ),
            Reason::Unreadable => format!(
                "데몬 환경 파일을 읽지 못했습니다: {file}. {differs} 파일 권한을 확인하거나 devbox setup으로 다시 만든 뒤 다시 점검하세요"
            ),
        };
        format!("점검 환경: 이 셸의 PATH로 도구를 찾았습니다({why})")
    }
}

/// `devbox doctor --json`: the protocol's report and the environment it was made in.
#[derive(Serialize)]
pub(super) struct Output<'a> {
    pub environment: &'a Used,
    #[serde(flatten)]
    pub report: &'a devbox_protocol::system::DoctorReport,
}

#[cfg(test)]
#[path = "environment_tests.rs"]
mod tests;
