use std::{fs, os::unix::fs::PermissionsExt, process::Command};

/// Return an error after real checkout has left resources, in an isolated process.
pub(super) fn run_child(test: &str) -> bool {
    if std::env::var_os("DEVBOX_RESOURCE_CHECKOUT_FLAG").is_some() {
        return false;
    }
    let root = tempfile::tempdir().expect("owned wrapper");
    let real_git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("Git path");
    assert!(real_git.status.success());
    let wrapper = root.path().join("git");
    fs::write(
        &wrapper,
        r#"#!/bin/sh
checkout=; previous=; target=; current_repo=
for argument do
 if [ "$previous:$argument" = worktree:add ]; then checkout=1; fi
 if [ "$previous" = -- ]; then target=$argument; fi
 if [ "$previous" = -C ]; then current_repo=$argument; fi
 previous=$argument
done
"$DEVBOX_RESOURCE_REAL_GIT" "$@" || exit $?
if [ "$checkout" = 1 ] && [ -f "$DEVBOX_RESOURCE_CHECKOUT_FLAG" ]; then
 repo=$(cat "$DEVBOX_RESOURCE_CHECKOUT_FLAG")
 if [ "$current_repo" = "$repo" ] && [ "$(dirname "$target")" = "$repo-agents" ]; then
  printf owned-checkout-data > "$target/checkout-created"
  echo 'owned checkout response failure' >&2
  exit 3
 fi
fi
"#,
    )
    .expect("owned wrapper");
    fs::set_permissions(wrapper, fs::Permissions::from_mode(0o700)).expect("wrapper mode");
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .args([test, "--exact", "--nocapture"])
        .env("DEVBOX_RESOURCE_CHECKOUT_FLAG", root.path().join("repo"))
        .env(
            "DEVBOX_RESOURCE_REAL_GIT",
            String::from_utf8(real_git.stdout).expect("Git path").trim(),
        )
        .env(
            "PATH",
            format!(
                "{}:{}",
                root.path().display(),
                std::env::var("PATH").expect("PATH")
            ),
        )
        .output()
        .expect("owned child");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"),
        "child must execute exactly the selected test"
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
    true
}
