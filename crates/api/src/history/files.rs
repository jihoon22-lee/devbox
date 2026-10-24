//! `api-history/<YYYY-MM-DD>/<historyId>.json`(0600, 원자적 쓰기)과 1MiB 넘는 본문의 `<historyId>.body`.
use crate::HistoryEntry;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

pub(crate) const INLINE_BODY: usize = 1024 * 1024;
pub(crate) const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// UTC 날짜(proleptic Gregorian, Howard Hinnant `civil_from_days`).
pub(crate) fn day(ms: i64) -> String {
    let z = ms.div_euclid(DAY_MS) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

pub(crate) fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn valid_day(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

pub(crate) fn entry_path(root: &Path, day: &str, id: &str) -> PathBuf {
    root.join(day).join(format!("{id}.json"))
}
pub(crate) fn body_path(root: &Path, day: &str, id: &str) -> PathBuf {
    root.join(day).join(format!("{id}.body"))
}

fn atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("history path"))?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let temp = dir.join(format!(".tmp-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// 본문(이미 가린 것)이 1MiB 이하면 JSON 안에, 넘으면 `.body`에 둔다.
pub(crate) fn write(root: &Path, mut entry: HistoryEntry, body: &[u8]) -> std::io::Result<()> {
    let day = day(entry.sent_at_ms);
    if body.len() > INLINE_BODY {
        atomic(&body_path(root, &day, &entry.id), body)?;
        entry.body_base64 = String::new();
    } else {
        entry.body_base64 = data_encoding::BASE64.encode(body);
    }
    let json = serde_json::to_vec(&entry).map_err(std::io::Error::other)?;
    atomic(&entry_path(root, &day, &entry.id), &json)
}

/// 화면에는 본문을 1MiB까지만 준다.
pub(crate) fn read(root: &Path, day: &str, id: &str) -> std::io::Result<HistoryEntry> {
    let bytes = std::fs::read(entry_path(root, day, id))?;
    let mut entry: HistoryEntry = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
    match std::fs::File::open(body_path(root, day, id)) {
        Ok(file) => {
            use std::io::Read;
            let mut body = Vec::with_capacity(INLINE_BODY);
            file.take(INLINE_BODY as u64).read_to_end(&mut body)?;
            entry.body_base64 = data_encoding::BASE64.encode(&body);
            entry.body_truncated = true;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    Ok(entry)
}

pub(crate) fn days(root: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| valid_day(n))
        .collect();
    out.sort();
    out
}

/// 색인을 다시 만들 때 읽는 기록 전체(깨진 파일은 건너뛴다).
pub(crate) fn scan(root: &Path) -> Vec<HistoryEntry> {
    let mut out = vec![];
    for day in days(root) {
        for file in std::fs::read_dir(root.join(&day))
            .into_iter()
            .flatten()
            .flatten()
        {
            let name = file.file_name().to_string_lossy().into_owned();
            let Some(id) = name.strip_suffix(".json").filter(|id| valid_id(id)) else {
                continue;
            };
            if let Ok(entry) = std::fs::read(file.path())
                .map_err(drop)
                .and_then(|b| serde_json::from_slice::<HistoryEntry>(&b).map_err(drop))
                && entry.id == id
            {
                out.push(entry);
            }
        }
    }
    out
}

/// `cutoff_day`보다 앞선 날짜 폴더를 지운다.
pub(crate) fn prune(root: &Path, cutoff_day: &str) {
    for day in days(root) {
        if day.as_str() < cutoff_day {
            let _ = std::fs::remove_dir_all(root.join(day));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn days_are_utc_calendar_dates() {
        assert_eq!(super::day(0), "1970-01-01");
        assert_eq!(super::day(951_782_400_000), "2000-02-29");
        assert_eq!(super::day(1_790_000_000_000), "2026-09-21");
        assert_eq!(super::day(-1), "1969-12-31");
    }
}
