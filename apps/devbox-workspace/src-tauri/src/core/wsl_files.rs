//! Metadata acknowledged by one native Linux file owner. These records permit
//! session/recovery persistence; only the helper's retained grants permit IO.
use devbox_filesystem::{parse_safe_project_path, ProjectPathKind};
use serde_json::Value;
use std::collections::HashMap;
use workspace_wsl::files::WatchSnapshot;

type Result<T> = std::result::Result<T, &'static str>;
pub fn path(raw: &str) -> Result<String> {
    let parsed = parse_safe_project_path(raw).ok_or("invalid_file_path")?;
    if parsed.kind() != ProjectPathKind::Posix || parsed.as_str() == "/" {
        return Err("file_target_unavailable");
    }
    Ok(parsed.as_str().to_owned())
}
pub fn eligible(root: &str, raw: &str) -> bool {
    let (Ok(root), Ok(candidate)) = (path(root), path(raw)) else {
        return false;
    };
    candidate
        .strip_prefix(&root)
        .is_some_and(|suffix| suffix.starts_with('/'))
}
#[derive(Clone, Default)]
pub struct Documents {
    revisions: HashMap<String, Option<String>>,
    watched: HashMap<String, Option<WatchSnapshot>>,
}
impl Documents {
    /// Keep recovery/session eligibility, but never carry a retired helper's
    /// file grants or watcher observations into its replacement.
    pub fn disconnected(&self) -> Self {
        Self {
            revisions: self
                .revisions
                .keys()
                .map(|path| (path.clone(), None))
                .collect(),
            watched: self
                .watched
                .keys()
                .map(|path| (path.clone(), None))
                .collect(),
        }
    }
    pub fn has_documents(&self) -> bool {
        !self.revisions.is_empty()
    }
    pub fn len(&self) -> usize {
        self.revisions.len()
    }
    pub fn is_empty(&self) -> bool {
        self.revisions.is_empty()
    }
    pub fn has(&self, raw: &str) -> bool {
        path(raw).is_ok_and(|path| self.revisions.contains_key(&path))
    }
    pub fn revision(&self, raw: &str) -> Result<&str> {
        self.revisions
            .get(&path(raw)?)
            .and_then(Option::as_deref)
            .ok_or("file_snapshot_changed")
    }
    pub fn validate_paths(&self, paths: &[String]) -> Result<()> {
        if paths.iter().all(|path| self.has(path)) {
            Ok(())
        } else {
            Err("file_selection_required")
        }
    }
    pub fn record(
        &mut self,
        root: &str,
        expected: &str,
        result: &Value,
        opened: bool,
    ) -> Result<()> {
        let reported = path(result["path"].as_str().ok_or("wsl_protocol_invalid")?)?;
        if !eligible(root, &reported)
            || reported != path(expected)?
            || (!opened && !self.has(expected))
        {
            return Err("wsl_protocol_invalid");
        }
        let revision = match &result["nativeRevision"] {
            Value::String(revision) if workspace_wsl::token(revision) => Some(revision.clone()),
            Value::Null if !opened => None,
            _ => return Err("wsl_protocol_invalid"),
        };
        if !self.revisions.contains_key(&reported) && self.revisions.len() >= 64 {
            return Err("file_limit");
        }
        self.revisions.insert(reported, revision);
        Ok(())
    }
    pub fn renamed(
        &mut self,
        root: &str,
        before: &str,
        new_name: &str,
        result: &Value,
    ) -> Result<()> {
        let before = path(before)?;
        if !self.has(&before)
            || new_name.is_empty()
            || matches!(new_name, "." | "..")
            || new_name.contains(['/', '\\'])
        {
            return Err("invalid_file_path");
        }
        let (parent, _) = before.rsplit_once('/').ok_or("invalid_file_path")?;
        let expected = format!("{parent}/{new_name}");
        if !eligible(root, &expected) || result["path"].as_str() != Some(&expected) {
            return Err("wsl_protocol_invalid");
        }
        let revision = match &result["nativeRevision"] {
            Value::String(revision) if workspace_wsl::token(revision) => Some(revision.clone()),
            Value::Null => None,
            _ => return Err("wsl_protocol_invalid"),
        };
        self.revisions.remove(&before);
        self.revisions.insert(expected.clone(), revision);
        if self.watched.remove(&before).is_some() {
            self.watched.insert(expected, None);
        }
        Ok(())
    }
    pub fn close(&mut self, raw: &str) -> Result<()> {
        let path = path(raw)?;
        self.revisions.remove(&path);
        self.watched.remove(&path);
        Ok(())
    }
    pub fn watch(&mut self, raw: &str) -> Result<()> {
        let path = path(raw)?;
        self.validate_paths(std::slice::from_ref(&path))?;
        self.watched.entry(path).or_default();
        Ok(())
    }
    pub fn watched_paths(&self) -> Vec<String> {
        self.watched
            .keys()
            .filter(|path| self.revision(path).is_ok())
            .cloned()
            .collect()
    }
    pub fn accept_poll(&mut self, root: &str, result: Value) -> Result<Vec<WatchSnapshot>> {
        let snapshots: Vec<WatchSnapshot> =
            serde_json::from_value(result).map_err(|_| "wsl_protocol_invalid")?;
        if snapshots.len() > self.watched.len() {
            return Err("wsl_protocol_invalid");
        }
        let mut seen = std::collections::HashSet::new();
        for snapshot in &snapshots {
            if !eligible(root, &snapshot.path)
                || !self.watched.contains_key(&snapshot.path)
                || !seen.insert(&snapshot.path)
                || snapshot.content_hash.len() != 64
                || !snapshot
                    .content_hash
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || code_pad_lib::commands::file::parse_epoch_nanos(&snapshot.mtime_nanos).is_err()
                || snapshot.size > code_pad_lib::core::guard::MAX_OPENABLE_BYTES
            {
                return Err("wsl_protocol_invalid");
            }
        }
        let mut changed = Vec::new();
        for snapshot in snapshots {
            let previous = self
                .watched
                .get_mut(&snapshot.path)
                .ok_or("wsl_protocol_invalid")?;
            if previous.as_ref() != Some(&snapshot) {
                changed.push(snapshot.clone());
            }
            *previous = Some(snapshot);
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn native_metadata_never_accepts_foreign_paths_or_acknowledges_watcher_changes() {
        let mut docs = Documents::default();
        let revision = uuid::Uuid::new_v4().to_string();
        let opened = json!({"path":"/root/project/한글.txt","nativeRevision":revision});
        docs.record("/root/project", "/root/project/한글.txt", &opened, true)
            .unwrap();
        docs.watch("/root/project/한글.txt").unwrap();
        let snapshot = json!({"path":"/root/project/한글.txt","mtimeNanos":"123","size":2,"contentHash":"a".repeat(64)});
        assert_eq!(
            docs.accept_poll("/root/project", json!([snapshot]))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(docs.revision("/root/project/한글.txt").unwrap(), revision);
        let disconnected = docs.disconnected();
        assert!(disconnected.has("/root/project/한글.txt"));
        assert!(disconnected.revision("/root/project/한글.txt").is_err());
        assert!(disconnected.watched_paths().is_empty());
        let mut reconnected = disconnected;
        reconnected
            .record("/root/project", "/root/project/한글.txt", &opened, true)
            .unwrap();
        assert_eq!(reconnected.watched_paths(), vec!["/root/project/한글.txt"]);
        let mut foreign = snapshot.clone();
        foreign["path"] = json!("/root/project-other/한글.txt");
        assert!(docs.accept_poll("/root/project", json!([foreign])).is_err());
        assert!(docs
            .accept_poll("/root/project", json!([snapshot, snapshot]))
            .is_err());
        assert!(docs
            .record(
                "/root/project",
                "/root/project/a",
                &json!({"path":"C:\\private","nativeRevision":revision}),
                true
            )
            .is_err());
        assert!(!eligible("/root/project", "/root/project-other/a"));
        assert!(!eligible("/root/project", "/root/project/../other/a"));
        docs.record(
            "/root/project",
            "/root/project/한글.txt",
            &json!({"path":"/root/project/한글.txt","nativeRevision":null}),
            false,
        )
        .unwrap();
        assert!(docs.has_documents());
        assert!(docs.revision("/root/project/한글.txt").is_err());
        docs.validate_paths(&["/root/project/한글.txt".into()])
            .unwrap();
        docs.close("/root/project/한글.txt").unwrap();
        assert!(!docs.has_documents());
    }
}
