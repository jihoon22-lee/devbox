use devbox_applink::{OpenRequest, OpenTarget};
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
pub struct RepoOpenTarget {
    pub id: String,
    pub display_name: String,
    pub payload_kind: OpenPayloadKind,
}

impl RepoOpenTarget {
    pub fn request(&self, path: String) -> OpenRequest {
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
            from: Some("repo-manager".to_string()),
        }
    }
}

/// Convert the installed capability intersection into Repo Manager's menu.
///
/// A repository is always a `path`, so the path-capable list is authoritative.
/// If the same installed app also declares `workspace`, prefer that more
/// specific payload. The source app is excluded without maintaining a target
/// allowlist.
pub fn select_repo_open_targets(
    source_app_id: &str,
    path_targets: Vec<InstalledTarget>,
    workspace_targets: Vec<InstalledTarget>,
) -> Vec<RepoOpenTarget> {
    let workspace_ids = workspace_targets
        .into_iter()
        .map(|target| target.id)
        .collect::<HashSet<_>>();

    path_targets
        .into_iter()
        .filter(|target| target.id != source_app_id)
        .map(|target| RepoOpenTarget {
            payload_kind: if workspace_ids.contains(&target.id) {
                OpenPayloadKind::Workspace
            } else {
                OpenPayloadKind::Path
            },
            id: target.id,
            display_name: target.display_name,
        })
        .collect()
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

    #[test]
    fn capability_lists_drive_targets_without_an_app_allowlist() {
        let targets = select_repo_open_targets(
            "repo-manager",
            vec![
                installed("code-pad"),
                installed("future-sixteenth"),
                installed("repo-manager"),
            ],
            vec![installed("code-pad"), installed("workspace-only")],
        );

        assert_eq!(
            targets,
            vec![
                RepoOpenTarget {
                    id: "code-pad".into(),
                    display_name: "Display code-pad".into(),
                    payload_kind: OpenPayloadKind::Workspace,
                },
                RepoOpenTarget {
                    id: "future-sixteenth".into(),
                    display_name: "Display future-sixteenth".into(),
                    payload_kind: OpenPayloadKind::Path,
                },
            ]
        );
    }

    #[test]
    fn targets_absent_from_the_installed_path_list_stay_hidden() {
        let targets = select_repo_open_targets(
            "repo-manager",
            vec![installed("installed-path-app")],
            vec![installed("missing-executable"), installed("workspace-only")],
        );

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].id, "installed-path-app");
        assert_eq!(targets[0].payload_kind, OpenPayloadKind::Path);
    }

    #[test]
    fn request_shape_comes_from_declared_payload_kind() {
        let workspace = RepoOpenTarget {
            id: "any-editor".into(),
            display_name: "Any Editor".into(),
            payload_kind: OpenPayloadKind::Workspace,
        };
        assert_eq!(
            workspace.request("E:\\repos\\devbox".into()),
            OpenRequest {
                target: OpenTarget::Workspace {
                    path: "E:\\repos\\devbox".into()
                },
                from: Some("repo-manager".into())
            }
        );
    }
}
