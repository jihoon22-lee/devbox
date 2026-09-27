use std::{fs, io::Read, path::Path};
const MAX_PROCESSES: usize = 32_768;
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessSample {
    pub processes: u32,
    pub rss_bytes: u64,
    pub cpu_ticks: u64,
    pub clock_ticks_per_second: u64,
    pub truncated: bool,
}
fn bounded_text(path: &Path, limit: usize) -> Option<String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= limit)
        .then(|| String::from_utf8(bytes).ok())
        .flatten()
}
pub fn sample(proc_root: &Path, root: &Path, page_size: u64, clock_ticks: u64) -> ProcessSample {
    let mut result = ProcessSample {
        processes: 0,
        rss_bytes: 0,
        cpu_ticks: 0,
        clock_ticks_per_second: clock_ticks,
        truncated: false,
    };
    let Ok(entries) = fs::read_dir(proc_root) else {
        result.truncated = true;
        return result;
    };
    let mut visited = 0;
    for entry in entries {
        let Ok(entry) = entry else {
            result.truncated = true;
            continue;
        };
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        if visited == MAX_PROCESSES {
            result.truncated = true;
            break;
        }
        visited += 1;
        let dir = entry.path();
        let Ok(cwd) = fs::read_link(dir.join("cwd")) else {
            continue;
        };
        if !cwd.starts_with(root) {
            continue;
        }
        let Some(stat) = bounded_text(&dir.join("stat"), 64 * 1024) else {
            continue;
        };
        let Some((_, fields)) = stat.rsplit_once(')') else {
            continue;
        };
        let mut fields = fields.split_whitespace();
        let Some(user) = fields.nth(11).and_then(|s| s.parse::<u64>().ok()) else {
            continue;
        };
        let Some(system) = fields.next().and_then(|s| s.parse::<u64>().ok()) else {
            continue;
        };
        let Some(memory) = bounded_text(&dir.join("statm"), 4096) else {
            continue;
        };
        let Some(pages) = memory
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u64>().ok())
        else {
            continue;
        };
        result.processes += 1;
        result.cpu_ticks = result.cpu_ticks.saturating_add(user.saturating_add(system));
        result.rss_bytes = result
            .rss_bytes
            .saturating_add(pages.saturating_mul(page_size));
    }
    result
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::fs;

    fn process(
        proc_root: &Path,
        pid: &str,
        cwd: &Path,
        utime: u64,
        stime: u64,
        resident_pages: u64,
    ) {
        let dir = proc_root.join(pid);
        fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink(cwd, dir.join("cwd")).unwrap();
        // comm may contain spaces and parentheses; fields are read after the last ')'.
        fs::write(
            dir.join("stat"),
            format!("{pid} (node (x)) S 1 1 1 0 -1 0 0 0 0 0 {utime} {stime} 0 0 20 0 1 0 0 0 0\n"),
        )
        .unwrap();
        fs::write(
            dir.join("statm"),
            format!("1000 {resident_pages} 10 1 0 100 0\n"),
        )
        .unwrap();
    }

    #[test]
    fn sums_processes_whose_working_directory_is_inside_the_root() {
        let tmp = tempfile::tempdir().unwrap();
        let proc_root = tmp.path().join("proc");
        let root = tmp.path().join("devbox-fix");
        fs::create_dir_all(root.join("src")).unwrap();
        let elsewhere = tmp.path().join("devbox");
        fs::create_dir_all(&elsewhere).unwrap();
        process(&proc_root, "101", &root, 30, 20, 256);
        process(&proc_root, "102", &root.join("src"), 5, 5, 128);
        process(&proc_root, "103", &elsewhere, 999, 999, 999);
        fs::create_dir_all(proc_root.join("self")).unwrap();
        fs::create_dir_all(proc_root.join("104")).unwrap(); // exited between listing and reading
        let sample = sample(&proc_root, &root, 4096, 100);
        assert_eq!(sample.processes, 2);
        assert_eq!(sample.cpu_ticks, 60);
        assert_eq!(sample.rss_bytes, 384 * 4096);
        assert_eq!(sample.clock_ticks_per_second, 100);
        assert!(!sample.truncated);
    }

    #[test]
    fn unreadable_or_oversized_process_samples_are_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let proc_root = tmp.path().join("proc");
        let root = tmp.path().join("root");
        fs::create_dir(&root).unwrap();
        process(&proc_root, "1", &root, 1, 1, 1);
        process(&proc_root, "2", &root, 1, 1, 1);
        fs::write(proc_root.join("1/stat"), vec![b'x'; 64 * 1024 + 1]).unwrap();
        fs::write(proc_root.join("2/statm"), "not a sample").unwrap();
        assert_eq!(sample(&proc_root, &root, 4096, 100).processes, 0);
        assert!(sample(&tmp.path().join("missing"), &root, 4096, 100).truncated);
    }

    #[test]
    fn a_sibling_with_the_same_prefix_is_not_inside() {
        let tmp = tempfile::tempdir().unwrap();
        let proc_root = tmp.path().join("proc");
        let root = tmp.path().join("devbox");
        let sibling = tmp.path().join("devbox-fix");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&sibling).unwrap();
        process(&proc_root, "201", &sibling, 1, 1, 1);
        assert_eq!(sample(&proc_root, &root, 4096, 100).processes, 0);
    }
}
