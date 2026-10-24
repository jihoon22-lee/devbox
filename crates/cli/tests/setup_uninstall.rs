use devbox_cli::setup::uninstall::{UninstallError, UnitController, uninstall_with};
use devbox_core::{Env, Instance, Paths};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Mutex};
struct FixtureEnv(String);
impl Env for FixtureEnv {
    fn var(&self, key: &str) -> Option<String> {
        match key {
            "HOME" => Some(self.0.clone()),
            "XDG_RUNTIME_DIR" => Some(format!("{}/run", self.0)),
            _ => None,
        }
    }
    fn uid(&self) -> u32 {
        1000
    }
}
fn paths(home: &Path, instance: &str) -> Paths {
    Paths::resolve(
        Instance::parse(instance).expect("instance"),
        &FixtureEnv(home.display().to_string()),
    )
    .expect("paths")
}
fn install_fixture(paths: &Paths) {
    use std::os::unix::fs::{PermissionsExt, symlink};
    paths.ensure_dirs().expect("fixture dirs");
    std::fs::create_dir_all(&paths.bin_dir).expect("bin");
    std::fs::set_permissions(&paths.bin_dir, std::fs::Permissions::from_mode(0o700))
        .expect("private bin");
    std::fs::write(paths.bin_dir.join(".owner"), "devbox-desktop-bin-v1\n").expect("owner");
    let bytes = b"owned fixture binary";
    let name = format!("devbox-{:x}", Sha256::digest(bytes));
    std::fs::write(paths.bin_dir.join(&name), bytes).expect("binary");
    symlink(name, paths.bin_dir.join("devbox")).expect("current link");
    let units = paths.xdg_config_home.join("systemd/user");
    std::fs::create_dir_all(&units).expect("units");
    for (name, body) in devbox_cli::setup::render_units(paths.instance.channel()) {
        std::fs::write(units.join(name), body).expect("owned unit");
    }
    let channel = paths.instance.channel().config();
    for name in [
        format!(
            "{}@{}.service.d",
            channel.daemon_unit,
            paths.instance.as_str()
        ),
        format!(
            "{}@{}.socket.d",
            channel.daemon_unit,
            paths.instance.as_str()
        ),
        format!(
            "{}@{}.service.d",
            channel.tmux_unit,
            paths.instance.as_str()
        ),
    ] {
        std::fs::create_dir_all(units.join(&name)).expect("dropin");
        std::fs::write(
            units.join(name).join("paths.conf"),
            "# Managed by devbox setup\n[Service]\n",
        )
        .expect("owned dropin");
    }
    std::fs::write(paths.data_dir.join("user-data"), "keep database and files").expect("data");
    std::fs::write(
        paths.config_dir.join("config.toml"),
        "[projects]\nroots=['keep']\n",
    )
    .expect("config");
}
#[derive(Default)]
struct FakeController {
    calls: Mutex<Vec<Vec<String>>>,
    fail: bool,
}
impl UnitController for FakeController {
    async fn disable(&self, units: &[String]) -> Result<(), UninstallError> {
        self.calls.lock().expect("calls").push(units.to_vec());
        if self.fail {
            Err(UninstallError::UnitControl)
        } else {
            Ok(())
        }
    }
    async fn reload(&self) -> Result<(), UninstallError> {
        self.calls
            .lock()
            .expect("calls")
            .push(vec!["reload".into()]);
        Ok(())
    }
}
#[tokio::test]
async fn uninstall_removes_only_this_channel_units_and_binary() {
    let temp = tempfile::tempdir().expect("owned fixture");
    let accept = paths(temp.path(), "accept");
    let prod = paths(temp.path(), "prod");
    install_fixture(&accept);
    install_fixture(&prod);
    let preserved = [
        prod.bin_dir.join("devbox"),
        prod.config_dir.join("config.toml"),
        prod.data_dir.join("user-data"),
        accept.config_dir.join("config.toml"),
        accept.data_dir.join("user-data"),
        prod.xdg_config_home.join("systemd/user/devbox@.service"),
    ];
    let before: Vec<_> = preserved
        .iter()
        .map(|path| {
            (
                path,
                std::fs::read(path).expect("before"),
                std::fs::metadata(path)
                    .expect("metadata")
                    .modified()
                    .expect("time"),
            )
        })
        .collect();
    let controller = FakeController::default();
    let report = uninstall_with(&accept, &controller)
        .await
        .expect("uninstall fixture");
    assert!(report.removed_files > 0);
    assert!(!accept.bin_dir.exists());
    assert!(
        !accept
            .xdg_config_home
            .join("systemd/user/devbox-accept@.service")
            .exists()
    );
    assert_eq!(
        *controller.calls.lock().expect("calls"),
        vec![
            vec![
                "devbox-accept@accept.socket".to_owned(),
                "devbox-accept@accept.service".into(),
                "devbox-accept-tmux@accept.service".into()
            ],
            vec!["reload".into()]
        ]
    );
    for (path, bytes, modified) in before {
        assert_eq!(std::fs::read(path).expect("preserved"), bytes);
        assert_eq!(
            std::fs::metadata(path)
                .expect("metadata")
                .modified()
                .expect("time"),
            modified
        );
    }
}
#[tokio::test]
async fn uninstall_refuses_foreign_files_symlinks_and_noncanonical_instances_before_stopping() {
    let temp = tempfile::tempdir().expect("owned fixture");
    let p = paths(temp.path(), "accept");
    install_fixture(&p);
    let controller = FakeController::default();
    std::fs::write(p.bin_dir.join("user-material.txt"), "keep").expect("foreign file");
    assert!(uninstall_with(&p, &controller).await.is_err());
    assert!(controller.calls.lock().expect("calls").is_empty());
    assert_eq!(
        std::fs::read(p.bin_dir.join("user-material.txt")).expect("preserved"),
        b"keep"
    );
    std::fs::remove_file(p.bin_dir.join("user-material.txt")).expect("remove owned test fixture");
    let unit = p
        .xdg_config_home
        .join("systemd/user/devbox-accept@.service");
    let outside = temp.path().join("outside-unit");
    std::fs::write(&outside, "foreign").expect("outside");
    std::fs::remove_file(&unit).expect("fixture unit");
    std::os::unix::fs::symlink(&outside, &unit).expect("fixture link");
    assert!(uninstall_with(&p, &controller).await.is_err());
    assert!(controller.calls.lock().expect("calls").is_empty());
    assert_eq!(std::fs::read(&outside).expect("preserved"), b"foreign");
    assert!(
        uninstall_with(&paths(temp.path(), "test-other-instance"), &controller)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn uninstall_keeps_all_files_when_unit_shutdown_fails() {
    let temp = tempfile::tempdir().expect("owned fixture");
    let p = paths(temp.path(), "dev");
    install_fixture(&p);
    let controller = FakeController {
        fail: true,
        ..Default::default()
    };
    assert!(uninstall_with(&p, &controller).await.is_err());
    assert!(p.bin_dir.join("devbox").exists());
    assert!(
        p.xdg_config_home
            .join("systemd/user/devbox-dev@.service")
            .exists()
    );
    assert_eq!(controller.calls.lock().expect("calls").len(), 1);
}

#[tokio::test]
async fn uninstall_preserves_an_active_installer_and_tampered_binary_before_stopping() {
    let temp = tempfile::tempdir().expect("owned fixture");
    let p = paths(temp.path(), "accept");
    install_fixture(&p);
    let controller = FakeController::default();
    let lock = std::fs::File::open(&p.bin_dir).expect("owned bin");
    lock.try_lock().expect("owned installer lock");
    assert!(matches!(
        uninstall_with(&p, &controller).await,
        Err(UninstallError::Busy)
    ));
    assert!(controller.calls.lock().expect("calls").is_empty());
    assert!(p.bin_dir.join("devbox").exists());
    drop(lock);
    let target = std::fs::read_link(p.bin_dir.join("devbox")).expect("current binary");
    std::fs::write(p.bin_dir.join(target), "foreign replacement").expect("owned tamper fixture");
    assert!(uninstall_with(&p, &controller).await.is_err());
    assert!(controller.calls.lock().expect("calls").is_empty());
    assert_eq!(
        std::fs::read(p.bin_dir.join("devbox")).expect("preserved"),
        b"foreign replacement"
    );
}

struct ReplacedParentController {
    bin: std::path::PathBuf,
    moved: std::path::PathBuf,
}
impl UnitController for ReplacedParentController {
    async fn disable(&self, _: &[String]) -> Result<(), UninstallError> {
        std::fs::rename(&self.bin, &self.moved)?;
        std::os::unix::fs::symlink(&self.moved, &self.bin)?;
        Ok(())
    }
    async fn reload(&self) -> Result<(), UninstallError> {
        Ok(())
    }
}
#[tokio::test]
async fn review_uninstall_preserves_files_after_parent_is_replaced_during_shutdown() {
    let temp = tempfile::tempdir().unwrap();
    let p = paths(temp.path(), "accept");
    install_fixture(&p);
    let moved = temp.path().join("moved-user-directory");
    let controller = ReplacedParentController {
        bin: p.bin_dir.clone(),
        moved: moved.clone(),
    };
    let result = uninstall_with(&p, &controller).await;
    assert!(result.is_err(), "replacement must be refused");
    assert!(
        moved.join(".owner").exists(),
        "uninstall followed replaced ancestor and deleted moved files"
    );
    assert!(
        moved.join("devbox").exists(),
        "moved binary must remain intact"
    );
}
struct SwapTree {
    source: std::path::PathBuf,
    moved: std::path::PathBuf,
    link: bool,
}
impl UnitController for SwapTree {
    async fn disable(&self, _: &[String]) -> Result<(), UninstallError> {
        std::fs::rename(&self.source, &self.moved)?;
        if self.link {
            std::os::unix::fs::symlink(&self.moved, &self.source)?;
        } else {
            std::fs::create_dir(&self.source)?;
        }
        Ok(())
    }
    async fn reload(&self) -> Result<(), UninstallError> {
        Ok(())
    }
}
fn snapshot_files(root: &Path, files: &mut Vec<(std::path::PathBuf, Vec<u8>)>) {
    for entry in std::fs::read_dir(root).expect("owned tree") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            snapshot_files(&path, files);
        } else {
            files.push((path.clone(), std::fs::read(path).expect("owned bytes")));
        }
    }
}
#[tokio::test]
async fn uninstall_refuses_replaced_bin_unit_and_dropin_ancestors_before_any_unlink() {
    for variant in 0..5 {
        let temp = tempfile::tempdir().expect("fixture");
        let p = paths(temp.path(), "accept");
        install_fixture(&p);
        let source = match variant {
            0 | 4 => p.bin_dir.clone(),
            1 => p.bin_dir.parent().expect("parent").to_owned(),
            2 => p.xdg_config_home.join("systemd"),
            _ => p
                .xdg_config_home
                .join("systemd/user/devbox-accept@accept.service.d"),
        };
        let moved = temp.path().join("relocated-user-tree");
        let mut files = Vec::new();
        snapshot_files(temp.path(), &mut files);
        let controller = SwapTree {
            source: source.clone(),
            moved: moved.clone(),
            link: variant != 4,
        };
        assert!(uninstall_with(&p, &controller).await.is_err());
        for (path, bytes) in files {
            let expected = if let Ok(relative) = path.strip_prefix(&source) {
                moved.join(relative)
            } else {
                path
            };
            assert_eq!(
                std::fs::read(&expected).expect("must preserve every preflight file"),
                bytes,
                "{}",
                expected.display()
            );
        }
    }
}
