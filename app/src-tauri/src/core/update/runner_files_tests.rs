use super::*;
pub(crate) fn inputs() -> (TempDir, PathBuf, SealedSource, Binding, ProcessIdentity) {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("application")).unwrap();
    let exe = temp.path().join("application/Devbox.exe");
    fs::write(&exe, b"owned runner executable fixture; never executed").unwrap();
    let path = temp.path().join("downloaded.exe");
    let bytes = b"owned verified installer fixture; never executed";
    fs::write(&path, bytes).unwrap();
    let file = locked(&path).unwrap();
    let identity = storage::identity(&file).unwrap();
    let source = SealedSource {
        file,
        path,
        size: bytes.len() as u64,
        digest: Sha256::digest(bytes).into(),
        identity,
    };
    let binding = Binding {
        operation: 7,
        update_generation: 8,
        quit_generation: 9,
        target_version: "1.0.1".into(),
        fixture_install: false,
    };
    (
        temp,
        exe,
        source,
        binding,
        ProcessIdentity {
            pid: 42,
            started: 123,
            session: 1,
        },
    )
}
pub(crate) fn staged() -> (TempDir, Stage) {
    let (temp, exe, source, binding, parent) = inputs();
    let stage = Stage::create(
        &temp.path().join("data/updates"),
        &exe,
        source,
        binding,
        AppChannel::Prod,
        parent,
        &CancellationToken::new(),
    )
    .unwrap();
    (temp, stage)
}
#[test]
fn sealed_ready_source_has_an_independent_cursor_and_stage_outlives_original_proof() {
    let (temp, exe, _unused, binding, parent) = inputs();
    let downloads = Root::open(&temp.path().join("downloads"), AppChannel::Prod).unwrap();
    let bytes = b"verified download bytes";
    let candidate = crate::core::update::release::Candidate {
        version: "1.0.1".into(),
        id: 1,
        size: bytes.len() as u64,
        digest: Sha256::digest(bytes).into(),
    };
    let mut pending = downloads.pending().unwrap();
    pending.push(bytes, candidate.size).unwrap();
    let ready = pending.finish(&candidate).unwrap();
    let mut first = ready.sealed_source().unwrap();
    first.file.read_exact(&mut [0; 3]).unwrap();
    let second = ready.sealed_source().unwrap();
    let original = second.path.clone();
    drop(first);
    let stage = Stage::create(
        &temp.path().join("data/updates"),
        &exe,
        second,
        binding,
        AppChannel::Prod,
        parent,
        &CancellationToken::new(),
    )
    .unwrap();
    drop(ready);
    assert!(!original.exists());
    assert_eq!(fs::read(&stage.manifest.installer).unwrap(), bytes);
    stage.revalidate(&CancellationToken::new()).unwrap();
}
#[test]
fn staged_copy_is_frozen_and_abort_keeps_original_download_and_executable() {
    let (temp, mut stage) = staged();
    let path = stage.manifest.runner_exe.parent().unwrap().to_owned();
    let original = fs::read(temp.path().join("downloaded.exe")).unwrap();
    assert_eq!(fs::read(&stage.manifest.installer).unwrap(), original);
    assert_eq!(
        fs::read(&stage.manifest.runner_exe).unwrap(),
        fs::read(&stage.manifest.parent_exe).unwrap()
    );
    stage.revalidate(&CancellationToken::new()).unwrap();
    let manifest = read_manifest(&mut stage.payload).unwrap();
    assert_eq!(manifest.binding, stage.manifest.binding);
    drop(stage);
    assert!(!path.exists());
    assert_eq!(
        fs::read(temp.path().join("downloaded.exe")).unwrap(),
        original
    );
    assert!(temp.path().join("application/Devbox.exe").exists());
}
#[test]
fn cancellation_wrong_source_hash_or_identity_never_stages_an_installable_runner() {
    for mode in 0..4 {
        let (temp, exe, mut source, binding, parent) = inputs();
        let token = CancellationToken::new();
        match mode {
            0 => token.cancel(),
            1 => source.digest[0] ^= 1,
            2 => source.identity.index += 1,
            _ => source.size += 1,
        }
        assert!(
            Stage::create(
                &temp.path().join("data/updates"),
                &exe,
                source,
                binding,
                AppChannel::Prod,
                parent,
                &token
            )
            .is_err()
        );
        let updates = temp.path().join("data/updates");
        if updates.exists() {
            assert_eq!(fs::read_dir(updates).unwrap().count(), 1);
        }
        assert!(temp.path().join("downloaded.exe").exists());
    }
}
#[test]
fn copy_collision_and_unexpected_entry_are_preserved_instead_of_recursively_deleted() {
    let (temp, _exe, mut source, _binding, _parent) = inputs();
    let root = temp.path().join("updates");
    Root::open(&root, AppChannel::Prod).unwrap();
    let mut owned = OwnedDir::new(&root).unwrap();
    let path = owned.path.clone();
    let collision = path.join("installer.exe");
    fs::write(&collision, b"unowned collision").unwrap();
    assert!(
        copy(
            &mut source.file,
            &collision,
            source.size,
            Some(source.digest),
            &CancellationToken::new(),
            &mut owned
        )
        .is_err()
    );
    drop(owned);
    assert_eq!(fs::read(collision).unwrap(), b"unowned collision");
    let (_temp, stage) = staged();
    let dir = stage.manifest.runner_exe.parent().unwrap().to_owned();
    fs::write(dir.join("user-file.txt"), b"preserve unexpected bytes").unwrap();
    drop(stage);
    assert_eq!(
        fs::read(dir.join("user-file.txt")).unwrap(),
        b"preserve unexpected bytes"
    );
}
#[test]
fn root_ownership_channel_and_internal_fixture_mode_are_required() {
    let (temp, exe, source, binding, parent) = inputs();
    let root = temp.path().join("updates");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("user-file"), b"not updater owned").unwrap();
    assert!(
        Stage::create(
            &root,
            &exe,
            source,
            binding,
            AppChannel::Prod,
            parent,
            &CancellationToken::new()
        )
        .is_err()
    );
    assert_eq!(
        fs::read(root.join("user-file")).unwrap(),
        b"not updater owned"
    );
    let (_temp, stage) = staged();
    assert!(
        stage
            .manifest
            .validate(AppChannel::Accept, &stage.manifest.runner_exe)
            .is_err()
    );
    assert!(
        stage
            .manifest
            .validate(AppChannel::Dev, &stage.manifest.runner_exe)
            .is_err()
    );
    let mut fixture = stage.manifest.clone();
    fixture.binding.fixture_install = true;
    assert!(
        fixture
            .validate(AppChannel::Prod, &fixture.runner_exe)
            .is_err()
    );
    fixture.channel = "accept".into();
    assert!(
        fixture
            .validate(AppChannel::Accept, &fixture.runner_exe)
            .is_ok()
    );
    fixture.binding.fixture_install = false;
    assert!(
        fixture
            .validate(AppChannel::Accept, &fixture.runner_exe)
            .is_err()
    );
}
#[test]
fn immutable_manifest_refuses_path_escape_and_source_proof_mismatch() {
    let (_temp, stage) = staged();
    for mode in 0..4 {
        let mut manifest = stage.manifest.clone();
        match mode {
            0 => manifest.installer = manifest.parent_exe.clone(),
            1 => manifest.nonce = "install-other".into(),
            2 => manifest.source_digest[0] ^= 1,
            _ => manifest.binding.quit_generation = 0,
        }
        assert!(
            manifest
                .validate(AppChannel::Prod, &stage.manifest.runner_exe)
                .is_err()
        );
    }
}
#[cfg(unix)]
#[test]
fn staged_path_replacement_and_symlink_root_are_rejected_and_preserved() {
    let (_temp, stage) = staged();
    let path = stage.manifest.installer.clone();
    let original = fs::read(&path).unwrap();
    fs::remove_file(&path).unwrap();
    fs::write(&path, &original).unwrap();
    assert_eq!(
        stage.revalidate(&CancellationToken::new()),
        Err(RunnerError::Identity)
    );
    drop(stage);
    assert_eq!(fs::read(path).unwrap(), original);
    let (temp, exe, source, binding, parent) = inputs();
    let target = temp.path().join("foreign");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("sentinel"), b"unchanged").unwrap();
    let root = temp.path().join("updates");
    std::os::unix::fs::symlink(&target, &root).unwrap();
    assert!(
        Stage::create(
            &root,
            &exe,
            source,
            binding,
            AppChannel::Prod,
            parent,
            &CancellationToken::new()
        )
        .is_err()
    );
    assert_eq!(fs::read(target.join("sentinel")).unwrap(), b"unchanged");
}
#[cfg(windows)]
#[test]
fn staged_windows_handles_refuse_write_and_delete_until_owned_stage_drops() {
    let (_temp, stage) = staged();
    assert!(
        OpenOptions::new()
            .write(true)
            .open(&stage.manifest.installer)
            .is_err()
    );
    assert!(fs::remove_file(&stage.manifest.installer).is_err());
    assert!(
        OpenOptions::new()
            .write(true)
            .open(&stage.manifest.runner_exe)
            .is_err()
    );
}
