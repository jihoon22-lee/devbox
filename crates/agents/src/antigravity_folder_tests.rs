//! The hooks folder beside a work folder: made private, only ever the task's own, no symlink followed.
use super::*;
use std::{fs, os::unix::fs::PermissionsExt};

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777
}

#[test]
fn a_first_launch_makes_a_private_folder_with_the_hooks_file() {
    let root = tempfile::tempdir().expect("owned folder");
    let workdir = root.path().join("fix");
    fs::create_dir(&workdir).expect("owned work folder");
    let dir = hooks_dir(workdir.to_str().expect("UTF-8"));
    write_hooks_dir(&dir, "/b/devbox", "a1", true).expect("first launch");
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&dir.join(".agents")), 0o700);
    let file = dir.join(".agents/hooks.json");
    assert_eq!(mode(&file), 0o600);
    assert_eq!(
        fs::read_to_string(&file).expect("hooks"),
        hooks_json("/b/devbox")
    );
    assert_eq!(
        fs::read_to_string(dir.join(OWNER_FILE)).expect("owner"),
        "a1"
    );
    // Nothing went into the work folder.
    assert_eq!(fs::read_dir(&workdir).expect("work folder").count(), 0);
    // A folder that is already there is never taken by a first launch, the task's own included.
    for agent in ["a1", "a2"] {
        let again = write_hooks_dir(&dir, "/b/devbox", agent, true);
        assert_eq!(
            again.expect_err("exists").kind(),
            std::io::ErrorKind::AlreadyExists
        );
    }
}

#[test]
fn a_resume_rewrites_its_own_folder_and_refuses_anothers() {
    let root = tempfile::tempdir().expect("owned folder");
    let dir = root.path().join("fix.agy-hooks");
    write_hooks_dir(&dir, "/old/devbox", "a1", true).expect("first launch");
    write_hooks_dir(&dir, "/new/devbox", "a1", false).expect("resume");
    let file = dir.join(".agents/hooks.json");
    assert_eq!(
        fs::read_to_string(&file).expect("hooks"),
        hooks_json("/new/devbox")
    );
    assert_eq!(mode(&file), 0o600);
    // The folder was removed meanwhile: the resume makes it again.
    fs::remove_dir_all(&dir).expect("owned folder");
    write_hooks_dir(&dir, "/new/devbox", "a1", false).expect("resume without folder");
    assert!(file.is_file());
    // Another task's folder, and one without a task's name, stay as they are.
    let refused = write_hooks_dir(&dir, "/x/devbox", "a2", false);
    assert_eq!(
        refused.expect_err("another task").kind(),
        std::io::ErrorKind::AlreadyExists
    );
    fs::remove_file(dir.join(OWNER_FILE)).expect("owned file");
    assert!(write_hooks_dir(&dir, "/x/devbox", "a1", false).is_err());
    assert_eq!(
        fs::read_to_string(&file).expect("hooks"),
        hooks_json("/new/devbox")
    );
}

#[test]
fn symlinks_are_never_followed() {
    let root = tempfile::tempdir().expect("owned folder");
    let elsewhere = root.path().join("elsewhere");
    fs::create_dir(&elsewhere).expect("owned target");
    // The hooks folder's name is a symlink.
    let dir = root.path().join("fix.agy-hooks");
    std::os::unix::fs::symlink(&elsewhere, &dir).expect("owned symlink");
    for fresh in [true, false] {
        assert!(write_hooks_dir(&dir, "/b/devbox", "a1", fresh).is_err());
    }
    assert_eq!(fs::read_dir(&elsewhere).expect("target").count(), 0);
    assert!(
        !remove_hooks_dir(root.path().join("fix").to_str().expect("UTF-8"), "a1")
            .expect("not removed")
    );
    assert!(elsewhere.is_dir());
    fs::remove_file(&dir).expect("owned symlink");
    // The hooks file is replaced by a symlink after the first launch.
    write_hooks_dir(&dir, "/b/devbox", "a1", true).expect("first launch");
    let file = dir.join(".agents/hooks.json");
    let target = elsewhere.join("target.json");
    fs::write(&target, "untouched").expect("owned target");
    fs::remove_file(&file).expect("owned file");
    std::os::unix::fs::symlink(&target, &file).expect("owned symlink");
    write_hooks_dir(&dir, "/b/devbox", "a1", false).expect("resume");
    assert_eq!(fs::read_to_string(&target).expect("target"), "untouched");
    assert!(fs::symlink_metadata(&file).expect("file").is_file());
    // A parent that is a symlink.
    let linked = root.path().join("linked");
    std::os::unix::fs::symlink(&elsewhere, &linked).expect("owned symlink");
    assert!(write_hooks_dir(&linked.join("fix.agy-hooks"), "/b/devbox", "a1", true).is_err());
    // Refused before anything is made on the other side of the link.
    assert!(!elsewhere.join("fix.agy-hooks").exists());
}

#[test]
fn a_file_in_a_folders_place_and_a_changed_owner_file_are_refused() {
    let root = tempfile::tempdir().expect("owned folder");
    let workdir = root.path().join("fix");
    let workdir = workdir.to_str().expect("UTF-8");
    let dir = hooks_dir(workdir);
    // A regular file of the hooks folder's name.
    fs::write(&dir, "user file").expect("owned file");
    for fresh in [true, false] {
        assert!(write_hooks_dir(&dir, "/b/devbox", "a1", fresh).is_err());
    }
    assert!(!remove_hooks_dir(workdir, "a1").expect("not a folder"));
    assert_eq!(fs::read_to_string(&dir).expect("file"), "user file");
    fs::remove_file(&dir).expect("owned file");
    // `.agents` replaced by a file after the first launch.
    write_hooks_dir(&dir, "/b/devbox", "a1", true).expect("first launch");
    fs::remove_dir_all(dir.join(".agents")).expect("owned folder");
    fs::write(dir.join(".agents"), "agent file").expect("owned file");
    assert!(write_hooks_dir(&dir, "/b/devbox", "a1", false).is_err());
    assert_eq!(
        fs::read_to_string(dir.join(".agents")).expect("file"),
        "agent file"
    );
    // The owner file names the task exactly: anything else is not this task's folder.
    for changed in ["a1\n", "a10", "", "a"] {
        fs::write(dir.join(OWNER_FILE), changed).expect("owned file");
        assert!(
            !remove_hooks_dir(workdir, "a1").expect("kept"),
            "{changed:?}"
        );
        assert!(
            write_hooks_dir(&dir, "/b/devbox", "a1", false).is_err(),
            "{changed:?}"
        );
    }
    fs::write(dir.join(OWNER_FILE), "x".repeat(300)).expect("owned file");
    assert!(!remove_hooks_dir(workdir, &"x".repeat(300)).expect("kept"));
    assert!(dir.is_dir());
}

#[test]
fn only_the_tasks_own_folder_is_removed() {
    let root = tempfile::tempdir().expect("owned folder");
    let workdir = root.path().join("fix");
    let workdir = workdir.to_str().expect("UTF-8");
    let dir = hooks_dir(workdir);
    // Nothing there.
    assert!(!remove_hooks_dir(workdir, "a1").expect("absent"));
    write_hooks_dir(&dir, "/b/devbox", "a1", true).expect("first launch");
    // What the agent wrote there goes with it.
    fs::write(dir.join("notes.md"), "agent notes").expect("owned file");
    assert!(!remove_hooks_dir(workdir, "a2").expect("another task"));
    assert!(dir.is_dir());
    assert!(remove_hooks_dir(workdir, "a1").expect("own folder"));
    assert!(fs::symlink_metadata(&dir).is_err());
    // A folder of that name devbox did not make.
    fs::create_dir(&dir).expect("foreign folder");
    fs::write(dir.join("keep.txt"), "user file").expect("owned file");
    assert!(!remove_hooks_dir(workdir, "a1").expect("foreign"));
    assert!(dir.join("keep.txt").is_file());
}
