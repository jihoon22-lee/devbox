use super::*;
use devbox_agents::command_path::ShellAnswer;
use futures::future::BoxFuture;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

/// Answers with the paths it was given, or that nothing was found, and counts the lookups.
#[derive(Default)]
struct Shell {
    answers: Vec<(&'static str, PathBuf)>,
    asked: AtomicUsize,
}
impl ShellProbe for Shell {
    fn command_v<'a>(&'a self, program: &'a str) -> BoxFuture<'a, ShellAnswer> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let answer = self
            .answers
            .iter()
            .find(|(name, _)| *name == program)
            .map(|(_, path)| path.clone());
        Box::pin(async move { ShellAnswer::Answered(answer) })
    }
}
fn tool(dir: &Path, name: &str, body: &str) -> PathBuf {
    fs::create_dir_all(dir).expect("owned fixture folder");
    let path = dir.join(name);
    fs::write(&path, body).expect("owned fixture tool");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("owned fixture mode");
    path
}
fn at(found: &[Option<AgentCommand>], tool: AgentTool) -> Option<&AgentCommand> {
    let index = AgentTool::ALL.iter().position(|known| *known == tool);
    found[index.expect("known tool")].as_ref()
}
/// What a task start finds, with the daemon's `PATH` as `path`.
fn start(program: &str, path: &str, shell: &Shell) -> Option<PathBuf> {
    runner::block_on(async { Ok(command_path::resolve(program, OsStr::new(path), shell).await) })
        .expect("doctor runtime")
        .map(|found| found.program)
}

#[test]
fn a_check_finds_a_cli_installed_since_and_the_next_start_uses_it() {
    let root = tempfile::tempdir().expect("owned fixture");
    let bin = root.path().join("bin");
    fs::create_dir(&bin).expect("owned fixture");
    let path = bin.display().to_string();
    let shell = Shell::default();
    // A start before pi is installed finds nothing, and that answer is kept.
    assert_eq!(start("pi", &path, &shell), None);
    let pi = tool(&bin, "pi", "#!/bin/sh\necho 'pi 1.3.0'\n");
    assert_eq!(start("pi", &path, &shell), None);
    // The environment check looks again and shows it...
    let found = look_up(&path, &shell).expect("doctor runtime");
    let shown = at(&found, AgentTool::Pi).expect("pi found");
    assert_eq!(shown.path, pi.display().to_string());
    assert_eq!(shown.version.as_deref(), Some("1.3.0"));
    // ...and the next start uses that answer without asking the shell.
    let asked = shell.asked.load(Ordering::SeqCst);
    assert_eq!(start("pi", &path, &shell), Some(pi));
    assert_eq!(shell.asked.load(Ordering::SeqCst), asked);
}

#[test]
fn each_check_asks_the_clis_for_their_versions_again() {
    let root = tempfile::tempdir().expect("owned fixture");
    let service = root.path().join("service");
    // Only the interactive shell knows this folder. Its opencode is a `#!/usr/bin/env` script
    // whose interpreter sits next to it, the way nvm installs Node CLIs.
    let interactive = root.path().join("interactive bin");
    tool(
        &interactive,
        "fixture-node",
        "#!/bin/sh\nexec /bin/sh \"$@\"\n",
    );
    let opencode = tool(
        &interactive,
        "opencode",
        "#!/usr/bin/env fixture-node\necho 'opencode v1.19.4'\n",
    );
    tool(&service, "codex", "#!/bin/sh\necho 'no version here'\n");
    tool(&service, "pi", "#!/bin/sh\necho 'pi 1.2.0'\n");
    let shell = Shell {
        answers: vec![("opencode", opencode)],
        ..Shell::default()
    };
    let path = service.display().to_string();
    let found = look_up(&path, &shell).expect("doctor runtime");
    assert!(at(&found, AgentTool::Claude).is_none());
    let shown = at(&found, AgentTool::Opencode).expect("opencode found");
    assert!(shown.interactive);
    assert_eq!(shown.version.as_deref(), Some("1.19.4"));
    assert_eq!(
        at(&found, AgentTool::Codex).expect("codex found").version,
        None
    );
    let version = |found: &[Option<AgentCommand>]| {
        at(found, AgentTool::Pi).expect("pi found").version.clone()
    };
    assert_eq!(version(&found).as_deref(), Some("1.2.0"));
    // An upgrade shows on the next check.
    tool(&service, "pi", "#!/bin/sh\necho 'pi 1.3.0'\n");
    let found = look_up(&path, &shell).expect("doctor runtime");
    assert_eq!(version(&found).as_deref(), Some("1.3.0"));
}
