use devbox_core::{Env, Instance, Paths, log};
use std::process::Command;
struct FixtureEnv(String);
impl Env for FixtureEnv {
    fn var(&self, key: &str) -> Option<String> {
        (key == "HOME").then(|| self.0.clone())
    }
    fn uid(&self) -> u32 {
        1000
    }
}
#[test]
fn logging_and_panic_contract() {
    if std::env::var_os("DEVBOX_LOG_CHILD").is_none() {
        for filter in ["info", "warn", "info["] {
            let output =
                Command::new(std::env::current_exe().expect("resolve fixture test executable"))
                    .args(["--exact", "logging_and_panic_contract", "--nocapture"])
                    .env("DEVBOX_LOG_CHILD", "1")
                    .env("RUST_LOG", filter)
                    .output()
                    .expect("collect fixture process output");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return;
    }
    tracing::subscriber::with_default(tracing_subscriber::registry(), || {
        tracing::info!("scoped subscriber before initialization");
    });
    let temp = tempfile::tempdir().expect("create fixture directory");
    let mut paths = Paths::resolve(
        Instance::parse("test-log").expect("parse fixture instance"),
        &FixtureEnv(temp.path().display().to_string()),
    )
    .expect("parse fixture instance");
    std::fs::write(temp.path().join("blocked"), "file").expect("write fixture file");
    paths.log_dir = temp.path().join("blocked/child");
    assert!(log::init_logging(&paths, false).is_err());
    paths.log_dir = temp.path().join("logs");
    std::fs::create_dir_all(&paths.log_dir).expect("create fixture directory");
    for day in 1..=10 {
        std::fs::write(
            paths.log_dir.join(format!("daemon.2000-01-{day:02}.log")),
            "old",
        )
        .expect("write fixture file");
    }
    std::fs::write(paths.log_dir.join("crash-preserved.log"), "preserve")
        .expect("write fixture file");
    let guard = log::init_logging(&paths, false).expect("initialize fixture logger");
    assert!(log::init_logging(&paths, false).is_err());
    log::install_panic_hook(&paths, "1.0.0-test");
    for i in 0..60 {
        tracing::warn!(sequence = i, "recent-event");
    }
    tracing::info!("info-only");
    assert!(
        std::thread::Builder::new()
            .name("worker-first".into())
            .spawn(|| panic!("boom here"))
            .expect("spawn fixture process")
            .join()
            .is_err()
    );
    drop(guard);
    let files: Vec<_> = std::fs::read_dir(&paths.log_dir)
        .expect("list fixture directory")
        .map(|e| e.expect("list fixture directory").path())
        .collect();
    let crashes: Vec<_> = files
        .iter()
        .filter(|p| {
            p.file_name()
                .expect("fixture path has filename")
                .to_str()
                .expect("fixture path is UTF-8")
                .starts_with("crash-")
        })
        .collect();
    assert_eq!(crashes.len(), 2);
    let info_enabled = std::env::var("RUST_LOG").expect("fixture environment is set") != "warn";
    for file in crashes
        .into_iter()
        .filter(|p| p.file_name().expect("fixture path has filename") != "crash-preserved.log")
    {
        let text = std::fs::read_to_string(file).expect("read fixture text");
        assert!(text.contains("version: 1.0.0-test"));
        assert!(text.contains("thread: worker-"));
        assert!(text.contains("panic_hook.rs"));
        assert!(text.contains("boom here"));
        assert_eq!(text.lines().filter(|l| l.starts_with("  ")).count(), 50);
        assert_eq!(text.contains("info-only"), info_enabled);
        assert!(!text.contains("sequence=0 "));
    }
    let threads: Vec<_> = (0..8)
        .map(|i| {
            std::thread::Builder::new()
                .name(format!("worker-{i}"))
                .spawn(|| panic!("boom here"))
                .expect("spawn fixture process")
        })
        .collect();
    for thread in threads {
        assert!(thread.join().is_err());
    }
    assert_eq!(
        std::fs::read_dir(&paths.log_dir)
            .expect("list fixture directory")
            .filter(|e| e
                .as_ref()
                .expect("fixture child exists")
                .file_name()
                .to_str()
                .expect("fixture path is UTF-8")
                .starts_with("crash-"))
            .count(),
        10
    );
    assert_eq!(
        files
            .iter()
            .filter(|p| p
                .file_name()
                .expect("fixture path has filename")
                .to_str()
                .expect("fixture path is UTF-8")
                .starts_with("daemon."))
            .count(),
        7
    );
    assert_eq!(
        std::fs::read_to_string(paths.log_dir.join("crash-preserved.log"))
            .expect("read fixture text"),
        "preserve"
    );
    let daemon = files
        .iter()
        .find(|p| {
            p.file_name()
                .expect("fixture path has filename")
                .to_str()
                .expect("fixture path is UTF-8")
                .starts_with("daemon.")
                && !p
                    .file_name()
                    .expect("fixture path has filename")
                    .to_str()
                    .expect("fixture path is UTF-8")
                    .contains("2000-01")
        })
        .expect("fixture path is UTF-8");
    let text = std::fs::read_to_string(daemon).expect("read fixture text");
    assert!(text.contains("sequence=0"));
    assert_eq!(text.contains("info-only"), info_enabled);
}
