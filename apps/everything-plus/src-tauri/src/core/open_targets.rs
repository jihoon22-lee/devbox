use devbox_applink::{OpenRequest, OpenTarget};
use devbox_launch::InstalledTarget;
use serde::Serialize;
use std::path::{Component, Path};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EverythingOpenTarget {
    pub id: String,
    pub display_name: String,
}

impl EverythingOpenTarget {
    fn request(&self, path: String) -> OpenRequest {
        OpenRequest {
            target: OpenTarget::Path {
                path,
                line: None,
                column: None,
            },
            from: Some("everything-plus".to_string()),
        }
    }
}

/// Catalog `path` capability와 실제 설치 상태의 교집합을 Everything+ 메뉴용
/// 공개 정보로 줄인다. 실행 파일 경로는 frontend로 보내지 않는다.
pub fn select_open_targets(
    source_app_id: &str,
    path_targets: Vec<InstalledTarget>,
) -> Vec<EverythingOpenTarget> {
    path_targets
        .into_iter()
        .filter(|target| target.id != source_app_id)
        .map(|target| EverythingOpenTarget {
            id: target.id,
            display_name: target.display_name,
        })
        .collect()
}

/// Frontend가 보낸 app id와 검색 결과 경로를 실행 직전에 다시 검증한다.
/// 오류에는 대상 id나 로컬 경로를 포함하지 않아 의도치 않은 경로 노출을 막는다.
pub fn prepare_open_request(
    targets: &[EverythingOpenTarget],
    app_id: &str,
    path: &str,
) -> Result<(String, OpenRequest), &'static str> {
    let normalized_id = app_id.to_ascii_lowercase();
    let target = targets
        .iter()
        .find(|target| target.id == normalized_id)
        .ok_or("사용 가능한 대상 앱이 아닙니다")?;

    let candidate = Path::new(path);
    if path.is_empty()
        || !candidate.is_absolute()
        || candidate
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err("검색 결과 파일 경로가 올바르지 않습니다");
    }
    filesystem::ensure_no_links(candidate).map_err(|_| "검색 결과 파일을 찾을 수 없습니다")?;
    // Re-check the final filesystem object without following a symlink or
    // Windows reparse point.  Search rows are stale by definition, so a path
    // that was safe when indexed must not become an opener escape hatch later.
    filesystem::filesystem_identity(candidate, false)
        .map_err(|_| "검색 결과 파일을 찾을 수 없습니다")?;

    Ok((target.id.clone(), target.request(path.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn installed(id: &str) -> InstalledTarget {
        InstalledTarget {
            id: id.to_string(),
            display_name: format!("Display {id}"),
            executable: PathBuf::from(format!("C:/installed/{id}.exe")),
        }
    }

    fn temp_file() -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "everything-open-target-{}-{unique}",
            std::process::id(),
        ));
        std::fs::write(&path, b"fixture").unwrap();
        path
    }

    #[test]
    fn catalog_order_drives_targets_and_excludes_the_source() {
        let targets = select_open_targets(
            "everything-plus",
            vec![
                installed("code-pad"),
                installed("future-app"),
                installed("everything-plus"),
            ],
        );

        assert_eq!(
            targets,
            vec![
                EverythingOpenTarget {
                    id: "code-pad".into(),
                    display_name: "Display code-pad".into(),
                },
                EverythingOpenTarget {
                    id: "future-app".into(),
                    display_name: "Display future-app".into(),
                },
            ]
        );
    }

    #[test]
    fn prepared_request_uses_only_a_selected_installed_target() {
        let file = temp_file();
        let path = file.to_string_lossy().into_owned();
        let targets = vec![EverythingOpenTarget {
            id: "code-pad".into(),
            display_name: "Code Pad".into(),
        }];

        let (id, request) = prepare_open_request(&targets, "CODE-PAD", &path).unwrap();
        assert_eq!(id, "code-pad");
        assert_eq!(
            request,
            OpenRequest {
                target: OpenTarget::Path {
                    path: path.clone(),
                    line: None,
                    column: None,
                },
                from: Some("everything-plus".into()),
            }
        );

        let _ = std::fs::remove_file(file);
    }

    #[test]
    fn missing_target_and_unsafe_paths_fail_without_echoing_input() {
        let targets = vec![EverythingOpenTarget {
            id: "code-pad".into(),
            display_name: "Code Pad".into(),
        }];
        let secret_id = "missing-secret-app";
        let missing =
            prepare_open_request(&targets, secret_id, "/missing-secret-path").unwrap_err();
        assert_eq!(missing, "사용 가능한 대상 앱이 아닙니다");
        assert!(!missing.contains(secret_id));

        let relative_secret = "../secret/file.txt";
        let invalid = prepare_open_request(&targets, "code-pad", relative_secret).unwrap_err();
        assert_eq!(invalid, "검색 결과 파일 경로가 올바르지 않습니다");
        assert!(!invalid.contains(relative_secret));

        let removed_file = temp_file();
        std::fs::remove_file(&removed_file).unwrap();
        let removed_secret = removed_file.to_string_lossy().into_owned();
        let missing_file = prepare_open_request(&targets, "code-pad", &removed_secret).unwrap_err();
        assert_eq!(missing_file, "검색 결과 파일을 찾을 수 없습니다");
        assert!(!missing_file.contains(&removed_secret));

        let directory_secret = std::env::temp_dir().to_string_lossy().into_owned();
        let directory = prepare_open_request(&targets, "code-pad", &directory_secret).unwrap_err();
        assert_eq!(directory, "검색 결과 파일을 찾을 수 없습니다");
        assert!(!directory.contains(&directory_secret));
    }
}
