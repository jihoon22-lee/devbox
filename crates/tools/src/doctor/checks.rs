use super::Probe;
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
fn check(id: &str, status: Status, message: impl Into<String>, fix: Option<&str>) -> Check {
    Check {
        id: id.into(),
        status,
        message: message.into(),
        fix: fix.map(Into::into),
    }
}
pub fn checks(p: &dyn Probe) -> Vec<Check> {
    let mut out = vec![
        if p.pid1().trim() == "systemd" && p.user_manager_available() {
            check("systemd", Status::Ok, "systemd가 켜져 있습니다", None)
        } else {
            check(
                "systemd",
                Status::Fail,
                "WSL에서 systemd가 꺼져 있습니다",
                Some("/etc/wsl.conf에 [boot] systemd=true를 넣고 wsl --shutdown"),
            )
        },
    ];
    out.push(if p.linger().lines().any(|l| l.trim() == "Linger=yes") {
        check(
            "linger",
            Status::Ok,
            "linger가 켜져 있습니다(사용자 관리자 유지용 보험이며 WSL 종료를 막지는 않습니다)",
            None,
        )
    } else {
        check(
            "linger",
            Status::Warn,
            "linger가 꺼져 있습니다(권장, WSL 종료 방지 기능은 아닙니다)",
            Some("loginctl enable-linger <user>"),
        )
    });
    out.push(match p.tmux_version() {
        Some(v) if tmux_at_least(&v, (3, 2)) => check(
            "tool.tmux",
            Status::Ok,
            "tmux 3.2 이상을 사용할 수 있습니다",
            None,
        ),
        _ => check(
            "tool.tmux",
            Status::Fail,
            "tmux 3.2 이상이 필요합니다",
            Some("sudo apt install tmux 또는 최신 tmux 설치"),
        ),
    });
    let inotify = p.inotify_instances().trim().parse::<u32>().unwrap_or(0);
    out.push(check(
        "inotify",
        if inotify >= 512 {
            Status::Ok
        } else {
            Status::Warn
        },
        format!("max_user_instances={inotify}"),
        (inotify < 512).then_some("fs.inotify.max_user_instances를 1024로 설정"),
    ));
    out.push(if p.env_path_valid() {
        check("env", Status::Ok, "사용자 환경을 잡아 두었습니다", None)
    } else {
        check(
            "env",
            Status::Fail,
            "사용자 환경 파일에 유효한 PATH가 없습니다",
            Some("devbox setup"),
        )
    });
    let path = p.daemon_path();
    for tool in ["git", "claude", "codex", "gh", "node", "pnpm"] {
        let found = p.which(tool, &path);
        out.push(check(
            &format!("tool.{tool}"),
            if found {
                Status::Ok
            } else if tool == "git" {
                Status::Fail
            } else {
                Status::Warn
            },
            if found {
                format!("{tool} 사용 가능")
            } else {
                format!("{tool}: PATH에서 찾지 못했습니다")
            },
            (!found).then_some(if tool == "gh" {
                "PR 만들기를 쓰려면 설치"
            } else {
                "설치했다면 셸 환경 다시 읽기"
            }),
        ));
    }
    let hook_executable = p.hook_executable();
    out.push(check(
        "hook.binary",
        if hook_executable {
            Status::Ok
        } else {
            Status::Fail
        },
        if hook_executable {
            "상태 알림 실행 파일 사용 가능"
        } else {
            "상태 알림 실행 파일을 실행할 수 없습니다"
        },
        (!hook_executable).then_some("devbox 실행 파일 배치를 확인하세요"),
    ));

    out
}
