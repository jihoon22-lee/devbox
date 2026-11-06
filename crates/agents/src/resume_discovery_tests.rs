//! A Codex resume with no thread verified before finds the session as a completion report
//! does: only a primary session with a turn since the launch is the task's.
use super::*;

/// The first launch had a turn, but its thread was never verified, for example because the
/// daemon was down while it ran. Resume finds that session and keeps it.
#[test]
fn codex_resume_without_a_stored_thread_discovers_a_turned_session() {
    with_fake_tmux(
        "resume_discovery_tests::codex_resume_without_a_stored_thread_discovers_a_turned_session",
        || async {
            let f = codex_fixture().await;
            let rollout = f.rollout(
                "primary",
                "user",
                "2026-10-13T00:00:01Z",
                "one",
                "2026-10-13T00:00:02Z",
            );
            end_first_launch(&f).await;
            let unbound = f.svc.launch_binding("a").await.expect("binding");
            assert!(unbound.thread_id.is_none() && unbound.rollout.is_none());
            let resumed = f.svc.resume("a").await.expect("resume");
            let script = resume_script(&f, &current_launch(&f).await);
            assert!(script.contains(" resume 'primary'"), "{script}");
            let binding = f.svc.launch_binding("a").await.expect("binding");
            assert_eq!(binding.thread_id.as_deref(), Some("primary"));
            assert_eq!(binding.rollout, Some(rollout));
            assert_eq!(resumed.session_id.as_deref(), Some("primary"));
            f.svc
                .terminal
                .close_internal(resumed.terminal_id.as_deref().expect("owned terminal"))
                .await
                .expect("remove owned fake terminal");
        },
    );
}

/// With no thread verified before, a session without a turn since the launch is not known to
/// be the task's, and neither is no session: the launch may have ended at Codex's login or
/// trust screen. Resume fails and starts nothing.
#[test]
fn codex_resume_without_a_verifiable_session_fails_without_starting_codex() {
    with_fake_tmux(
        "resume_discovery_tests::codex_resume_without_a_verifiable_session_fails_without_starting_codex",
        || async {
            for case in ["a rollout without a turn", "no rollout"] {
                let f = codex_fixture().await;
                if case == "a rollout without a turn" {
                    let path = f.rollout(
                        "primary",
                        "user",
                        "2026-10-13T00:00:01Z",
                        "one",
                        "2026-10-13T00:00:02Z",
                    );
                    let text = fs::read_to_string(&path).expect("rollout");
                    let (meta, _) = text.split_once('\n').expect("session_meta line");
                    fs::write(&path, format!("{meta}\n")).expect("only session_meta");
                }
                end_first_launch(&f).await;
                let before = f.svc.get("a").await.expect("task");
                let binding = f.svc.launch_binding("a").await.expect("binding");
                assert!(binding.thread_id.is_none(), "{case}");
                let error = f.svc.resume("a").await.expect_err("no verifiable session");
                assert_eq!(
                    serde_json::to_value(match &error {
                        Fail::Domain(e) => e,
                        _ => panic!("expected domain error: {error:?}"),
                    })
                    .expect("error")["code"],
                    "agents.session_metadata",
                    "{case}"
                );
                assert_eq!(f.svc.get("a").await.expect("task"), before, "{case}");
                assert_eq!(
                    f.svc.launch_binding("a").await.expect("binding"),
                    binding,
                    "{case}"
                );
                assert!(
                    resume_files(&f).is_empty(),
                    "{case}: no start script, so no Codex"
                );
            }
        },
    );
}
