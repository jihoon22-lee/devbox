//! `devbox project list` and `devbox project add <path>…`: the registered projects through the
//! daemon (`projects.list`, `projects.add`), for scripts and for registering many folders at once.
use crate::client::{Client, ClientError};
use devbox_projects::{AddProject, ListProjects, ProjectsAdd, ProjectsList};
use devbox_protocol::message::ClientKind;
use std::path::{Path, PathBuf};

/// A connection that failed or dropped before anything was asked of the daemon.
const NOT_CONNECTED: &str = "데몬에 연결할 수 없습니다. devbox 앱을 켠 뒤 다시 실행하세요.";

async fn connect() -> anyhow::Result<Client> {
    let paths = crate::paths()?;
    Client::connect(&paths.socket, ClientKind::Cli, paths.instance.as_str())
        .await
        .map_err(|error| anyhow::anyhow!(reason(&error, NOT_CONNECTED)))
}

/// Text for the terminal: a control character is written as its escape (`\u{1b}`, `\n`, `\t`),
/// so a name or path cannot move the cursor, change colours or break the lines and columns.
fn shown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_control() {
            out.extend(c.escape_default());
        } else {
            out.push(c);
        }
    }
    out
}

/// One line per project, `name<TAB>path`, in the daemon's order; `json` prints its list as is.
pub async fn list(json: bool) -> anyhow::Result<()> {
    let lost = "데몬 연결이 끊겨 목록을 읽지 못했습니다. 다시 실행하세요.";
    let list = connect()
        .await?
        .call::<ProjectsList>(&ListProjects {})
        .await
        .map_err(|error| anyhow::anyhow!(reason(&error, lost)))?;
    if json {
        println!("{}", serde_json::to_string(&list)?);
    } else {
        for project in &list.items {
            println!("{}\t{}", shown(&project.name), shown(&project.path));
        }
    }
    Ok(())
}

/// Registers each path and writes one line for it: a result on stdout, a failure on stderr. A
/// registered path is skipped; a failed one does not stop the rest, and makes the command fail
/// at the end.
pub async fn add(paths: Vec<PathBuf>) -> anyhow::Result<()> {
    let client = connect().await?;
    let cwd = std::env::current_dir().ok();
    let mut failed = 0_usize;
    for path in paths {
        match add_one(&client, cwd.as_deref(), &path).await {
            Ok(Added::New(path)) => println!("등록됨: {}", shown(&path)),
            Ok(Added::Registered(path)) => println!("이미 등록됨: {}", shown(&path)),
            Err(reason) => {
                failed += 1;
                eprintln!("실패: {}: {reason}", shown(&path.display().to_string()));
            }
        }
    }
    if failed > 0 {
        anyhow::bail!("등록하지 못한 경로가 {failed}개 있습니다.")
    }
    Ok(())
}

/// The project's path as the daemon keeps it.
enum Added {
    New(String),
    Registered(String),
}

/// The error is the line's cause and next step.
async fn add_one(client: &Client, cwd: Option<&Path>, path: &Path) -> Result<Added, String> {
    let absolute = match (path.is_absolute(), cwd) {
        (true, _) => path.to_owned(),
        (false, Some(cwd)) => cwd.join(path),
        (false, None) => {
            return Err("현재 폴더를 읽을 수 없습니다. 절대 경로로 넣으세요.".into());
        }
    };
    let Some(text) = absolute.to_str() else {
        return Err("경로가 UTF-8 글자가 아닙니다. 폴더 이름을 바꾼 뒤 다시 실행하세요.".into());
    };
    match client
        .call::<ProjectsAdd>(&AddProject { path: text.into() })
        .await
    {
        Ok(project) => Ok(Added::New(project.path)),
        Err(ClientError::Rpc { code, detail, .. }) if code == "projects.already_exists" => {
            #[derive(serde::Deserialize)]
            struct Existing {
                path: String,
            }
            let registered = detail
                .and_then(|detail| serde_json::from_str::<Existing>(detail.get()).ok())
                .map_or_else(|| text.to_owned(), |existing| existing.path);
            Ok(Added::Registered(registered))
        }
        // The request was sent: the daemon may have registered the folder.
        Err(error) => Err(reason(
            &error,
            "데몬 연결이 끊겨 결과를 확인하지 못했습니다. devbox project list로 확인한 뒤 다시 실행하세요.",
        )),
    }
}

/// A daemon or connection error as a cause and a next step. `lost` is the sentence for a
/// connection that dropped: what that means depends on what had been asked.
fn reason(error: &ClientError, lost: &str) -> String {
    match error {
        ClientError::Rpc {
            code,
            diagnostic_id,
            ..
        } => match (code.as_str(), diagnostic_id) {
            ("projects.path_invalid", _) => {
                "경로를 열 수 없습니다. 경로가 맞는지, WSL 안의 폴더인지 확인하세요.".into()
            }
            ("projects.not_directory", _) => {
                "폴더가 아닙니다. 프로젝트 폴더의 경로를 넣으세요.".into()
            }
            ("daemon_restarting", _) => {
                "데몬이 다시 시작하는 중입니다. 잠시 뒤 다시 실행하세요.".into()
            }
            (code, Some(id)) => format!(
                "예상하지 못한 문제가 생겼습니다({code}, 진단 ID {id}). 잠시 뒤 다시 실행하세요."
            ),
            (code, None) => {
                format!("예상하지 못한 문제가 생겼습니다({code}). 잠시 뒤 다시 실행하세요.")
            }
        },
        ClientError::Io(_) => NOT_CONNECTED.into(),
        ClientError::ConnectionLost => lost.into(),
        ClientError::Protocol(_) | ClientError::Decode(_) => {
            "데몬의 응답을 읽지 못했습니다. devbox 앱과 같은 버전의 devbox로 다시 실행하세요."
                .into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rpc(code: &str, diagnostic_id: Option<&str>) -> ClientError {
        ClientError::Rpc {
            code: code.into(),
            detail: None,
            diagnostic_id: diagnostic_id.map(Into::into),
        }
    }

    const LOST: &str = "fixture 연결이 끊겼습니다. 다시 실행하세요.";

    #[test]
    fn every_error_is_a_sentence_with_a_next_step_not_a_bare_code() {
        for error in [
            rpc("projects.path_invalid", None),
            rpc("projects.not_directory", None),
            rpc("daemon_restarting", None),
            rpc("internal", Some("diag-7")),
            rpc("projects.in_use", None),
            ClientError::Io(std::io::ErrorKind::NotFound.into()),
            ClientError::ConnectionLost,
            ClientError::Protocol("welcome mismatch"),
        ] {
            let text = reason(&error, LOST);
            assert!(text.ends_with("세요."), "{text}");
            assert!(text.matches(". ").count() >= 1, "{text}");
        }
        // An unknown code is still named, with its diagnostic id, for a report.
        let unknown = reason(&rpc("internal", Some("diag-7")), LOST);
        assert!(
            unknown.contains("internal") && unknown.contains("diag-7"),
            "{unknown}"
        );
        assert!(!reason(&rpc("projects.path_invalid", None), LOST).contains("projects."));
    }

    #[test]
    fn a_dropped_connection_is_worded_by_what_had_been_asked() {
        assert_eq!(reason(&ClientError::ConnectionLost, LOST), LOST);
        // Not being able to connect at all never depends on it.
        let refused = ClientError::Io(std::io::ErrorKind::ConnectionRefused.into());
        assert_eq!(reason(&refused, LOST), NOT_CONNECTED);
    }

    #[test]
    fn shown_text_keeps_letters_and_escapes_every_control_character() {
        assert_eq!(shown("한글 name-1/경로"), "한글 name-1/경로");
        assert_eq!(
            shown("a\u{1b}[31mb\nc\td\re\u{7}\u{9b}"),
            "a\\u{1b}[31mb\\nc\\td\\re\\u{7}\\u{9b}"
        );
    }
}
