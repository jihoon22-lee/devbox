use crate::core::db;
use crate::core::entry_actions::{canonical_existing_entry, validated_new_entry};
use crate::core::frontmatter::parse;
use crate::core::store;
use crate::core::wikilink::{
    normalize_link_key, note_link_keys, note_link_target, parse_wikilinks,
};
use rusqlite::Connection;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const MAX_ROOT_ENTRIES: usize = 10_000;
const MAX_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_REWRITTEN_FILES: usize = 200;
const MAX_REWRITTEN_LINKS: usize = 5_000;
const MAX_PREVIEW_LINK_BYTES: usize = 1_024;

const PREVIEW_FAILED: &str = "이름 변경 미리보기를 만들 수 없습니다";
const PLAN_CONFLICT: &str = "미리보기 이후 항목이 변경되었습니다. 다시 미리보기를 실행하세요";
const APPLY_FAILED: &str = "이름 변경을 적용할 수 없습니다";
const ROLLBACK_FAILED: &str =
    "이름 변경을 되돌리는 중 문제가 발생했습니다. Knowledge 폴더를 확인하세요";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RenameDiffItem {
    pub path: String,
    pub before: String,
    pub after: String,
    pub meta: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreview {
    pub plan_id: String,
    pub from: String,
    pub to: String,
    pub is_dir: bool,
    pub items: Vec<RenameDiffItem>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RenameApplied {
    pub from: String,
    pub to: String,
}

#[derive(Default)]
pub struct RenamePlanStore {
    next_id: u64,
    current: Option<RenamePlan>,
}

impl RenamePlanStore {
    pub fn clear(&mut self) {
        self.current = None;
    }

    pub fn next_id(&mut self) -> String {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        format!("rename-{}", self.next_id)
    }

    pub fn put(&mut self, plan: RenamePlan) {
        self.current = Some(plan);
    }

    pub fn take(&mut self, plan_id: &str) -> Result<RenamePlan, String> {
        match self.current.take() {
            Some(plan) if plan.id == plan_id => Ok(plan),
            _ => Err("이름 변경 미리보기가 만료되었습니다".to_string()),
        }
    }

    pub fn discard(&mut self, plan_id: &str) {
        if self.current.as_ref().is_some_and(|plan| plan.id == plan_id) {
            self.current = None;
        }
    }
}

/// 원문 전체를 직렬화하거나 로그로 출력하지 않도록 의도적으로 Debug/Serialize를
/// 구현하지 않는다. 한 번 적용을 시도하면 store에서 제거되는 one-shot plan이다.
pub struct RenamePlan {
    id: String,
    root: PathBuf,
    from: String,
    to: String,
    fingerprint: [u8; 32],
    is_dir: bool,
    rewrites: Vec<FileRewrite>,
    index_documents: Vec<IndexDocument>,
    missing_parent_dirs: Vec<String>,
}

struct FileRewrite {
    path: String,
    final_path: String,
    before: String,
    after: String,
}

struct IndexDocument {
    path: String,
    content: String,
}

struct ScannedNote {
    path: String,
    content: String,
    title: String,
}

struct ScannedText {
    path: String,
    content: String,
}

struct Scan {
    root: PathBuf,
    source: PathBuf,
    destination: PathBuf,
    is_dir: bool,
    fingerprint: [u8; 32],
    notes: Vec<ScannedNote>,
    source_texts: Vec<ScannedText>,
    missing_parent_dirs: Vec<String>,
    source_file_count: usize,
}

struct Replacement {
    start: usize,
    end: usize,
    target: String,
    line: usize,
    before_link: String,
    after_link: String,
}

pub fn prepare(
    root: &Path,
    from: &str,
    to: &str,
    plan_id: String,
) -> Result<(RenamePreview, RenamePlan), String> {
    if from == to {
        return Err("현재 이름과 새 이름이 같습니다".to_string());
    }
    let scan = scan(root, from, to)?;
    if scan.is_dir && is_same_or_child(to, from) {
        return Err("폴더를 자기 자신 아래로 이동할 수 없습니다".to_string());
    }

    let old_keys = link_key_map(
        scan.notes
            .iter()
            .map(|note| (note.path.clone(), note_link_keys(&note.path, &note.title))),
    );
    let future_keys = link_key_map(scan.notes.iter().map(|note| {
        let path = remap_path(&note.path, from, to);
        let title = parse(&note.content)
            .0
            .title
            .unwrap_or_else(|| db::default_title(&path));
        (path.clone(), note_link_keys(&path, &title))
    }));

    let moved_notes = scan
        .notes
        .iter()
        .filter(|note| is_same_or_child(&note.path, from))
        .map(|note| note.path.clone())
        .collect::<BTreeSet<_>>();
    for current_path in &moved_notes {
        let future_path = remap_path(current_path, from, to);
        let canonical_target = note_link_target(&future_path);
        let Some(canonical_key) = normalize_link_key(&canonical_target) else {
            return Err("새 경로로 안전한 위키링크를 만들 수 없습니다".to_string());
        };
        if uniquely_resolved(&future_keys, &canonical_key) != Some(future_path.as_str()) {
            return Err(
                "새 경로의 위키링크가 다른 노트와 충돌합니다. 이름을 다시 선택하세요".to_string(),
            );
        }
    }

    let mut rewrites = Vec::new();
    let mut rewritten_links = 0_usize;
    for note in &scan.notes {
        let mut replacements = Vec::new();
        for link in parse_wikilinks(&note.content) {
            let Some(key) = link.target_key.as_deref() else {
                continue;
            };
            let Some(current_target) = uniquely_resolved(&old_keys, key) else {
                continue;
            };
            if !moved_notes.contains(current_target) {
                continue;
            }
            let future_target = remap_path(current_target, from, to);
            if uniquely_resolved(&future_keys, key) == Some(future_target.as_str()) {
                continue;
            }

            let new_target = note_link_target(&future_target);
            let Some(new_key) = normalize_link_key(&new_target) else {
                return Err("새 경로로 안전한 위키링크를 만들 수 없습니다".to_string());
            };
            if uniquely_resolved(&future_keys, &new_key) != Some(future_target.as_str()) {
                return Err(
                    "새 경로의 위키링크가 다른 노트와 충돌합니다. 이름을 다시 선택하세요"
                        .to_string(),
                );
            }

            let (start, end) = target_byte_range(&note.content, link.from_byte, link.to_byte)
                .ok_or_else(|| PREVIEW_FAILED.to_string())?;
            let before_link = note
                .content
                .get(link.from_byte..link.to_byte)
                .ok_or_else(|| PREVIEW_FAILED.to_string())?;
            let after_link = replace_link_target(before_link, &new_target)
                .ok_or_else(|| PREVIEW_FAILED.to_string())?;
            replacements.push(Replacement {
                start,
                end,
                target: new_target,
                line: link.line,
                before_link: bounded_preview(before_link),
                after_link: bounded_preview(&after_link),
            });
        }
        if replacements.is_empty() {
            continue;
        }
        rewritten_links = rewritten_links.saturating_add(replacements.len());
        if rewrites.len() >= MAX_REWRITTEN_FILES || rewritten_links > MAX_REWRITTEN_LINKS {
            return Err("변경할 위키링크가 너무 많아 이름 변경을 중단했습니다".to_string());
        }
        let after = apply_replacements(&note.content, &replacements)?;
        let final_path = remap_path(&note.path, from, to);
        rewrites.push((
            FileRewrite {
                path: note.path.clone(),
                final_path,
                before: note.content.clone(),
                after,
            },
            replacements,
        ));
    }

    let rewrite_contents = rewrites
        .iter()
        .map(|(rewrite, _)| (rewrite.path.as_str(), rewrite.after.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut index_documents = scan
        .source_texts
        .iter()
        .map(|text| IndexDocument {
            path: remap_path(&text.path, from, to),
            content: rewrite_contents
                .get(text.path.as_str())
                .copied()
                .unwrap_or(&text.content)
                .to_string(),
        })
        .collect::<Vec<_>>();
    index_documents.extend(
        rewrites
            .iter()
            .filter(|(rewrite, _)| !is_same_or_child(&rewrite.path, from))
            .map(|(rewrite, _)| IndexDocument {
                path: rewrite.path.clone(),
                content: rewrite.after.clone(),
            }),
    );

    let mut items = vec![RenameDiffItem {
        path: format!("이름 변경 · {from}"),
        before: from.to_string(),
        after: to.to_string(),
        meta: if scan.is_dir {
            format!("폴더 이동 · 하위 파일 {}개", scan.source_file_count)
        } else {
            "파일 이동".to_string()
        },
    }];
    items.extend(rewrites.iter().map(|(rewrite, replacements)| {
        RenameDiffItem {
            path: rewrite.path.clone(),
            before: replacements
                .iter()
                .map(|replacement| format!("L{}: {}", replacement.line, replacement.before_link))
                .collect::<Vec<_>>()
                .join("\n"),
            after: replacements
                .iter()
                .map(|replacement| format!("L{}: {}", replacement.line, replacement.after_link))
                .collect::<Vec<_>>()
                .join("\n"),
            meta: format!("위키링크 {}개 갱신", replacements.len()),
        }
    }));

    let preview = RenamePreview {
        plan_id: plan_id.clone(),
        from: from.to_string(),
        to: to.to_string(),
        is_dir: scan.is_dir,
        items,
    };
    let plan = RenamePlan {
        id: plan_id,
        root: scan.root,
        from: from.to_string(),
        to: to.to_string(),
        fingerprint: scan.fingerprint,
        is_dir: scan.is_dir,
        rewrites: rewrites.into_iter().map(|(rewrite, _)| rewrite).collect(),
        index_documents,
        missing_parent_dirs: scan.missing_parent_dirs,
    };
    Ok((preview, plan))
}

pub fn apply(
    root: &Path,
    conn: &mut Connection,
    plan: RenamePlan,
) -> Result<RenameApplied, String> {
    let current_root = root.canonicalize().map_err(|_| PLAN_CONFLICT.to_string())?;
    if current_root != plan.root {
        return Err(PLAN_CONFLICT.to_string());
    }
    let current = scan(root, &plan.from, &plan.to).map_err(|_| PLAN_CONFLICT.to_string())?;
    if current.fingerprint != plan.fingerprint
        || current.is_dir != plan.is_dir
        || current.missing_parent_dirs != plan.missing_parent_dirs
    {
        return Err(PLAN_CONFLICT.to_string());
    }
    // Revalidation scan의 note/source text는 여기서 즉시 drop해 apply 동안 preview
    // 원문 두 벌을 함께 보관하지 않는다.
    let Scan {
        root: operation_root,
        source: current_source,
        destination: current_destination,
        ..
    } = current;

    let transaction = conn
        .transaction()
        .map_err(|_| "검색 인덱스 transaction을 시작할 수 없습니다".to_string())?;
    let mut created_dirs = Vec::<PathBuf>::new();
    for rel in &plan.missing_parent_dirs {
        let path = operation_root.join(rel);
        if let Err(_error) = std::fs::create_dir(&path) {
            drop(transaction);
            cleanup_created_dirs(&created_dirs);
            return Err(APPLY_FAILED.to_string());
        }
        created_dirs.push(path);
    }

    let mut written = 0_usize;
    for rewrite in &plan.rewrites {
        if devbox_filesystem::atomic_write(
            operation_root.join(&rewrite.path),
            rewrite.after.as_bytes(),
        )
        .is_err()
        {
            drop(transaction);
            let rolled_back = rollback(&operation_root, &plan, written, false, &created_dirs);
            return Err(if rolled_back {
                APPLY_FAILED
            } else {
                ROLLBACK_FAILED
            }
            .to_string());
        }
        written += 1;
    }

    if std::fs::rename(&current_source, &current_destination).is_err() {
        drop(transaction);
        let rolled_back = rollback(&operation_root, &plan, written, false, &created_dirs);
        return Err(if rolled_back {
            APPLY_FAILED
        } else {
            ROLLBACK_FAILED
        }
        .to_string());
    }

    let index_result = (|| -> Result<(), String> {
        db::remove_docs_under(&transaction, &plan.from)
            .map_err(|_| "검색 인덱스를 갱신할 수 없습니다".to_string())?;
        for document in &plan.index_documents {
            db::index_doc_in_transaction(&transaction, &document.path, &document.content)
                .map_err(|_| "검색 인덱스를 갱신할 수 없습니다".to_string())?;
        }
        Ok(())
    })();
    if let Err(error) = index_result {
        drop(transaction);
        let rolled_back = rollback(&operation_root, &plan, written, true, &created_dirs);
        return Err(if rolled_back {
            error
        } else {
            ROLLBACK_FAILED.to_string()
        });
    }
    if transaction.commit().is_err() {
        let rolled_back = rollback(&operation_root, &plan, written, true, &created_dirs);
        return Err(if rolled_back {
            "검색 인덱스 transaction을 완료할 수 없습니다".to_string()
        } else {
            ROLLBACK_FAILED.to_string()
        });
    }

    Ok(RenameApplied {
        from: plan.from,
        to: plan.to,
    })
}

fn scan(root: &Path, from: &str, to: &str) -> Result<Scan, String> {
    let root = root
        .canonicalize()
        .map_err(|_| PREVIEW_FAILED.to_string())?;
    let source = canonical_existing_entry(&root, from).map_err(str::to_string)?;
    let destination = validated_new_entry(&root, to).map_err(str::to_string)?;
    if destination.exists() {
        return Err("같은 이름의 항목이 이미 존재합니다".to_string());
    }
    let is_dir = source.is_dir();
    if !is_dir && is_markdown(from) != is_markdown(to) {
        return Err("Markdown 파일은 .md 확장자를 유지해야 합니다".to_string());
    }
    if destination
        .parent()
        .is_some_and(|parent| parent.exists() && !parent.is_dir())
    {
        return Err("새 경로의 상위 항목이 폴더가 아닙니다".to_string());
    }
    let entries = store::tree(&root).map_err(|_| PREVIEW_FAILED.to_string())?;
    if entries.len() > MAX_ROOT_ENTRIES {
        return Err("Knowledge 항목이 너무 많아 이름 변경을 중단했습니다".to_string());
    }

    let mut hasher = Sha256::new();
    let mut scanned_bytes = 0_u64;
    let mut notes = Vec::new();
    let mut source_texts = Vec::new();
    let mut source_file_count = 0_usize;
    for (path, entry_is_dir) in entries {
        hash_field(&mut hasher, path.as_bytes());
        hasher.update([u8::from(entry_is_dir)]);
        if entry_is_dir {
            continue;
        }
        let in_source = is_same_or_child(&path, from);
        if in_source {
            source_file_count += 1;
        }
        let markdown = is_markdown(&path);
        if !in_source && !markdown {
            continue;
        }
        let file =
            canonical_existing_entry(&root, &path).map_err(|_| PREVIEW_FAILED.to_string())?;
        let length = file
            .metadata()
            .map_err(|_| PREVIEW_FAILED.to_string())?
            .len();
        scanned_bytes = scanned_bytes.saturating_add(length);
        if scanned_bytes > MAX_SNAPSHOT_BYTES {
            return Err("이름 변경 스냅샷이 64MiB 제한을 초과했습니다".to_string());
        }
        let bytes = std::fs::read(&file).map_err(|_| PREVIEW_FAILED.to_string())?;
        hash_field(&mut hasher, &bytes);
        let text = String::from_utf8(bytes);
        if markdown {
            let content = text.map_err(|_| "Markdown 파일을 읽을 수 없습니다".to_string())?;
            let title = parse(&content)
                .0
                .title
                .unwrap_or_else(|| db::default_title(&path));
            if in_source {
                source_texts.push(ScannedText {
                    path: path.clone(),
                    content: content.clone(),
                });
            }
            notes.push(ScannedNote {
                path,
                content,
                title,
            });
        } else if in_source {
            if let Ok(content) = text {
                source_texts.push(ScannedText { path, content });
            }
        }
    }

    let missing_parent_dirs = missing_parent_dirs(&root, &destination)?;
    Ok(Scan {
        root,
        source,
        destination,
        is_dir,
        fingerprint: hasher.finalize().into(),
        notes,
        source_texts,
        missing_parent_dirs,
        source_file_count,
    })
}

fn link_key_map(
    notes: impl Iterator<Item = (String, Vec<String>)>,
) -> BTreeMap<String, Vec<String>> {
    let mut map = BTreeMap::<String, Vec<String>>::new();
    for (path, keys) in notes {
        for key in keys {
            map.entry(key).or_default().push(path.clone());
        }
    }
    for paths in map.values_mut() {
        paths.sort();
        paths.dedup();
    }
    map
}

fn uniquely_resolved<'a>(map: &'a BTreeMap<String, Vec<String>>, key: &str) -> Option<&'a str> {
    match map.get(key).map(Vec::as_slice) {
        Some([path]) => Some(path),
        _ => None,
    }
}

fn target_byte_range(content: &str, from: usize, to: usize) -> Option<(usize, usize)> {
    let link = content.get(from..to)?;
    let inner = link.strip_prefix("[[")?.strip_suffix("]]")?;
    let target_part = inner
        .split_once('|')
        .map(|(target, _)| target)
        .unwrap_or(inner);
    let leading = target_part.len() - target_part.trim_start().len();
    let trimmed = target_part.trim();
    let start = from + 2 + leading;
    Some((start, start + trimmed.len()))
}

fn replace_link_target(link: &str, target: &str) -> Option<String> {
    let (start, end) = target_byte_range(link, 0, link.len())?;
    let mut result = link.to_string();
    result.replace_range(start..end, target);
    Some(result)
}

fn apply_replacements(content: &str, replacements: &[Replacement]) -> Result<String, String> {
    let mut result = content.to_string();
    for replacement in replacements.iter().rev() {
        if replacement.start > replacement.end || replacement.end > result.len() {
            return Err(PREVIEW_FAILED.to_string());
        }
        result.replace_range(replacement.start..replacement.end, &replacement.target);
    }
    Ok(result)
}

fn rollback(
    root: &Path,
    plan: &RenamePlan,
    written: usize,
    renamed: bool,
    created_dirs: &[PathBuf],
) -> bool {
    let rename_restored =
        !renamed || std::fs::rename(root.join(&plan.to), root.join(&plan.from)).is_ok();
    let mut restored = rename_restored;
    for rewrite in plan.rewrites[..written].iter().rev() {
        let path = if renamed && !rename_restored && is_same_or_child(&rewrite.path, &plan.from) {
            root.join(&rewrite.final_path)
        } else {
            root.join(&rewrite.path)
        };
        if devbox_filesystem::atomic_write(path, rewrite.before.as_bytes()).is_err() {
            restored = false;
        }
    }
    cleanup_created_dirs(created_dirs);
    restored
}

fn cleanup_created_dirs(created_dirs: &[PathBuf]) {
    for directory in created_dirs.iter().rev() {
        let _ = std::fs::remove_dir(directory);
    }
}

fn missing_parent_dirs(root: &Path, destination: &Path) -> Result<Vec<String>, String> {
    let parent = destination
        .parent()
        .ok_or_else(|| PREVIEW_FAILED.to_string())?;
    let relative = parent
        .strip_prefix(root)
        .map_err(|_| PREVIEW_FAILED.to_string())?;
    let mut current = root.to_path_buf();
    let mut missing = Vec::new();
    for component in relative.components() {
        current.push(component);
        if !current.exists() {
            missing.push(
                current
                    .strip_prefix(root)
                    .map_err(|_| PREVIEW_FAILED.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    Ok(missing)
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
}

fn is_markdown(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

fn is_same_or_child(path: &str, parent: &str) -> bool {
    path == parent
        || path
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn remap_path(path: &str, from: &str, to: &str) -> String {
    if path == from {
        return to.to_string();
    }
    path.strip_prefix(from)
        .filter(|rest| rest.starts_with('/'))
        .map(|rest| format!("{to}{rest}"))
        .unwrap_or_else(|| path.to_string())
}

fn bounded_preview(value: &str) -> String {
    if value.len() <= MAX_PREVIEW_LINK_BYTES {
        return value.to_string();
    }
    let mut end = MAX_PREVIEW_LINK_BYTES.saturating_sub('…'.len_utf8());
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}…", &value[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Connection) {
        let root = tempfile::tempdir().unwrap();
        let conn = Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        (root, conn)
    }

    fn write(root: &Path, path: &str, content: &str) {
        let target = root.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, content).unwrap();
    }

    #[test]
    fn file_preview_preserves_alias_and_only_rewrites_links_that_would_break() {
        let (root, _conn) = fixture();
        write(
            root.path(),
            "Notes/Old.md",
            "---\ntitle: Stable\n---\n\n# note\n",
        );
        write(
            root.path(),
            "Projects/source.md",
            "[[Stable]]\n[[ Notes/Old | Keep this alias ]]\n",
        );

        let (preview, plan) = prepare(
            root.path(),
            "Notes/Old.md",
            "Archive/New.md",
            "rename-1".into(),
        )
        .unwrap();

        assert_eq!(preview.items.len(), 2);
        assert!(preview.items[1]
            .before
            .contains("[[ Notes/Old | Keep this alias ]]"));
        assert!(preview.items[1]
            .after
            .contains("[[ Archive/New | Keep this alias ]]"));
        assert!(!preview.items[1].before.contains("[[Stable]]"));
        assert_eq!(
            plan.rewrites[0].after,
            "[[Stable]]\n[[ Archive/New | Keep this alias ]]\n"
        );
    }

    #[test]
    fn folder_apply_updates_internal_and_external_links_and_index() {
        let (root, mut conn) = fixture();
        write(root.path(), "Notes/Area/A.md", "[[Notes/Area/B]]\n");
        write(root.path(), "Notes/Area/B.md", "# B\n");
        write(
            root.path(),
            "Projects/source.md",
            "[[Notes/Area/A|A alias]]\n",
        );
        for path in ["Notes/Area/A.md", "Notes/Area/B.md", "Projects/source.md"] {
            db::index_doc(
                &conn,
                path,
                &std::fs::read_to_string(root.path().join(path)).unwrap(),
            )
            .unwrap();
        }
        let (preview, plan) =
            prepare(root.path(), "Notes/Area", "Archive/Area", "rename-1".into()).unwrap();
        assert!(preview.items[0].meta.contains("하위 파일 2개"));

        let applied = apply(root.path(), &mut conn, plan).unwrap();

        assert!(!root.path().join("Notes/Area").exists());
        assert_eq!(
            std::fs::read_to_string(root.path().join("Archive/Area/A.md")).unwrap(),
            "[[Archive/Area/B]]\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("Projects/source.md")).unwrap(),
            "[[Archive/Area/A|A alias]]\n"
        );
        assert_eq!(applied.to, "Archive/Area");
        assert!(matches!(
            db::analyze_wikilinks(&conn, "[[Archive/Area/A]]").unwrap()[0].resolution,
            db::LinkResolution::Resolved(ref path) if path == "Archive/Area/A.md"
        ));
    }

    #[test]
    fn preview_aborts_when_new_canonical_link_key_is_ambiguous() {
        let (root, _conn) = fixture();
        write(root.path(), "Notes/Old.md", "# Old\n");
        write(
            root.path(),
            "Notes/collision.md",
            "---\ntitle: Notes/New\n---\n",
        );
        let error = prepare(
            root.path(),
            "Notes/Old.md",
            "Notes/New.md",
            "rename-1".into(),
        )
        .err()
        .unwrap();
        assert!(error.contains("충돌"));
        assert!(root.path().join("Notes/Old.md").exists());
    }

    #[test]
    fn preview_rejects_existing_destination_invalid_parent_and_markdown_type_change() {
        let (root, _conn) = fixture();
        write(root.path(), "Notes/Old.md", "# Old\n");
        write(root.path(), "Notes/existing.md", "# Existing\n");
        write(root.path(), "not-a-folder", "text\n");

        assert!(prepare(
            root.path(),
            "Notes/Old.md",
            "Notes/existing.md",
            "rename-1".into(),
        )
        .err()
        .unwrap()
        .contains("이미 존재"));
        let invalid_parent = prepare(
            root.path(),
            "Notes/Old.md",
            "not-a-folder/New.md",
            "rename-2".into(),
        )
        .err()
        .unwrap();
        assert!(!invalid_parent.is_empty());
        assert!(!invalid_parent.contains("not-a-folder"));
        assert!(prepare(
            root.path(),
            "Notes/Old.md",
            "Notes/New.txt",
            "rename-3".into(),
        )
        .err()
        .unwrap()
        .contains(".md"));
        assert!(root.path().join("Notes/Old.md").exists());
    }

    #[test]
    fn apply_rejects_a_changed_snapshot_without_mutating_files() {
        let (root, mut conn) = fixture();
        write(root.path(), "Notes/Old.md", "# Old\n");
        write(root.path(), "source.md", "[[Old]]\n");
        let (_, plan) = prepare(
            root.path(),
            "Notes/Old.md",
            "Notes/New.md",
            "rename-1".into(),
        )
        .unwrap();
        write(root.path(), "source.md", "changed after preview\n");

        let error = apply(root.path(), &mut conn, plan).unwrap_err();

        assert_eq!(error, PLAN_CONFLICT);
        assert!(root.path().join("Notes/Old.md").exists());
        assert!(!root.path().join("Notes/New.md").exists());
        assert_eq!(
            std::fs::read_to_string(root.path().join("source.md")).unwrap(),
            "changed after preview\n"
        );
    }

    #[test]
    fn database_failure_rolls_back_rewrites_rename_and_created_directories() {
        let (root, mut conn) = fixture();
        write(root.path(), "Notes/Old.md", "# Old\n");
        write(root.path(), "source.md", "[[Old|alias]]\n");
        let (_, plan) = prepare(
            root.path(),
            "Notes/Old.md",
            "Created/Nested/New.md",
            "rename-1".into(),
        )
        .unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_docs_insert BEFORE INSERT ON docs BEGIN SELECT RAISE(ABORT, 'fixture'); END;",
        )
        .unwrap();

        let error = apply(root.path(), &mut conn, plan).unwrap_err();

        assert_eq!(error, "검색 인덱스를 갱신할 수 없습니다");
        assert!(root.path().join("Notes/Old.md").exists());
        assert!(!root.path().join("Created").exists());
        assert_eq!(
            std::fs::read_to_string(root.path().join("source.md")).unwrap(),
            "[[Old|alias]]\n"
        );
    }

    #[test]
    fn plan_store_keeps_only_the_exact_current_plan_and_apply_is_one_shot() {
        let (root, _conn) = fixture();
        write(root.path(), "Notes/Old.md", "# Old\n");
        let mut store = RenamePlanStore::default();
        let plan_id = store.next_id();
        let (_, plan) =
            prepare(root.path(), "Notes/Old.md", "Notes/New.md", plan_id.clone()).unwrap();
        store.put(plan);

        store.discard("different-plan");
        assert!(store.take(&plan_id).is_ok());
        assert!(store.take(&plan_id).is_err());
    }
}
