//! stdio MCP 서버: 셸 없이 실행하고, 환경은 비운 뒤 허용 목록(PATH·HOME·TMPDIR·로캘·XDG_RUNTIME_DIR)과
//! 사용자가 적은 이름만 넘긴다(v0.9.0 규칙, 허용 목록 이름은 덮어쓸 수 없음). 로더·인터프리터 주입 이름
//! (`FORBIDDEN_*` 목록: `LD_*`·`NODE_OPTIONS`·`NPM_CONFIG_*`·허용 목록 밖 `PYTHON*` 등)은 거절한다. cwd는 등록 프로젝트 안만(비면 프로젝트 파일의 루트).
//! 새 프로세스 그룹으로 띄워 스트림이 닫히면 그룹 전체를 끝낸다(npx류의 손자 프로세스까지). stderr는 버린다.
use crate::{ApiError, Pair};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const MAX_ARGS: usize = 64;
const MAX_ARG_BYTES: usize = 8 * 1024;
const MAX_ARGS_BYTES: usize = 64 * 1024;
const MAX_ENV: usize = 64;
const MAX_ENV_NAME_BYTES: usize = 256;
const MAX_ENV_BYTES: usize = 256 * 1024;
/// stdin을 닫은 뒤 스스로 끝나기를 기다리는 시간.
const GRACEFUL: Duration = Duration::from_millis(500);

/// 부모(데몬) 환경에서 넘기는 이름. 사용자 env로 덮어쓸 수 없다.
pub(crate) const RUNTIME_ENV: [&str; 7] = [
    "PATH",
    "HOME",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "XDG_RUNTIME_DIR",
];

/// stdio MCP 서버 env에 쓰지 않는 이름의 유일한 목록(대소문자 무시, 출처와 관계없이). 검토 화면이 명령을 정확히
/// 보여도 이 값들은 실행 내용을 바꾼다(검토 1 B1, 검토 2 N1·N3, 검토 3 R1):
/// - 로더·셸: `LD_*`·`DYLD_*`·`BASH_FUNC_*`·`BASH_ENV`·`ENV`
/// - 인터프리터 코드 주입: `NODE_OPTIONS`·`NODE_PATH`·`NPM_CONFIG_*`(npx가 node 옵션으로 넘김)·`PERL5*`·`PERLLIB`·
///   `RUBYOPT`·`RUBYLIB`·`JAVA_TOOL_OPTIONS`·`_JAVA_OPTIONS`·`JDK_JAVA_OPTIONS`
/// - python: `PYTHON*` 전부(`PYTHONWARNINGS`의 점이 든 범주처럼 시작할 때 모듈을 import하는 이름이 계속 생긴다).
///   `PYTHON_ALLOWED`만 예외
/// - 패키지 색인·설정 파일: `PIP_*`·`UV_*`(uvx), 도구 설정: `GIT_*`·`DOCKER_HOST`·`DOCKER_CONFIG`·`XDG_CONFIG_HOME`
/// - 다른 프로그램 실행: `BROWSER`(webbrowser 모듈·여러 CLI가 이 명령을 실행)
const FORBIDDEN_PREFIXES: [&str; 9] = [
    "LD_",
    "DYLD_",
    "BASH_FUNC_",
    "NPM_CONFIG_",
    "PERL5",
    "PYTHON",
    "PIP_",
    "UV_",
    "GIT_",
];
const FORBIDDEN_NAMES: [&str; 14] = [
    "BASH_ENV",
    "ENV",
    "NODE_OPTIONS",
    "NODE_PATH",
    "PERLLIB",
    "RUBYOPT",
    "RUBYLIB",
    "JAVA_TOOL_OPTIONS",
    "_JAVA_OPTIONS",
    "JDK_JAVA_OPTIONS",
    "DOCKER_HOST",
    "DOCKER_CONFIG",
    "XDG_CONFIG_HOME",
    "BROWSER",
];
/// `PYTHON*` 중 쓰는 이름. 모두 코드·경로·모듈을 읽어 들이지 않는다:
/// - `PYTHONUNBUFFERED`: stdout·stderr 버퍼링만 끈다(stdio 서버의 응답이 바로 나가게)
/// - `PYTHONIOENCODING`: 표준 입출력 인코딩 이름(코덱 이름만, 경로·모듈 아님)
/// - `PYTHONUTF8`: UTF-8 모드 켜기·끄기(0/1)
/// - `PYTHONDONTWRITEBYTECODE`: `.pyc`를 쓰지 않음(쓰기만 줄이고 읽어 들이는 곳은 바꾸지 않음)
const PYTHON_ALLOWED: [&str; 4] = [
    "PYTHONUNBUFFERED",
    "PYTHONIOENCODING",
    "PYTHONUTF8",
    "PYTHONDONTWRITEBYTECODE",
];

pub(crate) fn forbidden(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    if PYTHON_ALLOWED.contains(&upper.as_str()) {
        return false;
    }
    FORBIDDEN_NAMES.contains(&upper.as_str())
        || FORBIDDEN_PREFIXES.iter().any(|p| upper.starts_with(p))
}

fn env_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && name.len() <= MAX_ENV_NAME_BYTES
}

/// 치환 전 모양 검사(연결·실행 전).
pub(crate) fn check(command: &str, args: &[String], env: &[Pair]) -> Result<(), ApiError> {
    devbox_core::input::validate_command_text(command)
        .map_err(|_| ApiError::input("command", "invalid"))?;
    if command.trim().is_empty() || command.trim() != command {
        return Err(ApiError::input("command", "invalid"));
    }
    if args.len() > MAX_ARGS
        || args
            .iter()
            .any(|a| a.len() > MAX_ARG_BYTES || a.contains('\0'))
        || args.iter().map(String::len).sum::<usize>() > MAX_ARGS_BYTES
    {
        return Err(ApiError::input("args", "invalid"));
    }
    if env.len() > MAX_ENV {
        return Err(ApiError::input("env", "too_many"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for pair in env {
        if !env_name(&pair.name) {
            return Err(ApiError::input("env", "invalid_name"));
        }
        if RUNTIME_ENV.contains(&pair.name.as_str()) {
            return Err(ApiError::input("env", "reserved"));
        }
        if forbidden(&pair.name) {
            return Err(ApiError::input("env", "forbidden"));
        }
        if !seen.insert(pair.name.as_str()) {
            return Err(ApiError::input("env", "duplicate"));
        }
    }
    Ok(())
}

/// 치환 뒤 값 검사(NUL·합계).
pub(crate) fn check_values(env: &[(String, String)]) -> Result<(), ApiError> {
    if env.iter().any(|(_, v)| v.contains('\0'))
        || env.iter().map(|(n, v)| n.len() + v.len()).sum::<usize>() > MAX_ENV_BYTES
    {
        return Err(ApiError::input("env", "invalid_value"));
    }
    Ok(())
}

/// cwd: 절대 경로이고 실제 경로가 등록 프로젝트 폴더 안이어야 한다. 비었으면 `default`(프로젝트 파일이면 그
/// 프로젝트 루트)를 쓰고, 그것도 없으면(초안·개인 파일) 실행하지 않는다(홈 폴더로 가지 않음).
pub(crate) fn cwd(
    cwd: Option<&str>,
    default: Option<&str>,
    project_roots: &[String],
) -> Result<PathBuf, ApiError> {
    let Some(cwd) = cwd.or(default) else {
        return Err(ApiError::input("cwd", "required"));
    };
    let path = Path::new(cwd);
    if !path.is_absolute() || cwd.contains('\0') {
        return Err(ApiError::input("cwd", "relative"));
    }
    let real = std::fs::canonicalize(path).map_err(|_| ApiError::input("cwd", "missing"))?;
    if !real.is_dir() {
        return Err(ApiError::input("cwd", "missing"));
    }
    let inside = project_roots
        .iter()
        .any(|root| std::fs::canonicalize(root).is_ok_and(|root| real.starts_with(&root)));
    if !inside {
        return Err(ApiError::input("cwd", "outside_projects"));
    }
    Ok(real)
}

/// 자식 환경(순수 함수): 부모의 허용 목록 이름 + 사용자 env. 다른 부모 변수는 넘기지 않는다.
pub(crate) fn child_env(
    parent: impl Fn(&str) -> Option<String>,
    user: &[(String, String)],
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = RUNTIME_ENV
        .iter()
        .filter_map(|name| parent(name).map(|v| ((*name).to_owned(), v)))
        .collect();
    out.extend(
        user.iter()
            .filter(|(n, _)| !RUNTIME_ENV.contains(&n.as_str()))
            .cloned(),
    );
    out
}

pub(crate) struct Plan {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    pub(crate) cwd: PathBuf,
    pub(crate) env: Vec<(String, String)>,
}

pub(crate) struct Child {
    child: tokio::process::Child,
    stdin: Option<tokio::process::ChildStdin>,
    stdout: BufReader<tokio::process::ChildStdout>,
    line: Vec<u8>,
    /// 프로세스 그룹 id(= 시작한 자식 pid). 회수 뒤에도 그룹에 신호를 보내려고 둔다.
    pid: Option<u32>,
}

pub(crate) fn spawn(plan: &Plan) -> Result<Child, ApiError> {
    let mut command = tokio::process::Command::new(&plan.program);
    command
        .args(&plan.args)
        .current_dir(&plan.cwd)
        .env_clear()
        .envs(plan.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().map_err(|_| ApiError::mcp("spawn"))?;
    let pid = child.id();
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().ok_or_else(|| ApiError::mcp("spawn"))?;
    Ok(Child {
        child,
        stdin,
        stdout: BufReader::new(stdout),
        line: vec![],
        pid,
    })
}

impl Child {
    pub(crate) async fn send(&mut self, message: &[u8]) -> Result<(), ApiError> {
        let stdin = self.stdin.as_mut().ok_or_else(|| ApiError::mcp("closed"))?;
        let mut line = message.to_vec();
        line.push(b'\n');
        stdin
            .write_all(&line)
            .await
            .map_err(|_| ApiError::mcp("closed"))?;
        stdin.flush().await.map_err(|_| ApiError::mcp("closed"))
    }

    /// 줄 하나(줄바꿈 제외). 끝나면 `None`. 한 줄이 상한을 넘으면 오류(메모리 증가 없음).
    pub(crate) async fn recv(&mut self) -> Result<Option<Vec<u8>>, ApiError> {
        loop {
            let available = self
                .stdout
                .fill_buf()
                .await
                .map_err(|_| ApiError::mcp("closed"))?;
            if available.is_empty() {
                return Ok(None);
            }
            let (take, newline) = match available.iter().position(|b| *b == b'\n') {
                Some(at) => (at, true),
                None => (available.len(), false),
            };
            if self.line.len() + take > super::MAX_MESSAGE_BYTES {
                return Err(crate::validate::too_large(super::MAX_MESSAGE_BYTES));
            }
            self.line.extend_from_slice(&available[..take]);
            self.stdout.consume(take + usize::from(newline));
            if newline {
                let mut line = std::mem::take(&mut self.line);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if !line.iter().all(u8::is_ascii_whitespace) {
                    return Ok(Some(line));
                }
            }
        }
    }

    /// stdin을 닫고 잠깐 기다린 뒤 프로세스 그룹 전체를 끝내고 회수한다.
    /// 서버가 스스로 끝났어도 같은 그룹의 손자 프로세스가 남을 수 있어 그룹에는 늘 신호를 보낸다.
    pub(crate) async fn shutdown(mut self) {
        self.stdin = None;
        let exited = tokio::time::timeout(GRACEFUL, self.child.wait())
            .await
            .is_ok();
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
        if !exited {
            let _ = self.child.start_kill();
            let _ = self.child.wait().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_environment_is_an_allowlist_plus_user_names() {
        let parent = |k: &str| match k {
            "PATH" => Some("/usr/bin".to_owned()),
            "HOME" => Some("/home/u".to_owned()),
            "AWS_SECRET_ACCESS_KEY" | "GITHUB_TOKEN" | "DEVBOX_CANARY" => Some("leak".to_owned()),
            _ => None,
        };
        let env = child_env(
            parent,
            &[
                ("API_KEY".into(), "v".into()),
                ("PATH".into(), "/evil".into()),
            ],
        );
        assert_eq!(
            env,
            vec![
                ("PATH".to_owned(), "/usr/bin".to_owned()),
                ("HOME".to_owned(), "/home/u".to_owned()),
                ("API_KEY".to_owned(), "v".to_owned()),
            ]
        );
    }

    #[test]
    fn reserved_and_malformed_inputs_are_refused() {
        let pair = |n: &str| Pair {
            name: n.into(),
            value: "x".into(),
        };
        assert!(check("node", &[], &[pair("API_KEY")]).is_ok());
        for bad in ["PATH", "HOME", "LD_PRELOAD=", "A-B", "", "1A"] {
            assert!(check("node", &[], &[pair(bad)]).is_err(), "{bad}");
        }
        assert!(check("node", &[], &[pair("A"), pair("A")]).is_err());
        assert!(check("", &[], &[]).is_err());
        assert!(check("no\0de", &[], &[]).is_err());
        assert!(check(" node", &[], &[]).is_err());
        assert!(check("node", &vec!["a".to_owned(); MAX_ARGS + 1], &[]).is_err());
        assert!(check("node", &["a\0".to_owned()], &[]).is_err());
        assert!(check("node", &["a".repeat(MAX_ARG_BYTES + 1)], &[]).is_err());
    }
}
