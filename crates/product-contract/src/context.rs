//! Opaque, native-registered context. Paths and aliases are resolved by the
//! project owner; a renderer-provided path is never proof of project identity.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ExecutionTarget {
    Windows,
    Wsl {
        #[serde(rename = "distroId")]
        distro_id: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectContext {
    pub project_id: String,
    pub worktree_id: String,
    pub target: ExecutionTarget,
    pub revision: u64,
}

fn opaque_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl ProjectContext {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !opaque_id(&self.project_id)
            || !opaque_id(&self.worktree_id)
            || self.revision == 0
            || self.revision > 9_007_199_254_740_991
        {
            return Err("invalid project context");
        }
        if let ExecutionTarget::Wsl { distro_id } = &self.target {
            if !opaque_id(distro_id) {
                return Err("invalid execution target");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_fixture_keeps_target_and_revision_distinct() {
        let context: ProjectContext = serde_json::from_str(include_str!(
            "../../../packages/product-shell/fixtures/project-context.json"
        ))
        .unwrap();
        context.validate().unwrap();
        let mut other = context.clone();
        other.target = ExecutionTarget::Windows;
        assert_ne!(context, other);
        other = context.clone();
        other.revision += 1;
        assert_ne!(context, other);
        other.project_id = r"C:\user-project".into();
        assert!(other.validate().is_err());
        let mut value = serde_json::to_value(context).unwrap();
        value["target"]["path"] = "/same-path-is-not-identity".into();
        assert!(serde_json::from_value::<ProjectContext>(value).is_err());
    }
}
