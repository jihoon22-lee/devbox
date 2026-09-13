//! Native Knowledge -> Workspace file-object proof. Never Launcher metadata,
//! argv, history, or a renderer-supplied filesystem grant.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Proof {
    pub reference: String,
    pub path: String,
    pub volume: String,
    pub object: String,
    pub context: Option<crate::ProjectContext>,
    pub expires_at_ms: u64,
}
impl Proof {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.expires_at_ms == 0
            || self.expires_at_ms > 9_007_199_254_740_991
            || !crate::commands::opaque_id(&self.reference)
            || self.path.len() > 32768
            || devbox_filesystem::parse_safe_project_path(&self.path).is_none()
            || [&self.volume, &self.object].iter().any(|value| {
                value.is_empty()
                    || value.len() > 16
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
            || self
                .context
                .as_ref()
                .is_some_and(|context| context.validate().is_err())
        {
            return Err("file_reference_invalid");
        }
        Ok(())
    }
    pub fn revision(&self) -> Result<String, &'static str> {
        self.validate()?;
        Ok(Sha256::digest(
            serde_json::to_vec(&(
                &self.reference,
                &self.path,
                &self.volume,
                &self.object,
                &self.context,
            ))
            .map_err(|_| "file_reference_invalid")?,
        )
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
    }
    pub fn matches(&self, identity: devbox_filesystem::FilesystemIdentity) -> bool {
        let (volume, object) = identity.components();
        self.volume == format!("{volume:x}") && self.object == format!("{object:x}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_identity_and_reference_revision_do_not_follow_a_same_named_replacement() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first.txt");
        let second = root.path().join("second.txt");
        std::fs::write(&first, b"first synthetic file").unwrap();
        std::fs::write(&second, b"other synthetic file").unwrap();
        let (_handle, identity) = devbox_filesystem::open_filesystem_object(&first, false).unwrap();
        let (volume, object) = identity.components();
        let proof = Proof {
            reference: "reference-one".into(),
            path: first.to_str().unwrap().into(),
            volume: format!("{volume:x}"),
            object: format!("{object:x}"),
            context: None,
            expires_at_ms: 1000,
        };
        assert!(proof.matches(identity));
        assert!(!proof.matches(devbox_filesystem::filesystem_identity(&second, false).unwrap()));
        let mut changed = proof.clone();
        changed.expires_at_ms = 999;
        assert_eq!(proof.revision().unwrap(), changed.revision().unwrap());
        changed.path = second.to_str().unwrap().into();
        assert_ne!(proof.revision().unwrap(), changed.revision().unwrap());
        changed.volume = "../outside".into();
        assert!(changed.validate().is_err());
    }
}
