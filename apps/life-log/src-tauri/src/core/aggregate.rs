use crate::core::models::{GitDay, ProjectCommit};

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
    let since = format!("--since=@{since}");
    let until = format!("--until=@{until}");
    let args = ["log", since.as_str(), until.as_str(), "--pretty=oneline"];
    let result = devbox_git::GitTarget::from_project_path(path).and_then(|target| {
        devbox_git::run_bounded_target(
            &args,
            &target,
            std::time::Duration::from_secs(2),
            256 * 1024,
        )
    });
    match result {
        Ok(out) => out.lines().count() as u32,
        Err(_) => 0,
    }
}
