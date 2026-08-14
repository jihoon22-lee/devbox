use crate::core::models::{GitDay, ProjectCommit};
use tokio::process::Command;

/// 프로젝트 경로들에서 하루 동안의 커밋 수를 집계한다.
/// `git -C <path> log --since=@<secs> --until=@<secs> --pretty=oneline`의 줄 수.
pub async fn collect_git(projects: &[String], day_start: i64, day_end: i64) -> GitDay {
    let since = day_start.div_euclid(1000);
    let until = day_end.div_euclid(1000);
    let mut projects_out = Vec::new();
    let mut total = 0u32;

    for path in projects {
        let count = git_commit_count(path, since, until).await;
        total += count;
        projects_out.push(ProjectCommit {
            path: path.clone(),
            commits: count,
        });
    }

    GitDay {
        projects: projects_out,
        total_commits: total,
    }
}

async fn git_commit_count(path: &str, since: i64, until: i64) -> u32 {
    let mut cmd = Command::new("git");
    cmd.args([
        "-C",
        path,
        "log",
        &format!("--since=@{since}"),
        &format!("--until=@{until}"),
        "--pretty=oneline",
    ]);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: 콘솔 창 깜빡임 방지
    let output = cmd.output().await;
    match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).lines().count() as u32
        }
        _ => 0,
    }
}
