use devbox_applink::{OpenRequest, OpenTarget};
use serde::Serialize;

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

#[cfg(test)]
mod tests {
    use super::*;

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
