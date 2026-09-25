use super::*;

fn entry(root: &str, path: &str, body: &str) -> JournalEntry {
    JournalEntry {
        vault_root: root.into(),
        path: path.into(),
        content: body.into(),
        base_revision: "r1".into(),
        saved_at_ms: 1,
    }
}
#[test]
fn journal_separates_vaults_and_restores_the_original_view() {
    let mut file = JournalFile::default();
    file.upsert(entry("/a", "Notes/a.md", "first draft"))
        .unwrap();
    assert_eq!(
        file.view("/b"),
        JournalView {
            entries: vec![],
            other_vault_count: 1
        }
    );
    assert_eq!(file.view("/a").entries[0].content, "first draft");
    file.upsert(entry("/b", "Notes/a.md", "second draft"))
        .unwrap();
    assert_eq!(file.entries.len(), 2);
    assert_eq!(file.view("/a").entries[0].content, "first draft");
    assert_eq!(file.view("/b").entries[0].content, "second draft");
    assert_eq!(file.view("/b").other_vault_count, 1);
    let response = serde_json::to_string(&file.view("/b")).unwrap();
    assert!(!response.contains("vaultRoot"));
    assert!(!response.contains("first draft"));
}
#[test]
fn journal_clear_only_removes_the_current_vault_entry() {
    let mut file = JournalFile::default();
    file.upsert(entry("/a", "note.md", "a")).unwrap();
    file.upsert(entry("/b", "note.md", "b")).unwrap();
    assert!(file.remove("/b", "note.md"));
    assert!(!file.remove("/b", "note.md"));
    assert_eq!(file.entries, vec![entry("/a", "note.md", "a")]);
}
#[test]
fn journal_limit_preserves_all_existing_entries_and_allows_updates() {
    let mut file = JournalFile::default();
    for i in 0..MAX_ENTRIES {
        file.upsert(entry("/a", &format!("{i}.md"), "draft"))
            .unwrap();
    }
    let before = file.clone();
    assert_eq!(
        file.upsert(entry("/b", "new.md", "new")),
        Err(JournalError::Limit)
    );
    assert_eq!(file, before);
    file.upsert(entry("/a", "0.md", "updated")).unwrap();
    assert_eq!(file.entries.len(), MAX_ENTRIES);
    assert_eq!(file.view("/a").entries[0].content, "updated");
}
#[test]
fn journal_discard_other_keeps_current_entries() {
    let mut file = JournalFile::default();
    for root in ["/a", "/b", "/c"] {
        file.upsert(entry(root, "note.md", root)).unwrap();
    }
    assert!(file.discard_other("/b"));
    assert_eq!(file.entries, vec![entry("/b", "note.md", "/b")]);
    assert!(!file.discard_other("/b"));
}
#[test]
fn journal_validates_format_and_preserves_unicode_and_schema() {
    let mut file = JournalFile::default();
    file.upsert(entry("/노트", "Notes/한글.md", "본문\u{0001}"))
        .unwrap();
    assert_eq!(JournalFile::decode(&file.encode().unwrap()).unwrap(), file);
    assert_eq!(
        JournalFile::decode(br#"{"schemaVersion":2,"different":true}"#),
        Err(JournalError::FutureSchema)
    );
    assert_eq!(JournalFile::decode(b"{broken"), Err(JournalError::Invalid));
    file.entries.push(file.entries[0].clone());
    assert_eq!(
        JournalFile::decode(&serde_json::to_vec(&file).unwrap()),
        Err(JournalError::Invalid)
    );
    assert_eq!(file.encode(), Err(JournalError::Invalid));
}
#[test]
fn journal_rejects_invalid_or_oversized_entries_without_mutation() {
    let mut file = JournalFile::default();
    for path in ["", " ", "../a.md", "/a.md", "C:\\a.md", "a\0b", "a/../b.md"] {
        assert_eq!(
            file.upsert(entry("/a", path, "draft")),
            Err(JournalError::Invalid),
            "{path:?}"
        );
    }
    assert_eq!(
        file.upsert(entry("", "a.md", "x")),
        Err(JournalError::Invalid)
    );
    assert_eq!(
        file.upsert(entry("/a", "a.md", &"x".repeat(MAX_CONTENT_BYTES + 1))),
        Err(JournalError::Limit)
    );
    assert!(file.entries.is_empty());
}
