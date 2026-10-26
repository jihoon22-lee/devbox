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
