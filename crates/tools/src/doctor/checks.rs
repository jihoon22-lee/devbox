use super::{Probe, Probed};
use devbox_protocol::system::{Check, Status};

pub fn tmux_at_least(v: &str, want: (u32, u32)) -> bool {
    let Some(v) = v.strip_prefix("tmux ") else {
        return false;
    };
    let v = v.strip_prefix("next-").unwrap_or(v);
    let Some((major, rest)) = v.split_once('.') else {
        return false;
    };
    let len = rest.bytes().take_while(u8::is_ascii_digit).count();
    let (minor, suffix) = rest.split_at(len);
    if !suffix.bytes().all(|b| b.is_ascii_alphabetic()) {
        return false;
    }
    match (major.parse::<u32>(), minor.parse::<u32>()) {
        (Ok(a), Ok(b)) => (a, b) >= want,
        _ => false,
    }
}
pub(super) fn check(
    id: &str,
    status: Status,
    message: impl Into<String>,
    fix: Option<&str>,
) -> Check {
    Check {
        id: id.into(),
        status,
        message: message.into(),
        fix: fix.map(Into::into),
    }
}
fn boolean(
    id: &str,
    value: Probed<bool>,
    missing: Status,
    ok: &str,
    bad: &str,
    fix: &str,
) -> Check {
    match value {
        Ok(true) => check(id, Status::Ok, ok, None),
        Ok(false) => check(id, missing, bad, Some(fix)),
        Err(issue) => super::extended::unknown(id, issue, fix),
    }
}
pub fn checks(p: &dyn Probe) -> Vec<Check> {
    let systemd = p.pid1().and_then(|pid| {
        if pid.trim() == "systemd" {
            p.user_manager_available()
        } else {
            Ok(false)
        }
    });
    let mut out = vec![boolean(
        "systemd",
        systemd,
        Status::Fail,
        "systemd가 켜져 있습니다",
        "WSL에서 systemd가 꺼져 있습니다",
        "/etc/wsl.conf에 [boot] systemd=true를 넣고 wsl --shutdown",
    )];
    out.push(boolean(
        "linger",
        p.linger()
            .map(|text| text.lines().any(|line| line.trim() == "Linger=yes")),
        Status::Warn,
        "linger가 켜져 있습니다(사용자 관리자 유지용 보험이며 WSL 종료를 막지는 않습니다)",
        "linger가 꺼져 있습니다(권장, WSL 종료 방지 기능은 아닙니다)",
        "loginctl enable-linger <user>",
    ));
    out.push(boolean(
        "tool.tmux",
        p.tmux_version()
            .map(|v| v.is_some_and(|v| tmux_at_least(&v, (3, 2)))),
        Status::Fail,
        "tmux 3.2 이상을 사용할 수 있습니다",
        "tmux 3.2 이상이 필요합니다",
        "sudo apt install tmux 또는 최신 tmux 설치",
    ));
    out.push(match p.inotify_instances() {
        Ok(text) => {
            let inotify = text.trim().parse::<u32>().unwrap_or(0);
            check(
                "inotify",
                if inotify >= 512 {
                    Status::Ok
                } else {
                    Status::Warn
                },
                format!("max_user_instances={inotify}"),
                (inotify < 512).then_some("fs.inotify.max_user_instances를 1024로 설정"),
            )
        }
        Err(issue) => {
            super::extended::unknown("inotify", issue, "inotify 설정을 읽을 수 있는지 확인하세요")
        }
    });
    out.push(boolean(
        "env",
        p.env_path_valid(),
        Status::Fail,
        "사용자 환경을 잡아 두었습니다",
        "사용자 환경 파일에 유효한 PATH가 없습니다",
        "devbox setup",
    ));
    let path = p.daemon_path();
    for tool in ["git", "claude", "codex", "gh", "node", "pnpm"] {
        out.push(boolean(
            &format!("tool.{tool}"),
            p.which(tool, &path),
            if tool == "git" {
                Status::Fail
            } else {
                Status::Warn
            },
            &format!("{tool} 사용 가능"),
            &format!("{tool}: PATH에서 찾지 못했습니다"),
            if tool == "gh" {
                "PR 만들기를 쓰려면 설치"
            } else {
                "설치했다면 셸 환경 다시 읽기"
            },
        ));
    }
    out.push(boolean(
        "hook.binary",
        p.hook_executable(),
        Status::Fail,
        "상태 알림 실행 파일 사용 가능",
        "상태 알림 실행 파일을 실행할 수 없습니다",
        "devbox 실행 파일 배치를 확인하세요",
    ));
    out.extend(super::extended::checks(p));
    out
}
