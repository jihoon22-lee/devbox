//! 증분 인덱싱 watcher의 순수 로직. IO·DB 없이 WSL에서 테스트한다.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 정적 구간 동안 quiet이 유지된 경로만 배출하는 디바운스 창.
pub const DEBOUNCE_WINDOW: Duration = Duration::from_millis(800);

/// 파일시스템 이벤트 분류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventClass {
    Create,
    Modify,
    Remove,
    /// 이름 변경(rename)은 OS에 따라 remove+create 쌍 또는 별도 이벤트로 온다.
    /// [설계] 두 경로를 (삭제+생성)으로 각각 처리한다 — 정적 창 안에서
    /// 이전 경로 삭제 + 새 경로 생성이 함께 수렴한다.
    Rename,
    Other,
}

/// `notify` 이벤트 종류를 분류한다. 접근(access)은 무시한다.
pub fn classify_event(kind: &notify::EventKind) -> EventClass {
    use notify::event::{CreateKind, ModifyKind, RemoveKind};
    match kind {
        notify::EventKind::Create(CreateKind::Any | CreateKind::File | CreateKind::Folder) => {
            EventClass::Create
        }
        notify::EventKind::Modify(
            ModifyKind::Any | ModifyKind::Data(_) | ModifyKind::Metadata(_) | ModifyKind::Other,
        ) => EventClass::Modify,
        notify::EventKind::Modify(ModifyKind::Name(_)) => EventClass::Rename,
        notify::EventKind::Remove(RemoveKind::Any | RemoveKind::File | RemoveKind::Folder) => {
            EventClass::Remove
        }
        _ => EventClass::Other,
    }
}

/// 이벤트 경로가 감시 루트 아래인지 문자열 prefix로 판단한다.
/// (canonicalize는 command 레이어에서 수행 — 여기서는 IO 없음)
pub fn is_within_root(root: &str, path: &str) -> bool {
    if path == root {
        return true;
    }
    if let Some(rest) = path.strip_prefix(root) {
        return rest.starts_with('/') || rest.starts_with('\\');
    }
    false
}

/// 내용 인덱싱 대상 판정 (루트 설정·확장자·크기).
#[allow(dead_code)]
pub fn should_index_content(root_content: bool, path: &str, size: u64) -> bool {
    root_content
        && size <= crate::core::content::MAX_FILE_BYTES
        && crate::core::content::is_content_candidate(std::path::Path::new(path))
}

/// 경로별 quiet-period 디바운스.
#[derive(Debug, Default)]
pub struct Debouncer {
    window: Duration,
    pending: HashMap<PathBuf, Instant>,
}

impl Debouncer {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            pending: HashMap::new(),
        }
    }

    /// 이벤트를 기록한다. 같은 경로의 새 이벤트는 quiet 시계를 다시 시작한다.
    pub fn record(&mut self, path: &Path, now: Instant) {
        self.pending.insert(path.to_path_buf(), now);
    }

    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.pending.values().map(|seen| *seen + self.window).min()
    }

    /// 창 동안 quiet이 유지된 경로를 배출한다.
    pub fn take_ready(&mut self, now: Instant) -> Vec<PathBuf> {
        let ready: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, seen)| now.saturating_duration_since(**seen) >= self.window)
            .map(|(path, _)| path.clone())
            .collect();
        for path in &ready {
            self.pending.remove(path);
        }
        ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_mutation_kinds() {
        use notify::event::{CreateKind, DataChange, ModifyKind, RemoveKind};
        assert_eq!(
            classify_event(&notify::EventKind::Create(CreateKind::File)),
            EventClass::Create
        );
        assert_eq!(
            classify_event(&notify::EventKind::Modify(ModifyKind::Data(
                DataChange::Content
            ))),
            EventClass::Modify
        );
        assert_eq!(
            classify_event(&notify::EventKind::Modify(ModifyKind::Name(
                notify::event::RenameMode::Any
            ))),
            EventClass::Rename
        );
        assert_eq!(
            classify_event(&notify::EventKind::Remove(RemoveKind::File)),
            EventClass::Remove
        );
        assert_eq!(
            classify_event(&notify::EventKind::Access(notify::event::AccessKind::Read)),
            EventClass::Other
        );
    }

    #[test]
    fn within_root_prefix() {
        assert!(is_within_root("C:/proj", "C:/proj"));
        assert!(is_within_root("C:/proj", "C:/proj/a.rs"));
        assert!(!is_within_root("C:/proj", "C:/projects/x.rs"));
        assert!(!is_within_root("C:/proj", "C:/proj2/a.rs"));
    }

    #[test]
    fn content_target_respects_root_flag_and_size() {
        assert!(should_index_content(true, "C:/a.md", 10));
        assert!(should_index_content(true, "C:/report.PDF", 10));
        assert!(!should_index_content(false, "C:/a.md", 10));
        assert!(!should_index_content(true, "C:/a.png", 10));
        assert!(!should_index_content(
            true,
            "C:/a.md",
            crate::core::content::MAX_FILE_BYTES + 1
        ));
    }

    #[test]
    fn debounce_coalesces_bursts() {
        let mut d = Debouncer::new(Duration::from_millis(100));
        let start = Instant::now();
        d.record(Path::new("/root/a.rs"), start);
        assert!(d.take_ready(start + Duration::from_millis(99)).is_empty());
        d.record(Path::new("/root/a.rs"), start + Duration::from_millis(99));
        assert!(d.take_ready(start + Duration::from_millis(150)).is_empty());
        assert_eq!(
            d.take_ready(start + Duration::from_millis(199)),
            vec![PathBuf::from("/root/a.rs")]
        );
    }
}
