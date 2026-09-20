use crate::core::profile::ProjectProfile;
use devbox_applink::{OpenRequest, OpenTarget};
use devbox_filesystem::{parse_safe_project_path, ProjectPathKind};
use devbox_launch::InstalledTarget;
use serde::Serialize;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OpenPayloadKind {
    Path,
    Workspace,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchOpenTarget {
    pub id: String,
    pub display_name: String,
    pub payload_kind: OpenPayloadKind,
}

impl WorkbenchOpenTarget {
    fn request(&self, path: String) -> OpenRequest {
        let target = match self.payload_kind {
            OpenPayloadKind::Path => OpenTarget::Path {
                path,
                line: None,
                column: None,
            },
            OpenPayloadKind::Workspace => OpenTarget::Workspace { path },
        };
        OpenRequest {
            target,
            from: Some("workbench".to_string()),
        }
    }
}

/// Catalog capability와 실제 설치 executable의 교집합을 profile menu용
/// 공개 정보로 줄인다. 같은 app이 workspace도 받으면 더 구체적인 payload를
/// 우선하고 source app은 제외한다.
pub fn select_open_targets(
    source_app_id: &str,
    path_targets: Vec<InstalledTarget>,
    workspace_targets: Vec<InstalledTarget>,
) -> Vec<WorkbenchOpenTarget> {
    let workspace_ids = workspace_targets
        .iter()
        .map(|target| target.id.clone())
        .collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let mut targets = path_targets
        .into_iter()
        .filter(|target| target.id != source_app_id)
        .map(|target| {
            seen.insert(target.id.clone());
            WorkbenchOpenTarget {
                payload_kind: if workspace_ids.contains(&target.id) {
                    OpenPayloadKind::Workspace
                } else {
                    OpenPayloadKind::Path
                },
                id: target.id,
                display_name: target.display_name,
            }
        })
        .collect::<Vec<_>>();
    targets.extend(
        workspace_targets
            .into_iter()
            .filter(|target| target.id != source_app_id && seen.insert(target.id.clone()))
            .map(|target| WorkbenchOpenTarget {
                id: target.id,
                display_name: target.display_name,
                payload_kind: OpenPayloadKind::Workspace,
            }),
    );
    targets
}

fn safe_windows_path(profile: &ProjectProfile) -> Option<String> {
    profile
        .windows_path
        .as_deref()
        .and_then(parse_safe_project_path)
        .filter(|path| path.kind() != ProjectPathKind::Posix)
        .map(|path| path.into_string())
}

/// 일반 Path handoff와 사용자 요청의 경로 복사는 안전한 Windows 경로를
/// 우선하고, 없으면 안전한 WSL/POSIX profile path로 폴백한다.
pub fn profile_path(profile: &ProjectProfile) -> Result<String, &'static str> {
    safe_windows_path(profile)
        .or_else(|| {
            profile
                .wsl
                .as_ref()
                .and_then(|wsl| parse_safe_project_path(&wsl.path))
                .filter(|path| path.kind() == ProjectPathKind::Posix)
                .map(|path| path.into_string())
        })
        .ok_or("프로필에 안전하게 사용할 프로젝트 경로가 없습니다")
}

fn target_profile_path(
    profile: &ProjectProfile,
    payload_kind: OpenPayloadKind,
) -> Result<String, &'static str> {
    match payload_kind {
        OpenPayloadKind::Path => profile_path(profile),
        OpenPayloadKind::Workspace => safe_windows_path(profile)
            .ok_or("프로필에 안전하게 사용할 Windows workspace 경로가 없습니다"),
    }
}

pub fn actionable_targets(
    profile: &ProjectProfile,
    targets: Vec<WorkbenchOpenTarget>,
) -> Vec<WorkbenchOpenTarget> {
    targets
        .into_iter()
        .filter(|target| target_profile_path(profile, target.payload_kind).is_ok())
        .collect()
}

/// Frontend가 전달한 두 opaque ID를 현재 profile과 현재 설치 capability에
/// 다시 대조한 뒤에만 versioned app-link를 만든다. 오류에는 ID나 path 원문을
/// 포함하지 않는다.
pub fn prepare_open_request(
    profile: &ProjectProfile,
    targets: &[WorkbenchOpenTarget],
    app_id: &str,
) -> Result<(String, OpenRequest), &'static str> {
    let normalized_id = app_id.to_ascii_lowercase();
    let target = targets
        .iter()
        .find(|target| target.id == normalized_id)
        .ok_or("사용 가능한 대상 앱이 아닙니다")?;
    let path = target_profile_path(profile, target.payload_kind)?;
    Ok((target.id.clone(), target.request(path)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::profile::WslProfile;
    use std::path::PathBuf;

    fn installed(id: &str) -> InstalledTarget {
        InstalledTarget {
            id: id.to_string(),
            display_name: format!("Display {id}"),
            executable: PathBuf::from(format!("C:/installed/{id}.exe")),
        }
    }

    fn profile(windows: Option<&str>, posix: Option<&str>) -> ProjectProfile {
        let mut profile = ProjectProfile::new("devbox");
        profile.id = "profile-1".to_string();
        profile.windows_path = windows.map(str::to_string);
        profile.wsl = posix.map(|path| WslProfile {
            distro: "Ubuntu".to_string(),
            path: path.to_string(),
        });
        profile
    }

    #[test]
    fn catalog_order_drives_targets_and_workspace_is_preferred() {
        let targets = select_open_targets(
            "workbench",
            vec![
                installed("code-pad"),
                installed("future-app"),
                installed("workbench"),
            ],
            vec![installed("code-pad"), installed("workspace-only")],
        );

        assert_eq!(
            targets,
            vec![
                WorkbenchOpenTarget {
                    id: "code-pad".into(),
                    display_name: "Display code-pad".into(),
                    payload_kind: OpenPayloadKind::Workspace,
                },
                WorkbenchOpenTarget {
                    id: "future-app".into(),
                    display_name: "Display future-app".into(),
                    payload_kind: OpenPayloadKind::Path,
                },
                WorkbenchOpenTarget {
                    id: "workspace-only".into(),
                    display_name: "Display workspace-only".into(),
                    payload_kind: OpenPayloadKind::Workspace,
                },
            ]
        );
    }

    #[test]
    fn profile_paths_are_bounded_and_workspace_requires_windows() {
        let dual = profile(
            Some(" C:\\projects\\devbox\\ "),
            Some("/mnt/e/projects/devbox"),
        );
        assert_eq!(profile_path(&dual).unwrap(), "C:\\projects\\devbox");

        let posix = profile(None, Some("/home/me/devbox"));
        assert_eq!(profile_path(&posix).unwrap(), "/home/me/devbox");
        assert_eq!(
            target_profile_path(&posix, OpenPayloadKind::Workspace),
            Err("프로필에 안전하게 사용할 Windows workspace 경로가 없습니다")
        );

        let secret = "C:\\projects\\..\\TOP_SECRET";
        let invalid = profile(Some(secret), None);
        let error = profile_path(&invalid).unwrap_err();
        assert!(!error.contains("TOP_SECRET"));

        let wrong_wsl_kind = profile(None, Some("C:\\projects\\TOP_SECRET"));
        let error = profile_path(&wrong_wsl_kind).unwrap_err();
        assert!(!error.contains("TOP_SECRET"));
    }

    #[test]
    fn request_uses_only_the_selected_installed_target_and_safe_profile_path() {
        let profile = profile(Some("E:\\projects\\devbox"), Some("/mnt/e/projects/devbox"));
        let targets = vec![WorkbenchOpenTarget {
            id: "code-pad".into(),
            display_name: "Code Pad".into(),
            payload_kind: OpenPayloadKind::Workspace,
        }];

        let (id, request) = prepare_open_request(&profile, &targets, "CODE-PAD").unwrap();
        assert_eq!(id, "code-pad");
        assert_eq!(
            request,
            OpenRequest {
                target: OpenTarget::Workspace {
                    path: "E:\\projects\\devbox".into(),
                },
                from: Some("workbench".into()),
            }
        );

        let secret = "missing-secret-target";
        let error = prepare_open_request(&profile, &targets, secret).unwrap_err();
        assert_eq!(error, "사용 가능한 대상 앱이 아닙니다");
        assert!(!error.contains(secret));
    }

    #[test]
    fn actionable_targets_remove_payloads_the_profile_cannot_supply() {
        let posix = profile(None, Some("/home/me/devbox"));
        let targets = actionable_targets(
            &posix,
            vec![
                WorkbenchOpenTarget {
                    id: "code-pad".into(),
                    display_name: "Code Pad".into(),
                    payload_kind: OpenPayloadKind::Workspace,
                },
                WorkbenchOpenTarget {
                    id: "wsl-desktop".into(),
                    display_name: "WSL Desktop".into(),
                    payload_kind: OpenPayloadKind::Path,
                },
            ],
        );

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].id, "wsl-desktop");
    }
}
