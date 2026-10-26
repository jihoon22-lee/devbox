use super::*;
fn candidate(bytes: &[u8]) -> Candidate {
    Candidate {
        version: "1.1.0".into(),
        id: 1,
        size: bytes.len() as u64,
        digest: Sha256::digest(bytes).into(),
    }
}
fn downloads(path: &Path) -> usize {
    fs::read_dir(path)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("download-"))
        .count()
}
#[test]
fn ready_proof_revalidates_real_identity_digest_and_cleans_only_owned_directory() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("updates");
    let root = Root::open(&path, AppChannel::Prod).unwrap();
    fs::write(path.join("unrelated"), "keep").unwrap();
    let mut writer = root.pending().unwrap();
    writer.push(b"abc", 3).unwrap();
    let proof = writer.finish(&candidate(b"abc")).unwrap();
    proof.revalidate().unwrap();
    assert_eq!(downloads(&path), 1);
    drop(proof);
    assert_eq!(downloads(&path), 0);
    assert_eq!(fs::read_to_string(path.join("unrelated")).unwrap(), "keep");
}
#[test]
fn incomplete_oversized_or_wrong_digest_never_publish_ready() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("updates");
    let root = Root::open(&path, AppChannel::Prod).unwrap();
    let mut writer = root.pending().unwrap();
    assert_eq!(writer.push(b"abcd", 3), Err(UpdateError::Size));
    drop(writer);
    let mut writer = root.pending().unwrap();
    writer.push(b"ab", 3).unwrap();
    assert!(matches!(
        writer.finish(&candidate(b"abc")),
        Err(UpdateError::Size)
    ));
    let mut writer = root.pending().unwrap();
    writer.push(b"xyz", 3).unwrap();
    assert!(matches!(
        writer.finish(&candidate(b"abc")),
        Err(UpdateError::Digest)
    ));
    assert_eq!(downloads(&path), 0);
}
#[cfg(unix)]
#[test]
fn replacement_and_same_inode_tampering_invalidate_the_ready_proof() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::open(&temp.path().join("updates"), AppChannel::Prod).unwrap();
    let mut writer = root.pending().unwrap();
    writer.push(b"abc", 3).unwrap();
    let proof = writer.finish(&candidate(b"abc")).unwrap();
    let file = proof.dir.path().join("installer.exe");
    fs::write(&file, b"xyz").unwrap();
    assert_eq!(proof.revalidate(), Err(UpdateError::Digest));
    let other = proof.dir.path().join("other");
    fs::write(&other, b"abc").unwrap();
    fs::rename(other, file).unwrap();
    assert_eq!(proof.revalidate(), Err(UpdateError::Storage));
}
#[cfg(unix)]
#[test]
fn symlinked_roots_and_marker_are_rejected_without_touching_target() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("user");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("sentinel"), "keep").unwrap();
    let alias = temp.path().join("updates");
    std::os::unix::fs::symlink(&target, &alias).unwrap();
    assert!(Root::open(&alias, AppChannel::Prod).is_err());
    let root = temp.path().join("other");
    fs::create_dir(&root).unwrap();
    std::os::unix::fs::symlink(target.join("sentinel"), root.join("owner.json")).unwrap();
    assert!(Root::open(&root, AppChannel::Prod).is_err());
    assert_eq!(fs::read_to_string(target.join("sentinel")).unwrap(), "keep");
}
#[test]
fn ready_publication_does_not_replace_an_unexpected_file_and_root_marker_is_rechecked() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("updates");
    let root = Root::open(&path, AppChannel::Prod).unwrap();
    let mut writer = root.pending().unwrap();
    writer.push(b"abc", 3).unwrap();
    let collision = writer.dir.path().join("installer.exe");
    fs::write(&collision, "unexpected").unwrap();
    assert!(matches!(
        writer.finish(&candidate(b"abc")),
        Err(UpdateError::Storage)
    ));
    assert_eq!(fs::read_to_string(collision).unwrap(), "unexpected");
    fs::write(
        path.join("owner.json"),
        br#"{"schema":1,"channel":"accept"}"#,
    )
    .unwrap();
    assert!(root.pending().is_err());
    assert!(root.automatic_due(100000).is_err());
}
/// Models a process exit: every handle is closed, the owned directory and its files remain.
fn exited(pending: Pending) {
    let Pending { file, dir, .. } = pending;
    drop(file.keep().unwrap());
    let _ = dir.keep();
}
fn exited_ready(proof: ReadyProof) {
    let ReadyProof { file, dir, .. } = proof;
    drop(file);
    let _ = dir.keep();
}
#[test]
fn startup_sweep_removes_only_leaked_owned_download_directories() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("updates");
    let root = Root::open(&path, AppChannel::Prod).unwrap();
    // Leaked like a process exit: owners never drop, handles are closed.
    let mut writer = root.pending().unwrap();
    writer.push(b"abc", 3).unwrap();
    let proof = writer.finish(&candidate(b"abc")).unwrap();
    exited_ready(proof);
    let mut partial = root.pending().unwrap();
    partial.push(b"a", 3).unwrap();
    exited(partial);
    let user = path.join("download-user");
    fs::create_dir(&user).unwrap();
    fs::write(user.join("notes.txt"), "keep").unwrap();
    fs::write(path.join("download-file"), "keep").unwrap();
    let stage = path.join("install-stage");
    fs::create_dir(&stage).unwrap();
    fs::write(stage.join("installer.exe"), "keep").unwrap();
    #[cfg(unix)]
    {
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("installer.exe"), "keep").unwrap();
        std::os::unix::fs::symlink(&outside, path.join("download-link")).unwrap();
    }
    assert_eq!(downloads(&path), if cfg!(unix) { 5 } else { 4 });
    assert_eq!(sweep_downloads(&path, AppChannel::Prod), Ok(2));
    assert_eq!(fs::read_to_string(user.join("notes.txt")).unwrap(), "keep");
    assert_eq!(
        fs::read_to_string(path.join("download-file")).unwrap(),
        "keep"
    );
    assert_eq!(
        fs::read_to_string(stage.join("installer.exe")).unwrap(),
        "keep"
    );
    #[cfg(unix)]
    assert_eq!(
        fs::read_to_string(temp.path().join("outside/installer.exe")).unwrap(),
        "keep"
    );
    assert_eq!(sweep_downloads(&path, AppChannel::Prod), Ok(0));
}
#[test]
fn startup_sweep_never_creates_a_root_or_touches_another_channel() {
    let temp = tempfile::tempdir().unwrap();
    let absent = temp.path().join("absent");
    assert_eq!(sweep_downloads(&absent, AppChannel::Prod), Ok(0));
    assert!(!absent.exists());
    let path = temp.path().join("updates");
    let root = Root::open(&path, AppChannel::Accept).unwrap();
    exited(root.pending().unwrap());
    assert!(sweep_downloads(&path, AppChannel::Prod).is_err());
    assert_eq!(downloads(&path), 1);
}
