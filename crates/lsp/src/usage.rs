use crate::InotifyInfo;
use tokio::io::AsyncReadExt;
async fn bounded(path: impl AsRef<std::path::Path>) -> Option<String> {
    let file = tokio::fs::File::open(path).await.ok()?;
    let mut text = String::new();
    file.take(512 * 1024).read_to_string(&mut text).await.ok()?;
    Some(text)
}
pub(crate) async fn memory(pid: u32) -> Option<u64> {
    let status = bounded(format!("/proc/{pid}/status")).await?;
    status
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmRSS:")?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })
        .and_then(|kb| kb.checked_mul(1024))
}
pub(crate) async fn inotify(pids: &[u32]) -> InotifyInfo {
    let mut info = InotifyInfo {
        max_user_instances: bounded("/proc/sys/fs/inotify/max_user_instances")
            .await
            .and_then(|s| s.trim().parse().ok()),
        max_user_watches: bounded("/proc/sys/fs/inotify/max_user_watches")
            .await
            .and_then(|s| s.trim().parse().ok()),
        owned_instances: 0,
        owned_watches: 0,
        usage_complete: true,
    };
    for pid in pids {
        let Ok(mut entries) = tokio::fs::read_dir(format!("/proc/{pid}/fd")).await else {
            info.usage_complete = false;
            continue;
        };
        let mut seen = 0;
        loop {
            let entry = match entries.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(_) => {
                    info.usage_complete = false;
                    break;
                }
            };
            seen += 1;
            if seen > 8192 {
                info.usage_complete = false;
                break;
            }
            if tokio::fs::read_link(entry.path())
                .await
                .ok()
                .is_none_or(|p| p.to_string_lossy() != "anon_inode:inotify")
            {
                continue;
            }
            info.owned_instances += 1;
            if let Some(text) = bounded(format!(
                "/proc/{pid}/fdinfo/{}",
                entry.file_name().to_string_lossy()
            ))
            .await
            {
                info.owned_watches +=
                    text.lines().filter(|l| l.starts_with("inotify ")).count() as u64;
                if text.len() >= 512 * 1024 {
                    info.usage_complete = false;
                }
            } else {
                info.usage_complete = false;
            }
        }
    }
    info
}
