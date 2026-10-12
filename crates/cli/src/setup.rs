#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_roundtrip_and_rejects_injection() {
        let text =
            capture_env(b"PATH=/usr/bin\0LANG=a\"b\\c$d\0LC_X\nPATH=evil\0GITHUB_TOKEN=secret\0");
        assert_eq!(env_path(&text).as_deref(), Some("/usr/bin"));
        assert!(text.contains("LANG=\"a\\\"b\\\\c$d\""));
        assert!(!text.contains("evil"));
        assert!(!text.contains("secret"));
        assert!(env_path("PATH=\"\"").is_none());
    }
    #[test]
    fn units_reference_instance() {
        let units = render_units();
        assert!(
            units[1]
                .1
                .contains("EnvironmentFile=-%h/.config/devbox/%i/env")
        );
        assert!(units[0].1.contains("SocketMode=0600"));
    }
}

use serde::Serialize;
use std::io::Write;
use std::path::Path;
const ALLOWED: &[&str] = &[
    "PATH",
    "LANG",
    "NVM_DIR",
    "NVM_BIN",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "GOPATH",
    "PNPM_HOME",
    "BUN_INSTALL",
    "DENO_INSTALL",
    "JAVA_HOME",
    "EDITOR",
    "SHELL",
    "TZ",
];
const MARKER: &str = "# Managed by devbox setup\n";
pub fn capture_env(raw: &[u8]) -> String {
    let mut entries = std::collections::BTreeMap::new();
    for entry in raw.split(|b| *b == 0) {
        let Ok(entry) = std::str::from_utf8(entry) else {
            continue;
        };
        let Some((k, v)) = entry.split_once('=') else {
            continue;
        };
        if !k
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            || !(ALLOWED.contains(&k) || k.starts_with("LC_"))
            || v.chars().any(char::is_control)
        {
            continue;
        }
        entries.insert(k, v);
    }
    entries
        .into_iter()
        .map(|(k, v)| format!("{k}=\"{}\"\n", v.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect()
}
/// Decode the quoted subset emitted by capture_env without shell expansion.
pub(crate) fn env_path(text: &str) -> Option<String> {
    let value = text
        .lines()
        .filter_map(|l| l.strip_prefix("PATH="))
        .last()?;
    let quoted = value.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::new();
    let mut chars = quoted.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let n = chars.next()?;
                if !matches!(n, '\\' | '"' | '$' | '`') {
                    out.push('\\');
                }
                out.push(n);
            }
            '"' => return None,
            _ => out.push(c),
        }
    }
    if out.chars().any(char::is_control)
        || !out
            .split(':')
            .any(|s| !s.is_empty() && Path::new(s).is_absolute())
    {
        return None;
    }
    Some(out)
}
pub fn render_units() -> Vec<(&'static str, String)> {
    vec![
        (
            "devbox@.socket",
            format!(
                "{MARKER}[Unit]\nDescription=devbox daemon socket (%i)\n\n[Socket]\nListenStream=%t/devbox-%i.sock\nSocketMode=0600\n\n[Install]\nWantedBy=sockets.target\n"
            ),
        ),
        (
            "devbox@.service",
            format!(
                "{MARKER}[Unit]\nDescription=devbox daemon (%i)\nRequires=devbox@%i.socket\nAfter=devbox@%i.socket\n\n[Service]\nType=notify\nEnvironmentFile=-%h/.config/devbox/%i/env\nEnvironment=DEVBOX_INSTANCE=%i\nExecStart=%h/.local/share/devbox/bin/devbox daemon\nRestart=on-failure\nRestartSec=1\nKillMode=mixed\n\n[Install]\nWantedBy=default.target\n"
            ),
        ),
    ]
}
fn unit_quote(path: &Path) -> Result<String, String> {
    let value = path.to_str().ok_or("unit path is not UTF-8")?;
    if value.chars().any(char::is_control) {
        return Err("unit path contains control characters".into());
    }
    Ok(format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('\"', "\\\"")
            .replace('%', "%%")
    ))
}
// ListenStream's parser consumes the raw rvalue rather than a shell-style word.
fn socket_path(path: &Path) -> Result<String, String> {
    let value = path.to_str().ok_or("socket path is not UTF-8")?;
    if value.chars().any(char::is_control) {
        return Err("socket path contains control characters".into());
    }
    Ok(value.replace('%', "%%"))
}
fn instance_units(paths: &devbox_core::paths::Paths) -> Result<Vec<(String, String)>, String> {
    let mut units: Vec<_> = render_units()
        .into_iter()
        .map(|(n, b)| (n.into(), b))
        .collect();
    // Paths have already resolved XDG overrides; preserve them across the manager boundary.
    let body = (|| -> Result<String, String> {
        let mut body = format!(
            "{MARKER}[Service]\nEnvironmentFile=\nEnvironmentFile={}\nExecStart=\nExecStart=:{} daemon\n",
            unit_quote(&paths.instance_config_dir.join("env"))?,
            unit_quote(&paths.bin_dir.join("devbox"))?
        );
        for (key, path) in [
            ("HOME", paths.home.as_path()),
            ("XDG_CONFIG_HOME", paths.config_dir.parent().unwrap()),
            (
                "XDG_DATA_HOME",
                paths.data_dir.parent().unwrap().parent().unwrap(),
            ),
            (
                "XDG_STATE_HOME",
                paths.state_dir.parent().unwrap().parent().unwrap(),
            ),
            ("XDG_RUNTIME_DIR", paths.socket.parent().unwrap()),
        ] {
            let value = unit_quote(path)?;
            body.push_str(&format!(
                "Environment=\"{key}={}\"\n",
                &value[1..value.len() - 1]
            ));
        }
        Ok(body)
    })();
    units.push((
        format!("devbox@{}.service.d/paths.conf", paths.instance.as_str()),
        body?,
    ));
    let socket = socket_path(&paths.socket)?;
    units.push((
        format!("devbox@{}.socket.d/paths.conf", paths.instance.as_str()),
        format!("{MARKER}[Socket]\nListenStream=\nListenStream={socket}\n"),
    ));
    Ok(units)
}
#[derive(Debug, Serialize)]
pub struct Step {
    pub id: &'static str,
    pub ok: bool,
    pub message: String,
}
#[derive(Debug, Serialize)]
pub struct SetupReport {
    pub steps: Vec<Step>,
}
/// Refuse symlinks at every parent boundary before creating directories.
fn safe_dir(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if parent != path {
            safe_dir(parent)?;
        }
    }
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(std::io::Error::other("unsafe directory")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir(path),
        Err(e) => Err(e),
    }
}
fn publish(path: &Path, body: &str, managed: bool) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    safe_dir(
        path.parent()
            .ok_or_else(|| std::io::Error::other("missing parent"))?,
    )?;
    match std::fs::symlink_metadata(path) {
        Ok(m) => {
            if !m.is_file()
                || m.file_type().is_symlink()
                || (managed && !std::fs::read_to_string(path)?.starts_with(MARKER))
            {
                return Err(std::io::Error::other("refusing foreign file or symlink"));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let temp = path.with_file_name(format!(".devbox-{}", uuid::Uuid::new_v4()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    let result = (|| {
        file.write_all(body.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temp, path)
    })();
    let _ = std::fs::remove_file(temp);
    result
}
pub(crate) async fn command(cmd: &str, args: &[&str]) -> Result<std::process::Output, String> {
    let mut command = tokio::process::Command::new(cmd);
    command.args(args).kill_on_drop(true);
    let out = tokio::time::timeout(std::time::Duration::from_secs(15), command.output())
        .await
        .map_err(|_| format!("{cmd} timed out"))?
        .map_err(|_| format!("{cmd} could not run"))?;
    if out.status.success() {
        Ok(out)
    } else {
        Err(format!("{cmd} exited unsuccessfully"))
    }
}
trait Runner {
    async fn run(&self, cmd: &str, args: &[&str]) -> Result<Vec<u8>, String>;
}
struct OsRunner;
impl Runner for OsRunner {
    async fn run(&self, cmd: &str, args: &[&str]) -> Result<Vec<u8>, String> {
        command(cmd, args).await.map(|out| out.stdout)
    }
}
async fn install(paths: &devbox_core::paths::Paths, runner: &impl Runner) -> SetupReport {
    let mut report = SetupReport { steps: vec![] };
    if let Err(message) = instance_units(paths) {
        report.steps.push(Step {
            id: "units",
            ok: false,
            message,
        });
        return report;
    }
    for id in ["env", "units", "reload", "enable", "restart"] {
        let result: Result<(), String> = match id {
            "env" => match runner.run("bash", &["-lc", "env -0"]).await {
                Ok(out) => {
                    let text = capture_env(&out);
                    if env_path(&text).is_none() {
                        Err("captured PATH has no absolute entry".into())
                    } else {
                        publish(&paths.instance_config_dir.join("env"), &text, false)
                            .map_err(|e| e.to_string())
                    }
                }
                Err(e) => Err(e),
            },
            "units" => instance_units(paths).and_then(|units| {
                units
                    .iter()
                    .try_for_each(|(name, body)| {
                        publish(
                            &paths.home.join(".config/systemd/user").join(name),
                            body,
                            true,
                        )
                    })
                    .map_err(|e| e.to_string())
            }),
            "reload" => runner
                .run("systemctl", &["--user", "daemon-reload"])
                .await
                .map(|_| ()),
            "enable" => runner
                .run(
                    "systemctl",
                    &[
                        "--user",
                        "enable",
                        "--now",
                        &format!("devbox@{}.socket", paths.instance.as_str()),
                        &format!("devbox@{}.service", paths.instance.as_str()),
                    ],
                )
                .await
                .map(|_| ()),
            _ => runner
                .run(
                    "systemctl",
                    &[
                        "--user",
                        "restart",
                        &format!("devbox@{}.service", paths.instance.as_str()),
                    ],
                )
                .await
                .map(|_| ()),
        };
        let ok = result.is_ok();
        report.steps.push(Step {
            id,
            ok,
            message: result.err().unwrap_or_default(),
        });
        if !ok {
            break;
        }
    }
    report
}
pub async fn run(json: bool) -> anyhow::Result<()> {
    let report = install(&crate::paths()?, &OsRunner).await;
    if json {
        println!("{}", serde_json::to_string(&report)?);
    } else {
        for s in &report.steps {
            println!(
                "{} {} {}",
                if s.ok { "ok" } else { "FAIL" },
                s.id,
                s.message
            );
        }
    }
    if report.steps.iter().all(|s| s.ok) {
        Ok(())
    } else {
        anyhow::bail!("setup failed")
    }
}

#[cfg(test)]
mod safety_tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    #[test]
    fn publication_is_private_and_refuses_foreign_files_and_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let env = temp.path().join("env");
        publish(&env, "PATH=\"/usr/bin\"\n", false).unwrap();
        assert_eq!(
            std::fs::metadata(&env).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(publish(&env, "replacement", true).is_err());
        assert_eq!(
            std::fs::read_to_string(&env).unwrap(),
            "PATH=\"/usr/bin\"\n"
        );
        let link = temp.path().join("link");
        symlink(&env, &link).unwrap();
        assert!(publish(&link, "evil", false).is_err());
        let parent_link = temp.path().join("parent");
        symlink(temp.path(), &parent_link).unwrap();
        assert!(publish(&parent_link.join("env"), "evil", false).is_err());
    }
    #[test]
    fn resolved_xdg_paths_are_bound_to_only_the_selected_instance() {
        let temp = tempfile::tempdir().unwrap();
        let mut paths = devbox_core::paths::Paths::resolve(
            devbox_core::paths::Instance::parse("test-custom").unwrap(),
            &FakeEnv(temp.path().into()),
        )
        .unwrap();
        let root = temp.path().join("custom space%\"${HOME}");
        paths.config_dir = root.join("config/devbox");
        paths.instance_config_dir = paths.config_dir.join("test-custom");
        paths.data_dir = root.join("data/devbox/test-custom");
        paths.bin_dir = root.join("data/devbox/bin");
        paths.state_dir = root.join("state/devbox/test-custom");
        paths.socket = root.join("runtime/devbox-test-custom.sock");
        let units = instance_units(&paths).unwrap();
        assert_eq!(units.len(), 4);
        assert_eq!(units[2].0, "devbox@test-custom.service.d/paths.conf");
        assert!(units[2].1.contains("ExecStart=:\""));
        assert!(units[2].1.contains("space%%\\\"${HOME}"));
        for key in [
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
            "XDG_RUNTIME_DIR",
        ] {
            assert!(units[2].1.contains(key));
        }
        assert!(units[3].1.contains("ListenStream=\nListenStream="));
        let other = temp
            .path()
            .join(".config/systemd/user/devbox@other.service.d/paths.conf");
        std::fs::create_dir_all(other.parent().unwrap()).unwrap();
        std::fs::write(&other, "other instance").unwrap();
        for (name, body) in units {
            publish(
                &temp.path().join(".config/systemd/user").join(name),
                &body,
                true,
            )
            .unwrap();
        }
        assert_eq!(std::fs::read_to_string(other).unwrap(), "other instance");
        paths.socket = root.join("bad\npath");
        assert!(instance_units(&paths).is_err());
    }
    #[test]
    fn socket_path_is_accepted_by_systemd_parser_without_shell_quoting() {
        let temp = tempfile::tempdir().unwrap();
        let mut paths = devbox_core::paths::Paths::resolve(
            devbox_core::paths::Instance::parse("test-parser").unwrap(),
            &FakeEnv(temp.path().into()),
        )
        .unwrap();
        // Spaces, quotes, backslashes and dollar signs are literal socket path bytes.
        paths.socket = temp.path().join("socket space%\"\\$literal.sock");
        let units = instance_units(&paths).unwrap();
        let body = &units[3].1;
        let socket = temp.path().join("fixture.socket");
        let service = temp.path().join("fixture.service");
        std::fs::write(&socket, body).unwrap();
        std::fs::write(&service, "[Service]\nExecStart=/bin/true\n").unwrap();
        let out = std::process::Command::new("/usr/bin/systemd-analyze")
            .args(["--user", "verify"])
            .arg(&socket)
            .arg(&service)
            .env_clear()
            .env("HOME", temp.path())
            .env("XDG_RUNTIME_DIR", temp.path())
            .env("PATH", "/usr/bin:/bin")
            .env("SYSTEMD_UNIT_PATH", format!("{}:", temp.path().display()))
            .output()
            .expect("systemd-analyze is required for socket parser regression");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let raw = paths.socket.to_str().unwrap().replace('%', "%%");
        assert!(body.contains(&format!("ListenStream={raw}\n")));
    }
    struct FakeEnv(std::path::PathBuf);
    impl devbox_core::paths::Env for FakeEnv {
        fn var(&self, key: &str) -> Option<String> {
            (key == "HOME").then(|| self.0.display().to_string())
        }
        fn uid(&self) -> u32 {
            12345
        }
    }
    struct FakeRunner {
        env: Vec<u8>,
        calls: std::sync::Mutex<Vec<String>>,
        fail: Option<&'static str>,
    }
    impl Runner for FakeRunner {
        async fn run(&self, cmd: &str, args: &[&str]) -> Result<Vec<u8>, String> {
            let call = format!("{cmd} {}", args.join(" "));
            self.calls.lock().unwrap().push(call.clone());
            if self.fail.is_some_and(|s| call.contains(s)) {
                Err("fixture failure".into())
            } else {
                Ok(if cmd == "bash" {
                    self.env.clone()
                } else {
                    vec![]
                })
            }
        }
    }
    #[tokio::test]
    async fn setup_orders_commands_and_stops_after_each_failure() {
        for fail in [None, Some("daemon-reload"), Some("enable"), Some("restart")] {
            let temp = tempfile::tempdir().unwrap();
            let paths = devbox_core::paths::Paths::resolve(
                devbox_core::paths::Instance::parse("test-setup").unwrap(),
                &FakeEnv(temp.path().into()),
            )
            .unwrap();
            let runner = FakeRunner {
                env: b"PATH=/usr/bin\0TOKEN=secret\0".to_vec(),
                calls: Default::default(),
                fail,
            };
            let report = install(&paths, &runner).await;
            let expected = match fail {
                None => 5,
                Some("daemon-reload") => 3,
                Some("enable") => 4,
                _ => 5,
            };
            assert_eq!(report.steps.len(), expected);
            assert_eq!(report.steps.last().unwrap().ok, fail.is_none());
            let calls = runner.calls.lock().unwrap();
            assert_eq!(calls.len(), expected - 1);
            if expected >= 4 {
                assert!(
                    calls[2].contains(
                        "enable --now devbox@test-setup.socket devbox@test-setup.service"
                    )
                );
            }
        }
    }
    #[tokio::test]
    async fn invalid_env_or_foreign_unit_prevents_reload() {
        for invalid_env in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let paths = devbox_core::paths::Paths::resolve(
                devbox_core::paths::Instance::parse("test-setup").unwrap(),
                &FakeEnv(temp.path().into()),
            )
            .unwrap();
            let unit_dir = paths.home.join(".config/systemd/user");
            std::fs::create_dir_all(&unit_dir).unwrap();
            std::fs::write(unit_dir.join("devbox@.socket"), "foreign").unwrap();
            let runner = FakeRunner {
                env: if invalid_env {
                    b"PATH=relative\0".to_vec()
                } else {
                    b"PATH=/usr/bin\0".to_vec()
                },
                calls: Default::default(),
                fail: None,
            };
            let report = install(&paths, &runner).await;
            assert_eq!(report.steps.len(), if invalid_env { 1 } else { 2 });
            assert!(!report.steps.last().unwrap().ok);
            assert_eq!(runner.calls.lock().unwrap().len(), 1);
        }
    }
}
