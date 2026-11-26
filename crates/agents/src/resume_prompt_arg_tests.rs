//! A `promptArg` profile gets the task's prompt as an argument on its first launch only. A resume
//! runs the command again without it: the tool may have run the prompt already.
use super::*;

#[test]
fn a_resume_never_passes_the_prompt_again() {
    with_fake_tmux(
        "prompt_arg::a_resume_never_passes_the_prompt_again",
        || async {
            let mut f = Ended::new("mine", None).await;
            Arc::get_mut(&mut f.svc).expect("owned service").profiles =
                vec![crate::profile::ToolProfile {
                    id: "mine".into(),
                    label: "Mine".into(),
                    command: "mine --verbose".into(),
                    hooks: crate::profile::Hooks::None,
                    waiting_patterns: vec![],
                    prompt_arg: Some("--prompt".into()),
                }];
            assert_eq!(f.task().await.prompt, "owned prompt");
            f.svc.resume("a").await.expect("resume");
            let script = f.script().await;
            assert!(
                script.lines().any(|line| line == "mine --verbose"),
                "{script}"
            );
            assert!(
                !script.contains("$(cat") && !script.contains("--prompt"),
                "{script}"
            );
            f.close().await;
        },
    );
}
