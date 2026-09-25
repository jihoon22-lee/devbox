use super::*;
use crate::core::journal::{JournalFile, MAX_ENTRIES};

#[test]
fn journal_store_scopes_load_clear_and_explicit_discard() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("note-journal.json");
    let store = NoteJournalStore::new(path.clone());
    store
        .save("/a", "note.md".into(), "a".into(), "r1".into())
        .unwrap();
    assert!(store.load("/b").unwrap().entries.is_empty());
    assert_eq!(store.load("/b").unwrap().other_vault_count, 1);
    assert_eq!(store.load("/a").unwrap().entries[0].content, "a");
    store
        .save("/b", "note.md".into(), "b".into(), "r2".into())
        .unwrap();
    store.clear("/b", "note.md").unwrap();
    assert_eq!(store.load("/b").unwrap().other_vault_count, 1);
    store.discard_other("/b").unwrap();
    assert!(!path.exists());
    store.discard_other("/b").unwrap();
}
#[test]
fn journal_limit_leaves_file_bytes_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("note-journal.json");
    let store = NoteJournalStore::new(path.clone());
    for i in 0..MAX_ENTRIES {
        store
            .save("/a", format!("{i}.md"), "draft".into(), "r".into())
            .unwrap();
    }
    let before = std::fs::read(&path).unwrap();
    assert_eq!(
        store
            .save("/b", "new.md".into(), "new".into(), "r".into())
            .unwrap_err(),
        "journal_limit"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
#[test]
fn journal_damaged_file_is_preserved_on_save_but_not_reset_on_read_or_clear() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("note-journal.json");
    std::fs::write(&path, b"{broken").unwrap();
    let store = NoteJournalStore::new(path.clone());
    assert_eq!(store.load("/a").unwrap_err(), "journal_unavailable");
    assert!(store.clear("/a", "a.md").is_err());
    assert!(store.discard_other("/a").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
    store
        .save("/a", "a.md".into(), "draft".into(), "r".into())
        .unwrap();
    assert_eq!(store.load("/a").unwrap().entries.len(), 1);
    let preserved: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(Result::unwrap)
        .filter(|e| e.file_name().to_string_lossy().contains("corrupt-"))
        .collect();
    assert_eq!(preserved.len(), 1);
    assert_eq!(std::fs::read(preserved[0].path()).unwrap(), b"{broken");
    JournalFile::decode(&std::fs::read(&path).unwrap()).unwrap();
}
#[test]
fn journal_future_schema_and_io_failure_are_never_reset() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("note-journal.json");
    let bytes = br#"{"schemaVersion":9,"future":true}"#;
    std::fs::write(&path, bytes).unwrap();
    let store = NoteJournalStore::new(path.clone());
    assert!(store
        .save("/a", "a.md".into(), "draft".into(), "r".into())
        .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let directory_path = dir.path().join("directory.json");
    std::fs::create_dir(&directory_path).unwrap();
    let unavailable = NoteJournalStore::new(directory_path.clone());
    assert!(unavailable
        .save("/a", "a.md".into(), "draft".into(), "r".into())
        .is_err());
    assert!(directory_path.is_dir());
}
fn database() -> Mutex<Connection> {
    let conn = Connection::open_in_memory().unwrap();
    crate::core::db::migrate(&conn).unwrap();
    crate::core::db::set_setting(&conn, "root", "/configured-a").unwrap();
    Mutex::new(conn)
}
#[test]
fn journal_cached_root_survives_source_disconnect() {
    let dir = tempfile::tempdir().unwrap();
    let store = NoteJournalStore::new(dir.path().join("note-journal.json"));
    let db = database();
    assert_eq!(
        store
            .resolve_with(&db, |_| {
                assert!(db.try_lock().is_ok());
                Ok("/canonical-a".into())
            })
            .unwrap(),
        "/canonical-a"
    );
    let root = store
        .resolve_with(&db, |_| {
            panic!("cached scope must not probe disconnected source")
        })
        .unwrap();
    store
        .save(&root, "a.md".into(), "offline draft".into(), "r".into())
        .unwrap();
    assert_eq!(
        store.load(&root).unwrap().entries[0].content,
        "offline draft"
    );
    store.clear(&root, "a.md").unwrap();
    assert!(store.load(&root).unwrap().entries.is_empty());
}
#[test]
fn journal_does_not_guess_an_unknown_or_changed_root() {
    let dir = tempfile::tempdir().unwrap();
    let store = NoteJournalStore::new(dir.path().join("note-journal.json"));
    let db = database();
    assert!(store
        .resolve_with(&db, |_| Err("journal_unavailable".into()))
        .is_err());
    store
        .resolve_with(&db, |_| Ok("/canonical-a".into()))
        .unwrap();
    crate::core::db::set_setting(&db.lock().unwrap(), "root", "/configured-b").unwrap();
    assert!(store
        .resolve_with(&db, |_| Err("journal_unavailable".into()))
        .is_err());
    assert!(store
        .resolve_with(&db, |_| {
            crate::core::db::set_setting(&db.lock().unwrap(), "root", "/configured-c").unwrap();
            Ok("/canonical-b".into())
        })
        .is_err());
    assert!(!store.path.exists());
}

#[test]
fn journal_wire_inputs_cannot_select_a_foreign_vault() {
    assert!(parse::<SaveInput>(
        serde_json::json!({"path":"a.md","content":"x","baseRevision":"r","vaultRoot":"/foreign"})
    )
    .is_err());
    assert!(
        parse::<ClearInput>(serde_json::json!({"path":"a.md","vaultRoot":"/foreign"})).is_err()
    );
    assert!(parse::<EmptyInput>(serde_json::json!({"vaultRoot":"/foreign"})).is_err());
    assert!(parse::<EmptyInput>(serde_json::json!({})).is_ok());
}
