//! Product-only Daily workflow. Preparing a date never creates a note.
use crate::commands::{docs, handoff};
use crate::core::{db, templates, vault::VaultIdentity};
use rusqlite::Connection;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::Manager;

pub const METHODS: &[&str] = &["preview_daily", "save_daily", "discard_daily"];
const TTL_MS: u64 = 300_000;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Preview {
    path: String,
    content: String,
    preview_id: Option<String>,
    exists: bool,
}
struct Pending {
    id: String,
    vault: VaultIdentity,
    path: String,
    content: String,
    issued_at: u64,
}
#[derive(Default)]
pub struct DailyPreviews {
    sequence: u64,
    pending: Option<Pending>,
}
#[derive(Serialize)]
struct Saved {
    path: String,
    indexed: bool,
}
impl DailyPreviews {
    fn prepare(
        &mut self,
        connection: &Connection,
        date: &str,
        now: u64,
    ) -> Result<Preview, String> {
        self.pending = None;
        if !templates::valid_date(date) || now == 0 {
            return Err("component_args_invalid".into());
        }
        let root = docs::resolve_configured_root(connection)?;
        let vault = VaultIdentity::inspect(&root).map_err(|_| "daily_vault_unavailable")?;
        let path = format!("Journal/{date}.md");
        let target = vault
            .new_entry(&path)
            .map_err(|_| "daily_vault_unavailable")?;
        // Metadata only: an existing note is opened through the editor's normal
        // bounded read and dirty-document confirmation, never through a save.
        match std::fs::symlink_metadata(&target) {
            Ok(metadata) => {
                if !metadata.is_file() {
                    return Err("daily_target_exists".into());
                }
                vault
                    .existing_entry(&path)
                    .map_err(|_| "daily_vault_unavailable")?;
                return Ok(Preview {
                    path,
                    content: String::new(),
                    preview_id: None,
                    exists: true,
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("daily_vault_unavailable".into()),
        }
        let parent = target.parent().ok_or("daily_vault_unavailable")?;
        vault
            .lease_existing_directory(parent)
            .map_err(|_| "daily_vault_unavailable")?;
        let content = format!("---\ntags: [daily]\n---\n\n# {date}\n\n");
        self.sequence = self.sequence.checked_add(1).ok_or("daily_preview_stale")?;
        let id = format!("daily-{}", self.sequence);
        self.pending = Some(Pending {
            id: id.clone(),
            vault,
            path: path.clone(),
            content: content.clone(),
            issued_at: now,
        });
        Ok(Preview {
            path,
            content,
            preview_id: Some(id),
            exists: false,
        })
    }
    fn discard(&mut self, id: &str) {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.id == id)
        {
            self.pending = None;
        }
    }
    fn save(&mut self, connection: &Connection, id: &str, now: u64) -> Result<Saved, String> {
        if !self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.id == id)
        {
            return Err("daily_preview_stale".into());
        }
        let pending = self.pending.take().ok_or("daily_preview_stale")?;
        if now < pending.issued_at || now - pending.issued_at > TTL_MS {
            return Err("daily_preview_stale".into());
        }
        let root = docs::resolve_configured_root(connection)?;
        let vault = VaultIdentity::inspect(&root).map_err(|_| "daily_preview_stale")?;
        if vault != pending.vault {
            return Err("daily_preview_stale".into());
        }
        let target = vault
            .new_entry(&pending.path)
            .map_err(|_| "daily_preview_stale")?;
        handoff::write_new_note(&vault, &target, pending.content.as_bytes()).map_err(|error| {
            match error {
                handoff::NewNoteError::Exists => "daily_target_exists",
                handoff::NewNoteError::Stale => "daily_preview_stale",
                handoff::NewNoteError::Storage => "daily_write_failed",
            }
        })?;
        // Markdown is authoritative. A derived-index failure does not remove a
        // successfully published note or present a retry that could duplicate it.
        let indexed = db::index_doc(connection, &pending.path, &pending.content).is_ok();
        Ok(Saved {
            path: pending.path,
            indexed,
        })
    }
}
pub fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Date {
        date: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Approval {
        preview_id: String,
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "daily_preview_stale")?
        .as_millis()
        .try_into()
        .map_err(|_| "daily_preview_stale")?;
    let state = app.state::<Arc<docs::AppState>>();
    let previews = app.state::<Mutex<DailyPreviews>>();
    let mut previews = previews.lock().map_err(|_| "daily_write_failed")?;
    let value = match method {
        "preview_daily" => {
            let Date { date } =
                serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
            let connection = state.db.lock().map_err(|_| "daily_vault_unavailable")?;
            serde_json::to_value(previews.prepare(&connection, &date, now)?)
        }
        "save_daily" => {
            let Approval { preview_id } =
                serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
            let connection = state.db.lock().map_err(|_| "daily_write_failed")?;
            let result = previews.save(&connection, &preview_id, now)?;
            let _ =
                crate::integration::write_snapshot(&connection, state.integration_root.as_deref());
            serde_json::to_value(result)
        }
        "discard_daily" => {
            let Approval { preview_id } =
                serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
            previews.discard(&preview_id);
            Ok(serde_json::Value::Null)
        }
        _ => return Err("component_method_invalid".into()),
    };
    value.map_err(|_| "component_response_invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (tempfile::TempDir, Connection, DailyPreviews) {
        let root = tempfile::tempdir().unwrap();
        crate::core::store::ensure_layout(root.path()).unwrap();
        let connection = Connection::open_in_memory().unwrap();
        db::migrate(&connection).unwrap();
        db::set_setting(&connection, "root", root.path().to_str().unwrap()).unwrap();
        (root, connection, DailyPreviews::default())
    }
    #[test]
    fn date_preview_cancel_and_one_time_save_preserve_existing_bytes() {
        let (root, connection, mut previews) = setup();
        let preview = previews.prepare(&connection, "2024-02-29", 1).unwrap();
        let path = root.path().join(&preview.path);
        let id = preview.preview_id.unwrap();
        assert!(!path.exists());
        previews.discard(&id);
        assert!(previews.save(&connection, &id, 2).is_err());
        let preview = previews.prepare(&connection, "2024-02-29", 3).unwrap();
        let id = preview.preview_id.unwrap();
        assert!(previews.save(&connection, &id, 4).unwrap().indexed);
        let bytes = std::fs::read(&path).unwrap();
        assert!(String::from_utf8(bytes.clone())
            .unwrap()
            .contains("# 2024-02-29"));
        assert!(previews.save(&connection, &id, 5).is_err());
        assert!(
            previews
                .prepare(&connection, "2024-02-29", 6)
                .unwrap()
                .exists
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(previews.prepare(&connection, "2023-02-29", 7).is_err());
    }
    #[test]
    fn replacement_preview_expiry_root_change_and_racing_writer_reject_save() {
        let (root, connection, mut previews) = setup();
        let id = previews
            .prepare(&connection, "2026-09-08", 10)
            .unwrap()
            .preview_id
            .unwrap();
        let next = previews
            .prepare(&connection, "2026-09-09", 11)
            .unwrap()
            .preview_id
            .unwrap();
        assert!(previews.save(&connection, &id, 12).is_err());
        assert!(previews.save(&connection, &next, TTL_MS + 12).is_err());
        let preview = previews.prepare(&connection, "2026-09-08", 20).unwrap();
        let target = root.path().join(&preview.path);
        std::fs::write(&target, "external edit").unwrap();
        assert_eq!(
            previews
                .save(&connection, &preview.preview_id.unwrap(), 21)
                .err()
                .as_deref(),
            Some("daily_target_exists")
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "external edit");
        let id = previews
            .prepare(&connection, "2026-09-09", 22)
            .unwrap()
            .preview_id
            .unwrap();
        let other = tempfile::tempdir().unwrap();
        crate::core::store::ensure_layout(other.path()).unwrap();
        db::set_setting(&connection, "root", other.path().to_str().unwrap()).unwrap();
        assert!(previews.save(&connection, &id, 23).is_err());
        assert!(!other.path().join("Journal/2026-09-09.md").exists());
        assert!(!root.path().join("Journal/2026-09-09.md").exists());
    }
    #[test]
    fn unavailable_vault_does_not_create_default_layout() {
        let root = tempfile::tempdir().unwrap();
        let connection = Connection::open_in_memory().unwrap();
        db::migrate(&connection).unwrap();
        db::set_setting(
            &connection,
            "root",
            root.path().join("offline").to_str().unwrap(),
        )
        .unwrap();
        assert!(DailyPreviews::default()
            .prepare(&connection, "2026-09-08", 1)
            .is_err());
        assert!(!root.path().join("offline").exists());
    }

    #[test]
    fn index_failure_preserves_the_created_markdown_and_consumes_approval() {
        let (root, connection, mut previews) = setup();
        let preview = previews.prepare(&connection, "2026-09-08", 1).unwrap();
        connection.execute_batch("DROP TABLE docs_fts").unwrap();
        let id = preview.preview_id.unwrap();
        let saved = previews.save(&connection, &id, 2).unwrap();
        assert!(!saved.indexed);
        assert_eq!(
            std::fs::read_to_string(root.path().join(saved.path)).unwrap(),
            preview.content
        );
        assert!(previews.save(&connection, &id, 3).is_err());
    }
}
