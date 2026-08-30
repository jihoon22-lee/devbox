use crate::distro::validate_distro_name;
use crate::WslError;

/// `wsl.exe` 실행 argv를 조립한다.
///
/// ```text
/// wsl.exe -d <distro> [--cd <cwd>] -- [<command>]
/// ```
///
/// `command`는 단일 argv 요소로 끝에 붙는다. 빈 command면 `--`까지만 반환한다
/// (호출자가 실행 정책 argv를 이어 붙일 때 사용). cwd가 있으면 `--cd`로 전달한다.
/// distro는 검증한다 (argv 주입 방지).
pub fn build_exec_argv(
    distro: &str,
    cwd: Option<&str>,
    command: &str,
) -> Result<Vec<String>, WslError> {
    validate_distro_name(distro)?;
    let mut argv = vec!["wsl.exe".to_owned(), "-d".to_owned(), distro.to_owned()];
    if let Some(cwd) = cwd.filter(|c| !c.is_empty()) {
        argv.push("--cd".to_owned());
        argv.push(cwd.to_owned());
    }
    argv.push("--".to_owned());
    if !command.is_empty() {
        argv.push(command.to_owned());
    }
    Ok(argv)
}

/// `wsl.exe --exec` argv를 조립한다.
///
/// `--`는 뒤의 값을 기본 Linux 셸에 command line으로 전달하므로 공백이나
/// 메타문자가 포함된 인자가 다시 해석될 수 있다. 이 함수는 `--exec`를 사용해
/// 호출자가 뒤에 붙이는 프로그램과 인자를 각각의 argv 요소로 보존한다.
/// 빈 command면 `--exec`까지만 반환한다.
pub fn build_direct_exec_argv(
    distro: &str,
    cwd: Option<&str>,
    command: &str,
) -> Result<Vec<String>, WslError> {
    validate_distro_name(distro)?;
    let mut argv = vec!["wsl.exe".to_owned(), "-d".to_owned(), distro.to_owned()];
    if let Some(cwd) = cwd.filter(|c| !c.is_empty()) {
        argv.push("--cd".to_owned());
        argv.push(cwd.to_owned());
    }
    argv.push("--exec".to_owned());
    if !command.is_empty() {
        argv.push(command.to_owned());
    }
    Ok(argv)
}

/// Windows 경로를 WSL 경로로 변환하기 위한 `wslpath` 실행 argv.
///
/// ```text
/// wsl.exe -d <distro> -- wslpath -u -- <windows_path>
/// ```
///
/// 경로는 단일 argv 요소로 유지된다 (셸 인용 금지).
pub fn build_wslpath_argv(distro: &str, windows_path: &str) -> Result<Vec<String>, WslError> {
    validate_distro_name(distro)?;
    if windows_path.trim().is_empty() {
        return Err(WslError::InvalidPath("빈 Windows 경로".into()));
    }
    Ok(vec![
        "wsl.exe".to_owned(),
        "-d".to_owned(),
        distro.to_owned(),
        "--".to_owned(),
        "wslpath".to_owned(),
        "-u".to_owned(),
        "--".to_owned(),
        windows_path.to_owned(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_plain_exec_argv() {
        let argv = build_exec_argv("Ubuntu", None, "docker ps").unwrap();
        assert_eq!(argv, vec!["wsl.exe", "-d", "Ubuntu", "--", "docker ps"]);
    }

    #[test]
    fn builds_exec_argv_with_cwd() {
        let argv = build_exec_argv("Ubuntu", Some("/mnt/e/proj"), "ls").unwrap();
        assert_eq!(
            argv,
            vec!["wsl.exe", "-d", "Ubuntu", "--cd", "/mnt/e/proj", "--", "ls"]
        );
    }

    #[test]
    fn ignores_empty_cwd() {
        let argv = build_exec_argv("Ubuntu", Some(""), "ls").unwrap();
        assert_eq!(argv, vec!["wsl.exe", "-d", "Ubuntu", "--", "ls"]);
    }

    #[test]
    fn rejects_invalid_distro() {
        assert!(build_exec_argv("a;b", None, "ls").is_err());
        assert!(build_exec_argv("", None, "ls").is_err());
        // 공백은 유효한 argv (단일 요소)
        assert!(build_exec_argv("a b", None, "ls").is_ok());
    }

    #[test]
    fn empty_command_returns_base_only() {
        let argv = build_exec_argv("Ubuntu", None, "").unwrap();
        assert_eq!(argv, vec!["wsl.exe", "-d", "Ubuntu", "--"]);
        let argv = build_exec_argv("Ubuntu", Some("/mnt/e/p"), "").unwrap();
        assert_eq!(
            argv,
            vec!["wsl.exe", "-d", "Ubuntu", "--cd", "/mnt/e/p", "--"]
        );
    }

    #[test]
    fn builds_direct_exec_argv_without_shell_reinterpretation() {
        let argv = build_direct_exec_argv("Ubuntu", Some("/work tree"), "printf").unwrap();
        assert_eq!(
            argv,
            vec![
                "wsl.exe",
                "-d",
                "Ubuntu",
                "--cd",
                "/work tree",
                "--exec",
                "printf"
            ]
        );
        assert_eq!(
            build_direct_exec_argv("Ubuntu", None, "").unwrap(),
            vec!["wsl.exe", "-d", "Ubuntu", "--exec"]
        );
    }

    #[test]
    fn builds_wslpath_argv() {
        let argv = build_wslpath_argv("Ubuntu", "E:\\proj\\a b").unwrap();
        assert_eq!(
            argv,
            vec![
                "wsl.exe",
                "-d",
                "Ubuntu",
                "--",
                "wslpath",
                "-u",
                "--",
                "E:\\proj\\a b"
            ]
        );
    }

    #[test]
    fn wslpath_rejects_empty_path() {
        assert!(build_wslpath_argv("Ubuntu", "  ").is_err());
    }
}
