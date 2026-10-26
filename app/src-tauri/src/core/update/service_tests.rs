use super::super::{fixture::Fixture, storage::Root};
use super::*;
use sha2::{Digest, Sha256};
use std::fs;
const SOURCE: &str = "1111111111111111111111111111111111111111";
fn setup(delay: Duration) -> (tempfile::TempDir, Arc<Service>) {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input");
    fs::create_dir(&input).unwrap();
    let bytes = vec![42_u8; 100000];
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let metadata = serde_json::to_vec(&serde_json::json!({"tag_name":"v1.1.0","draft":false,"prerelease":false,"assets":[{"id":1,"name":"devbox-1.1.0-setup.exe","state":"uploaded","size":bytes.len(),"digest":format!("sha256:{digest}"),"url":"https://api.github.com/repos/jihoon22-lee/devbox/releases/assets/1","browser_download_url":"https://github.com/jihoon22-lee/devbox/releases/download/v1.1.0/devbox-1.1.0-setup.exe"}]})).unwrap();
    fs::write(input.join("metadata.json"), &metadata).unwrap();
    fs::write(input.join("installer.exe"), bytes).unwrap();
    fs::write(input.join("owner.json"), serde_json::to_vec(&serde_json::json!({"schema":1,"channel":"accept","sourceSha":SOURCE,"currentVersion":"1.0.0","metadataSha256":format!("{:x}",Sha256::digest(&metadata)),"installerSha256":digest})).unwrap()).unwrap();
    let mut fixture = Fixture::open(&input, SOURCE).unwrap();
    fixture.delay = delay;
    let service = Service::fixture(
        AppChannel::Accept,
        Some(temp.path().join("updates")),
        fixture,
        |_| {},
    )
    .unwrap();
    (temp, service)
}
async fn wait(service: &Service, predicate: impl Fn(&Snapshot) -> bool) -> Snapshot {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let value = service.snapshot();
            if predicate(&value) {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn one_rust_operation_survives_window_unmount_and_new_window_can_cancel() {
    let (temp, service) = setup(Duration::from_millis(35));
    let worker = tokio::spawn(service.clone().run());
    let started = service.check(false).unwrap();
    assert_eq!(started.phase, Phase::Checking);
    assert_eq!(service.check(false).unwrap(), started);
    let available = wait(&service, |s| s.phase == Phase::Available).await;
    let downloading = service.download(available.generation).unwrap();
    assert_eq!(downloading.phase, Phase::Downloading);
    assert_eq!(
        service.download(downloading.generation).unwrap(),
        downloading
    );
    let new_window = wait(&service, |s| s.received_bytes > 0).await;
    assert_eq!(new_window.generation, downloading.generation);
    assert_eq!(new_window.current_version, env!("CARGO_PKG_VERSION"));
    let cancelled = service.cancel(new_window.generation).unwrap();
    assert_eq!(cancelled.phase, Phase::Available);
    assert!(cancelled.revision > new_window.revision);
    assert_eq!(
        service.cancel(new_window.generation),
        Err(UpdateError::StaleOperation)
    );
    tokio::time::timeout(Duration::from_secs(1), async {
        while service.active.load(Ordering::SeqCst) != 0 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        fs::read_dir(temp.path().join("updates")).unwrap().count(),
        1
    );
    assert_eq!(service.snapshot(), cancelled);
    worker.abort();
    let _ = worker.await;
}
#[tokio::test]
async fn cancelled_live_writer_exits_before_latest_pending_download_and_stale_progress_is_ignored()
{
    let (temp, service) = setup(Duration::from_millis(50));
    let worker = tokio::spawn(service.clone().run());
    service.check(false).unwrap();
    let candidate = wait(&service, |s| s.phase == Phase::Available).await;
    let first = service.download(candidate.generation).unwrap();
    wait(&service, |s| s.received_bytes > 0).await;
    let mut next = service.cancel(first.generation).unwrap();
    for _ in 0..20 {
        next = service.download(next.generation).unwrap();
        next = service.cancel(next.generation).unwrap();
    }
    let latest = service.download(next.generation).unwrap();
    assert_eq!(service.active.load(Ordering::SeqCst), first.generation);
    service.progress(first.generation, 99999);
    assert_eq!(service.snapshot(), latest);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            assert!(
                fs::read_dir(temp.path().join("updates"))
                    .unwrap()
                    .filter_map(Result::ok)
                    .filter(|e| e.file_name().to_string_lossy().starts_with("download-"))
                    .count()
                    <= 1
            );
            if service.snapshot().phase == Phase::Ready {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    let ready = service.snapshot();
    assert_eq!(ready.generation, latest.generation);
    assert_eq!(ready.received_bytes, 100000);
    service.lock().ready.as_ref().unwrap().revalidate().unwrap();
    assert_eq!(service.download(ready.generation).unwrap(), ready);
    worker.abort();
    let _ = worker.await;
}
#[tokio::test]
async fn automatic_stamp_survives_new_service_and_manual_check_is_explicit() {
    let (temp, service) = setup(Duration::ZERO);
    let worker = tokio::spawn(service.clone().run());
    service.check(true).unwrap();
    let first = wait(&service, |s| s.phase == Phase::Available).await;
    assert_eq!(service.check(true).unwrap(), first);
    let fixture = Fixture::open(&temp.path().join("input"), SOURCE).unwrap();
    let restarted = Service::fixture(
        AppChannel::Accept,
        Some(temp.path().join("updates")),
        fixture,
        |_| {},
    )
    .unwrap();
    let second_worker = tokio::spawn(restarted.clone().run());
    assert_eq!(restarted.check(true).unwrap().phase, Phase::Idle);
    assert_eq!(restarted.check(false).unwrap().phase, Phase::Checking);
    wait(&restarted, |s| s.phase == Phase::Available).await;
    worker.abort();
    second_worker.abort();
    let _ = worker.await;
    let _ = second_worker.await;
}
#[test]
fn public_updater_is_disabled_for_nonprod_or_nonstable_without_io() {
    for (channel, version) in [
        (AppChannel::Prod, "1.0.0-dev"),
        (AppChannel::Prod, "1.0.0+build"),
        (AppChannel::Dev, "1.0.0"),
        (AppChannel::Accept, "1.0.0"),
    ] {
        let service = Service::new(channel, version, None, |_| {
            panic!("disabled updater must not publish")
        });
        assert!(!service.snapshot().enabled);
        assert_eq!(service.check(false).unwrap().phase, Phase::Idle);
        assert_eq!(service.check(true).unwrap().phase, Phase::Idle);
    }
}
#[test]
fn fixture_source_hash_channel_and_actual_installer_are_checked_and_prod_never_accepts_fixture() {
    let (temp, _service) = setup(Duration::ZERO);
    assert!(
        Fixture::open(
            &temp.path().join("input"),
            "2222222222222222222222222222222222222222"
        )
        .is_err()
    );
    let fixture = Fixture::open(&temp.path().join("input"), SOURCE).unwrap();
    assert!(
        Service::fixture(
            AppChannel::Prod,
            Some(temp.path().join("other")),
            fixture,
            |_| {}
        )
        .is_err()
    );
    fs::write(temp.path().join("input/metadata.json"), b"modified").unwrap();
    assert!(matches!(
        Fixture::open(&temp.path().join("input"), SOURCE),
        Err(UpdateError::Digest)
    ));
}
#[test]
fn automatic_interval_and_existing_unowned_directory_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::open(&temp.path().join("updates"), AppChannel::Prod).unwrap();
    assert!(root.automatic_due(100000).unwrap());
    assert!(!root.automatic_due(100001).unwrap());
    assert!(!root.automatic_due(99999).unwrap());
    assert!(root.automatic_due(186400).unwrap());
    assert!(Root::open(&temp.path().join("updates"), AppChannel::Accept).is_err());
    let user = temp.path().join("user");
    fs::create_dir(&user).unwrap();
    fs::write(user.join("sentinel"), "keep").unwrap();
    assert!(Root::open(&user, AppChannel::Prod).is_err());
    assert_eq!(fs::read_to_string(user.join("sentinel")).unwrap(), "keep");
}
#[test]
fn automatic_storage_failure_is_visible_as_typed_state_without_network_or_task() {
    let service = Service::new(AppChannel::Prod, "1.0.0", None, |_| {});
    let snapshot = service.check(true).unwrap();
    assert_eq!(snapshot.phase, Phase::Failed);
    assert_eq!(snapshot.error, Some(UpdateError::Storage));
    assert_eq!(snapshot.next_action, Action::Check);
}

#[test]
fn native_snapshot_serialization_matches_the_renderer_contract_without_file_authority() {
    let service = Service::new(AppChannel::Prod, "1.0.0", None, |_| {});
    let value = serde_json::to_value(service.snapshot()).unwrap();
    let mut keys: Vec<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "availableVersion",
            "currentVersion",
            "disabledReason",
            "enabled",
            "error",
            "generation",
            "nextAction",
            "phase",
            "receivedBytes",
            "revision",
            "totalBytes"
        ]
    );
    assert_eq!(value["currentVersion"], "1.0.0");
    assert_eq!(value["phase"], "idle");
    assert_eq!(value["nextAction"], "check");
    assert!(value["availableVersion"].is_null());
    assert!(value["error"].is_null());
    assert_eq!(
        serde_json::to_value(UpdateError::InvalidRelease).unwrap(),
        "invalidRelease"
    );
    assert_eq!(
        serde_json::to_value(Disabled::DevelopmentVersion).unwrap(),
        "developmentVersion"
    );
}
#[test]
fn fixture_wrong_channel_and_changed_installer_are_rejected_before_service_admission() {
    let (temp, service) = setup(Duration::ZERO);
    drop(service);
    let path = temp.path().join("input/owner.json");
    let original = fs::read(&path).unwrap();
    let mut marker: serde_json::Value = serde_json::from_slice(&original).unwrap();
    marker["channel"] = "prod".into();
    fs::write(&path, serde_json::to_vec(&marker).unwrap()).unwrap();
    assert!(matches!(
        Fixture::open(&temp.path().join("input"), SOURCE),
        Err(UpdateError::InvalidRelease)
    ));
    fs::write(&path, original).unwrap();
    fs::write(temp.path().join("input/installer.exe"), vec![43_u8; 100000]).unwrap();
    assert!(matches!(
        Fixture::open(&temp.path().join("input"), SOURCE),
        Err(UpdateError::Digest)
    ));
}

#[tokio::test]
async fn install_confirmation_epochs_cancel_cleanup_and_commit_are_fenced() {
    let (_temp, service) = setup(Duration::ZERO);
    let worker = tokio::spawn(service.clone().run());
    service.check(false).unwrap();
    let available = wait(&service, |s| s.phase == Phase::Available).await;
    service.download(available.generation).unwrap();
    let ready = wait(&service, |s| s.phase == Phase::Ready).await;
    assert_eq!(ready.next_action, Action::Install);
    assert_eq!(
        service
            .install_summary(ready.generation, None, 1, 2)
            .blocked,
        Some(install::InstallBlock::NativeUnknown)
    );
    assert_eq!(
        service
            .install_summary(ready.generation, Some(1), 1, 2)
            .blocked,
        Some(install::InstallBlock::NativeActive)
    );
    let ticket = service
        .install_summary(ready.generation, Some(0), 1, 2)
        .confirmation_id
        .unwrap();
    assert!(matches!(
        service.begin_install(ready.generation, ticket, Some(0), 2, 2),
        Err(UpdateError::StaleOperation)
    ));
    let ticket = service
        .install_summary(ready.generation, Some(0), 2, 2)
        .confirmation_id
        .unwrap();
    assert!(matches!(
        service.begin_install(ready.generation, ticket, Some(0), 2, 3),
        Err(UpdateError::StaleOperation)
    ));
    let ticket = service
        .install_summary(ready.generation, Some(0), 2, 3)
        .confirmation_id
        .unwrap();
    let first = service
        .begin_install(ready.generation, ticket, Some(0), 2, 3)
        .unwrap();
    assert!(
        service
            .begin_install(ready.generation, ticket, Some(0), 2, 3)
            .is_err()
    );
    let cancelled = service.cancel(first.generation).unwrap();
    assert!(first.token.is_cancelled());
    assert_eq!(first.version, "1.1.0");
    assert!(first.fixture_install);
    assert_eq!(cancelled.phase, Phase::Installing);
    assert_eq!(cancelled.next_action, Action::None);
    assert_eq!(
        service.commit_install(first.generation, || panic!(
            "cancelled runner cannot commit"
        )),
        Err(UpdateError::Cancelled)
    );
    assert_eq!(
        service.download(first.generation).unwrap().phase,
        Phase::Installing
    );
    service.install_quarantined(first.generation);
    assert_eq!(service.snapshot().phase, Phase::Installing);
    assert_eq!(service.snapshot().next_action, Action::None);
    assert_eq!(service.snapshot().error, Some(UpdateError::Runner));
    service.install_aborted(first.generation, UpdateError::Cancelled);
    first.proof.revalidate().unwrap();
    let ticket = service
        .install_summary(first.generation, Some(0), 2, 3)
        .confirmation_id
        .unwrap();
    let next = service
        .begin_install(first.generation, ticket, Some(0), 2, 3)
        .unwrap();
    service.commit_install(next.generation, || Ok(())).unwrap();
    assert_eq!(service.snapshot().next_action, Action::None);
    assert!(service.cancel(next.generation).is_err());
    assert!(
        service
            .commit_install(next.generation, || panic!("single commit"))
            .is_err()
    );
    service.install_aborted(next.generation, UpdateError::Runner);
    assert_eq!(service.snapshot().phase, Phase::Installing);
    worker.abort();
    let _ = worker.await;
}

#[test]
fn unknown_recovery_blocks_checks_and_known_failure_exposes_recovery() {
    let (_temp, service) = setup(Duration::ZERO);
    service.recovered_install(UpdateError::RecoveryRequired, true);
    let unknown = service.snapshot();
    assert_eq!(unknown.phase, Phase::Installing);
    assert_eq!(unknown.next_action, Action::None);
    assert_eq!(service.check(false).unwrap(), unknown);
    assert!(service.cancel(unknown.generation).is_err());
    service.recovered_install(UpdateError::InstallFailed, false);
    assert_eq!(service.snapshot().phase, Phase::Failed);
    assert_eq!(service.snapshot().error, Some(UpdateError::InstallFailed));
    let recovered = service.snapshot();
    assert_eq!(service.check(true).unwrap(), recovered);
    assert_eq!(service.check(false).unwrap().phase, Phase::Checking);
}

#[tokio::test]
async fn actual_commit_fence_requires_every_dirty_window_and_same_native_epoch() {
    use crate::core::quit::Quit;
    let (temp, service) = setup(Duration::ZERO);
    let worker = tokio::spawn(service.clone().run());
    service.check(false).unwrap();
    let available = wait(&service, |s| s.phase == Phase::Available).await;
    service.download(available.generation).unwrap();
    let ready = wait(&service, |s| s.phase == Phase::Ready).await;
    let ticket = service
        .install_summary(ready.generation, Some(0), 10, 0)
        .confirmation_id
        .unwrap();
    let work = service
        .begin_install(ready.generation, ticket, Some(0), 10, 0)
        .unwrap();
    {
        use std::io::Read;
        let mut first = work.proof.sealed_source().unwrap();
        let mut second = work.proof.sealed_source().unwrap();
        let mut byte = [0_u8];
        first.file.read_exact(&mut byte).unwrap();
        assert_eq!(byte, [42]);
        assert_eq!(first.identity, second.identity);
        assert_eq!(second.size, 100000);
        second.file.read_exact(&mut byte).unwrap();
        assert_eq!(
            byte,
            [42],
            "runner read cursors are independent of download proof validation"
        );
    }
    let mut quit = Quit::default();
    let main = quit.register("main");
    let note = quit.register("popout-note-dirty");
    let labels = vec!["main".into(), "popout-note-dirty".into()];
    let round = quit.start(labels.clone()).unwrap();
    quit.ack("main", main, round, true);
    assert_eq!(
        service.commit_after_quit(
            &mut quit,
            install::CommitFence {
                generation: work.generation,
                round,
                labels: &labels,
                native: (Some(0), 10),
                expected_native_epoch: 10
            },
            || panic!("dirty note must prevent runner commit")
        ),
        Err(UpdateError::QuitRejected)
    );
    assert!(
        quit.start(labels.clone()).is_none(),
        "cleanup still owns the quit round"
    );
    quit.unseal(round);
    for native in [(Some(1), 10), (None, 10), (Some(0), 11)] {
        let round = quit.start(labels.clone()).unwrap();
        quit.ack("main", main, round, true);
        quit.ack("popout-note-dirty", note, round, true);
        assert!(
            service
                .commit_after_quit(
                    &mut quit,
                    install::CommitFence {
                        generation: work.generation,
                        round,
                        labels: &labels,
                        native,
                        expected_native_epoch: 10
                    },
                    || panic!("changed native lifetime must block commit")
                )
                .is_err()
        );
        assert!(!quit.sealed());
        assert!(!quit.approved);
        assert!(quit.start(labels.clone()).is_none());
        quit.unseal(round);
    }
    let round = quit.start(labels.clone()).unwrap();
    quit.ack("main", main, round, true);
    quit.ack("popout-note-dirty", note, round, true);
    drop(work.proof); // Production coordinator releases this clone after the runner ACK.
    let mut commits = 0;
    service
        .commit_after_quit(
            &mut quit,
            install::CommitFence {
                generation: work.generation,
                round,
                labels: &labels,
                native: (Some(0), 10),
                expected_native_epoch: 10,
            },
            || {
                commits += 1;
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(commits, 1);
    assert_eq!(
        fs::read_dir(temp.path().join("updates"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("download-"))
            .count(),
        0,
        "original download is cleaned before Tauri directly exits the process"
    );
    assert!(
        service
            .commit_after_quit(
                &mut quit,
                install::CommitFence {
                    generation: work.generation,
                    round,
                    labels: &labels,
                    native: (Some(0), 10),
                    expected_native_epoch: 10
                },
                || panic!("already committed")
            )
            .is_err()
    );
    assert!(quit.sealed());
    assert!(quit.approved);
    assert!(!quit.reserve_window());
    assert_eq!(quit.register("popout-note-too-late"), 0);
    worker.abort();
    let _ = worker.await;
}
