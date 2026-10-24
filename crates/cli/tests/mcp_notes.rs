#[path = "mcp_cases/support.rs"]
mod mcp_support;
mod support;
use mcp_support::{Mcp, body, names};
use serde_json::json;
use support::TestDaemon;

#[test]
fn note_reads_stay_in_fixture_vault_and_write_is_hidden_and_rejected_by_default() {
    let mut d = TestDaemon::fixture();
    let vault = d.home.path().join("notes");
    std::fs::create_dir_all(&vault).unwrap();
    std::fs::write(vault.join("a.md"), "owned note").unwrap();
    let outside = d.home.path().join("outside.md");
    std::fs::write(&outside, "outside private fixture").unwrap();
    std::os::unix::fs::symlink(&outside, vault.join("escape.md")).unwrap();
    d.launch();
    let mut m = Mcp::start(&d);
    let listed = names(&m.call(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})));
    assert!(listed.contains(&"devbox_notes_search".into()));
    assert!(listed.contains(&"devbox_note_read".into()));
    assert!(!listed.contains(&"devbox_note_add".into()));
    assert_eq!(
        body(&m.tool("devbox_note_read", json!({"path":"a.md"})))["text"],
        "owned note"
    );
    for path in ["../outside.md", "escape.md"] {
        let reply = m.tool("devbox_note_read", json!({"path":path}));
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        assert!(!reply.to_string().contains("outside private fixture"));
    }
    let denied = m.tool("devbox_note_add", json!({"text":"never write"}));
    assert_eq!(denied["result"]["isError"], true);
    assert!(!vault.join("inbox.md").exists());
    m.finish();
}

#[test]
fn enabled_note_add_appends_to_inbox_or_daily_and_search_is_bounded() {
    let mut d = TestDaemon::fixture();
    d.write_config("[mcp]\nwrite=true\n");
    d.launch();
    let mut m = Mcp::start(&d);
    assert!(
        names(&m.call(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})))
            .contains(&"devbox_note_add".into())
    );
    let added = body(&m.tool("devbox_note_add", json!({"text":"owned capture"})));
    let path = added["path"].as_str().unwrap();
    assert!(
        body(&m.tool("devbox_note_read", json!({"path":path})))["text"]
            .as_str()
            .unwrap()
            .contains("owned capture")
    );
    let daily = body(&m.tool(
        "devbox_note_add",
        json!({"text":"owned daily","daily":true}),
    ));
    assert_ne!(daily["path"], added["path"]);
    let result = body(&m.tool("devbox_notes_search", json!({"text":"owned"})));
    assert!(result["items"].as_array().unwrap().len() <= 20);
    assert!(
        result["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|hit| hit["path"] == path)
    );
    m.finish();
}
