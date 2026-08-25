use devbox_applink::{OpenRequest, OpenTarget};
use devbox_launch::InstalledTarget;
use serde::Serialize;
use std::path::{Component, Path, PathBuf};

const INVALID_ENTRY: &str = "Knowledge 항목 경로가 올바르지 않습니다";
const MISSING_ENTRY: &str = "Knowledge 항목을 찾을 수 없습니다";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeOpenTarget {
    pub id: String,
    pub display_name: String,
}

pub fn select_open_targets(
    source_app_id: &str,
    path_targets: Vec<InstalledTarget>,
) -> Vec<KnowledgeOpenTarget> {
    path_targets
        .into_iter()
        .filter(|target| target.id != source_app_id)
        .map(|target| KnowledgeOpenTarget {
            id: target.id,
            display_name: target.display_name,
        })
        .collect()
}

fn validate_relative(rel: &str) -> Result<(), &'static str> {
    let path = Path::new(rel);
    if rel.is_empty()
        || rel.contains('\\')
        || rel.split('/').any(str::is_empty)
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::Prefix(_)
                    | Component::RootDir
                    | Component::CurDir
                    | Component::ParentDir
            )
        })
    {
        return Err(INVALID_ENTRY);
    }
    Ok(())
}

fn canonical_root(root: &Path) -> Result<PathBuf, &'static str> {
    root.canonicalize().map_err(|_| MISSING_ENTRY)
}

fn reject_symlink_components(root: &Path, rel: &str) -> Result<(), &'static str> {
    let mut cursor = root.to_path_buf();
    for component in Path::new(rel).components() {
        let Component::Normal(segment) = component else {
            return Err(INVALID_ENTRY);
        };
        cursor.push(segment);
        match std::fs::symlink_metadata(&cursor) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(INVALID_ENTRY),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(MISSING_ENTRY),
        }
    }
    Ok(())
}

/// 트리에서 받은 상대 경로를 실행 직전에 canonicalize한다. 루트 밖 symlink와
/// 사라진 항목은 raw path나 OS 오류를 반향하지 않고 거부한다.
pub fn canonical_existing_entry(root: &Path, rel: &str) -> Result<PathBuf, &'static str> {
    validate_relative(rel)?;
    let root = canonical_root(root)?;
    reject_symlink_components(&root, rel)?;
    let entry = root.join(rel).canonicalize().map_err(|_| MISSING_ENTRY)?;
    if entry == root || !entry.starts_with(&root) {
        return Err(INVALID_ENTRY);
    }
    Ok(entry)
}

/// 생성·이름변경 목적지는 아직 존재하지 않을 수 있으므로 가장 가까운 기존 조상을
/// canonicalize해 symlink를 통한 Knowledge root 탈출을 차단한다.
pub fn validated_new_entry(root: &Path, rel: &str) -> Result<PathBuf, &'static str> {
    validate_relative(rel)?;
    let root = canonical_root(root)?;
    reject_symlink_components(&root, rel)?;
    let entry = root.join(rel);
    let mut ancestor = entry.parent().ok_or(INVALID_ENTRY)?;
    while !ancestor.exists() {
        ancestor = ancestor.parent().ok_or(INVALID_ENTRY)?;
    }
    let ancestor = ancestor.canonicalize().map_err(|_| INVALID_ENTRY)?;
    if !ancestor.starts_with(&root) {
        return Err(INVALID_ENTRY);
    }
    Ok(entry)
}

pub fn prepare_open_request(
    targets: &[KnowledgeOpenTarget],
    app_id: &str,
    canonical_entry: &Path,
) -> Result<(String, OpenRequest), &'static str> {
    let normalized_id = app_id.to_ascii_lowercase();
    let target = targets
        .iter()
        .find(|target| target.id == normalized_id)
        .ok_or("사용 가능한 대상 앱이 아닙니다")?;
    let path = canonical_entry
        .to_str()
        .filter(|path| !path.is_empty())
        .ok_or(INVALID_ENTRY)?;
    Ok((
        target.id.clone(),
        OpenRequest {
            target: OpenTarget::Path {
                path: path.to_string(),
                line: None,
                column: None,
            },
            from: Some("knowledge-base".to_string()),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(id: &str) -> InstalledTarget {
        InstalledTarget {
            id: id.to_string(),
            display_name: format!("Display {id}"),
            executable: PathBuf::from(format!("C:/installed/{id}.exe")),
        }
    }

    #[test]
    fn canonical_entry_stays_inside_root_and_new_entry_checks_existing_ancestor() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("Notes")).unwrap();
        std::fs::write(root.path().join("Notes/a.md"), "fixture").unwrap();

        let existing = canonical_existing_entry(root.path(), "Notes/a.md").unwrap();
        assert_eq!(
            existing,
            root.path().join("Notes/a.md").canonicalize().unwrap()
        );
        assert!(canonical_existing_entry(root.path(), "../secret.md").is_err());
        assert!(canonical_existing_entry(root.path(), "Notes\\a.md").is_err());
        assert!(validated_new_entry(root.path(), "Notes//duplicate.md").is_err());
        assert!(validated_new_entry(root.path(), "Notes/new/deep.md").is_ok());
        assert!(validated_new_entry(root.path(), "../outside.md").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn canonical_entry_rejects_symlink_escape_without_echoing_path() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.md"), "secret").unwrap();
        symlink(outside.path(), root.path().join("escape")).unwrap();

        let secret = "escape/secret.md";
        let error = canonical_existing_entry(root.path(), secret).unwrap_err();
        assert_eq!(error, INVALID_ENTRY);
        assert!(!error.contains(secret));
        assert!(validated_new_entry(root.path(), "escape/new.md").is_err());

        let broken_target = outside.path().join("missing");
        symlink(&broken_target, root.path().join("broken")).unwrap();
        assert!(validated_new_entry(root.path(), "broken/new.md").is_err());
        assert!(!broken_target.join("new.md").exists());
    }

    #[test]
    fn target_list_is_catalog_ordered_and_request_requires_selected_target() {
        let targets = select_open_targets(
            "knowledge-base",
            vec![
                installed("code-pad"),
                installed("knowledge-base"),
                installed("workbench"),
            ],
        );
        assert_eq!(
            targets
                .iter()
                .map(|target| target.id.as_str())
                .collect::<Vec<_>>(),
            vec!["code-pad", "workbench"]
        );

        let path = std::env::temp_dir().join("knowledge-entry.md");
        let (_, request) = prepare_open_request(&targets, "CODE-PAD", &path).unwrap();
        assert_eq!(
            request,
            OpenRequest {
                target: OpenTarget::Path {
                    path: path.to_string_lossy().into_owned(),
                    line: None,
                    column: None,
                },
                from: Some("knowledge-base".into()),
            }
        );
        assert_eq!(
            prepare_open_request(&targets, "missing-secret", &path).unwrap_err(),
            "사용 가능한 대상 앱이 아닙니다"
        );
    }
}
