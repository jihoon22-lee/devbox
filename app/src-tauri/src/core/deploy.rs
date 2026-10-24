#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::core::process::ShellError;
    use std::time::Duration;
    use tokio::process::Command;
    use tokio_util::sync::CancellationToken;
    async fn publish(
        home: &std::path::Path,
        bytes: &[u8],
        sha: &str,
    ) -> Result<std::process::Output, ShellError> {
        publish_with_data(home, None, bytes, sha).await
    }
    async fn publish_with_data(
        home: &std::path::Path,
        data: Option<&std::path::Path>,
        bytes: &[u8],
        sha: &str,
    ) -> Result<std::process::Output, ShellError> {
        let mut cmd = Command::new("sh");
        cmd.current_dir(home)
            .env("HOME", home)
            .env_remove("XDG_DATA_HOME");
        if let Some(data) = data {
            cmd.env("XDG_DATA_HOME", data);
        }
        cmd.args(["-c", &install_script_for(AppChannel::Prod), "sh", sha]);
        super::super::process::run(
            cmd,
            bytes.to_vec(),
            Duration::from_secs(3),
            CancellationToken::new(),
        )
        .await
    }
    #[tokio::test]
    async fn redeploy_uses_the_first_launch_deploy_plan() {
        let steps = std::sync::Mutex::new(Vec::new());
        apply_deployment(
            || async {
                steps.lock().unwrap().push("install");
                Ok::<_, ()>(())
            },
            || async {
                steps.lock().unwrap().push("setup");
                Ok(())
            },
        )
        .await
        .unwrap();
        assert_eq!(*steps.lock().unwrap(), ["install", "setup"]);
        steps.lock().unwrap().clear();
        assert!(
            apply_deployment(
                || async {
                    steps.lock().unwrap().push("install");
                    Err::<(), _>(())
                },
                || async {
                    steps.lock().unwrap().push("setup");
                    Ok(())
                }
            )
            .await
            .is_err()
        );
        assert_eq!(*steps.lock().unwrap(), ["install"]);
    }
    #[tokio::test]
    async fn channel_install_and_exec_preserve_other_channel_bytes_and_mtimes() {
        let fixture = tempfile::tempdir().unwrap();
        let data = fixture.path().join("custom data");
        let mut before = Vec::new();
        for channel in [AppChannel::Prod, AppChannel::Accept, AppChannel::Dev] {
            let binary = format!(
                "#!/bin/sh\nprintf '%s\\n' '{}' \"$0\" \"$@\"\n",
                channel.config().instance
            );
            let mut command = Command::new("sh");
            command
                .env("HOME", fixture.path())
                .env("XDG_DATA_HOME", &data)
                .args([
                    "-c",
                    &install_script_for(channel),
                    "sh",
                    &digest(binary.as_bytes()),
                ]);
            super::super::process::run(
                command,
                binary.as_bytes().to_vec(),
                Duration::from_secs(3),
                CancellationToken::new(),
            )
            .await
            .unwrap();
            for (path, bytes, modified) in &before {
                assert_eq!(std::fs::read(path).unwrap(), *bytes);
                assert_eq!(
                    std::fs::metadata(path).unwrap().modified().unwrap(),
                    *modified
                );
            }
            let path = data.join(channel.config().storage_name).join("bin/devbox");
            before.push((
                path.clone(),
                std::fs::read(&path).unwrap(),
                std::fs::metadata(&path).unwrap().modified().unwrap(),
            ));
            for action in ["setup", "doctor", "bridge", "hook"] {
                let mut command = Command::new("sh");
                command
                    .env("HOME", fixture.path())
                    .env("XDG_DATA_HOME", &data)
                    .args(["-c", &exec_script_for(channel), "sh", action]);
                let output = super::super::process::run(
                    command,
                    vec![],
                    Duration::from_secs(3),
                    CancellationToken::new(),
                )
                .await
                .unwrap();
                assert_eq!(
                    String::from_utf8(output.stdout).unwrap(),
                    format!(
                        "{}\n{}\n{action}\n",
                        channel.config().instance,
                        path.display()
                    )
                );
            }
        }
        assert!(install_script().contains(CHANNEL.config().storage_name));
        assert!(exec_script().contains(CHANNEL.config().storage_name));
    }
    #[tokio::test]
    async fn absolute_xdg_publishes_in_custom_data_root_and_preserves_instance_data() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().join("home");
        let data = fixture.path().join("custom data/nested");
        std::fs::create_dir_all(&home).unwrap();
        let instance = data.join("devbox/prod");
        std::fs::create_dir_all(&instance).unwrap();
        std::fs::write(instance.join("owned-fixture"), "keep").unwrap();
        let bytes = b"custom xdg executable";
        publish_with_data(&home, Some(&data), bytes, &digest(bytes))
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(data.join("devbox/bin/devbox")).unwrap(),
            bytes
        );
        assert!(!home.join(".local").exists());
        assert_eq!(
            std::fs::read(instance.join("owned-fixture")).unwrap(),
            b"keep"
        );
    }
    #[tokio::test]
    async fn relative_xdg_falls_back_to_home_and_custom_symlink_ancestors_are_rejected() {
        let fixture = tempfile::tempdir().unwrap();
        let bytes = b"relative xdg executable";
        publish_with_data(
            fixture.path(),
            Some(std::path::Path::new("relative/data")),
            bytes,
            &digest(bytes),
        )
        .await
        .unwrap();
        assert_eq!(
            std::fs::read(fixture.path().join(".local/share/devbox/bin/devbox")).unwrap(),
            bytes
        );
        let other = tempfile::tempdir().unwrap();
        let linked = fixture.path().join("linked-data");
        std::os::unix::fs::symlink(other.path(), &linked).unwrap();
        assert!(
            publish_with_data(
                fixture.path(),
                Some(&linked.join("nested")),
                bytes,
                &digest(bytes)
            )
            .await
            .is_err()
        );
        assert!(std::fs::read_dir(other.path()).unwrap().next().is_none());
    }
    #[tokio::test]
    async fn setup_doctor_and_bridge_execute_the_published_xdg_binary() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().join("home");
        let custom = fixture.path().join("custom data");
        std::fs::create_dir_all(&home).unwrap();
        let binary = b"#!/bin/sh\nprintf '%s\\n' \"$0\" \"$@\"\n";
        for data in [
            Some(custom.as_path()),
            Some(std::path::Path::new("relative/data")),
            None,
        ] {
            publish_with_data(&home, data, binary, &digest(binary))
                .await
                .unwrap();
            let root = data
                .filter(|path| path.is_absolute())
                .map(std::path::Path::to_path_buf)
                .unwrap_or_else(|| home.join(".local/share"));
            for args in [&["setup", "--json"][..], &["doctor", "--json"], &["bridge"]] {
                let mut cmd = Command::new("sh");
                cmd.current_dir(&home)
                    .env("HOME", &home)
                    .env_remove("XDG_DATA_HOME");
                if let Some(data) = data {
                    cmd.env("XDG_DATA_HOME", data);
                }
                cmd.args(["-c", &exec_script_for(AppChannel::Prod), "sh"])
                    .args(args);
                let output = super::super::process::run(
                    cmd,
                    vec![],
                    Duration::from_secs(3),
                    CancellationToken::new(),
                )
                .await
                .unwrap();
                let expected = format!(
                    "{}\n{}\n",
                    root.join("devbox/bin/devbox").display(),
                    args.join("\n")
                );
                assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
            }
        }
    }
    #[tokio::test]
    async fn nonabsolute_home_is_rejected_before_writing() {
        let fixture = tempfile::tempdir().unwrap();
        let bytes = b"invalid home";
        let mut cmd = Command::new("sh");
        cmd.current_dir(fixture.path())
            .env("HOME", "relative-home")
            .env_remove("XDG_DATA_HOME")
            .args([
                "-c",
                &install_script_for(AppChannel::Prod),
                "sh",
                &digest(bytes),
            ]);
        assert!(
            super::super::process::run(
                cmd,
                bytes.to_vec(),
                Duration::from_secs(3),
                CancellationToken::new()
            )
            .await
            .is_err()
        );
        assert!(std::fs::read_dir(fixture.path()).unwrap().next().is_none());
    }
    #[tokio::test]
    async fn verifies_digest_and_atomically_replaces_only_owned_link() {
        let dir = tempfile::tempdir().unwrap();
        let a = b"first executable";
        let b = b"same version new bytes";
        publish(dir.path(), a, &digest(a)).await.unwrap();
        publish(dir.path(), b, &digest(b)).await.unwrap();
        let bin = dir.path().join(".local/share/devbox/bin");
        assert_eq!(std::fs::read(bin.join("devbox")).unwrap(), b);
        assert!(publish(dir.path(), a, &digest(b)).await.is_err());
        assert_eq!(std::fs::read(bin.join("devbox")).unwrap(), b);
        assert!(std::fs::read_dir(&bin).unwrap().all(|p| {
            !p.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".install-")
        }));
    }
    #[tokio::test]
    async fn refuses_foreign_bin_directory_and_symlink_parent() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join(".local/share/devbox/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("devbox"), "foreign").unwrap();
        assert!(publish(dir.path(), b"new", &digest(b"new")).await.is_err());
        assert_eq!(std::fs::read(bin.join("devbox")).unwrap(), b"foreign");
        let other = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(other.path(), dir.path().join("bad")).unwrap();
        assert!(
            publish(&dir.path().join("bad"), b"new", &digest(b"new"))
                .await
                .is_err()
        );
    }
}

use super::channel::{AppChannel, CHANNEL};
use sha2::{Digest, Sha256};
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
/// Version strings are not identities: development builds share the same version.
/// The private directory and ownership marker belong exclusively to this installer.
// Keep publish and every native invocation aligned with core::Paths: only absolute
// XDG_DATA_HOME overrides HOME/.local/share. HOME itself must always be absolute.
const PATHS_SCRIPT: &str = r#"set -eu
case "${HOME-}" in /*) ;; *) printf '%s\n' 'HOME must be absolute' >&2; exit 62;; esac
case "${XDG_DATA_HOME-}" in /*) data="$XDG_DATA_HOME";; *) data="$HOME/.local/share";; esac
root="$data/@STORAGE@"
d="$root/bin"
check_path() {
 p="$1"
 while [ "$p" != / ] && [ "$p" != . ]; do
  [ ! -L "$p" ] || exit 62
  p=$(dirname "$p")
 done
}
check_path "$HOME"
check_path "$d"
"#;

pub fn install_script() -> String {
    install_script_for(CHANNEL)
}
pub fn exec_script() -> String {
    exec_script_for(CHANNEL)
}
fn install_script_for(channel: AppChannel) -> String {
    let paths = PATHS_SCRIPT.replace("@STORAGE@", channel.config().storage_name);
    format!("{paths}\n{INSTALL_SCRIPT}")
}
fn exec_script_for(channel: AppChannel) -> String {
    let paths = PATHS_SCRIPT.replace("@STORAGE@", channel.config().storage_name);
    format!("{paths}\nexec \"$d/devbox\" \"$@\"")
}

const INSTALL_SCRIPT: &str = r#"umask 077
case "$1" in *[!0-9a-f]*|'') exit 61;; esac
[ "${#1}" = 64 ] || exit 61
mkdir -p -- "$root"
[ -d "$root" ] && [ ! -L "$root" ] || exit 62
if mkdir "$d" 2>/dev/null; then printf '%s\n' 'devbox-desktop-bin-v1' > "$d/.owner"; fi
[ ! -L "$d" ] && [ -d "$d" ] && [ "$(stat -c %u "$d")" = "$(id -u)" ] && [ "$(stat -c %a "$d")" = 700 ] || exit 63
[ ! -L "$d/.owner" ] && [ -f "$d/.owner" ] && [ "$(cat "$d/.owner")" = devbox-desktop-bin-v1 ] || exit 63
exec 9<"$d"
flock -x -w 15 9
if [ -e "$d/devbox" ] || [ -L "$d/devbox" ]; then
 [ -L "$d/devbox" ] || exit 64
 old=$(readlink "$d/devbox")
 case "$old" in devbox-*) h=${old#devbox-};; *) exit 64;; esac
 case "$h" in *[!0-9a-f]*|'') exit 64;; esac
 [ "${#h}" = 64 ] && [ -f "$d/$old" ] && [ ! -L "$d/$old" ] || exit 64
 [ "$(sha256sum "$d/$old" | cut -d ' ' -f1)" = "$h" ] || exit 64
fi
t=$(mktemp -d "$d/.install-XXXXXXXXXX")
trap 'rm -rf -- "$t"' EXIT HUP INT TERM
cat > "$t/binary"
[ "$(sha256sum "$t/binary" | cut -d ' ' -f1)" = "$1" ] || exit 65
chmod 755 "$t/binary"
target="$d/devbox-$1"
if [ -e "$target" ] || [ -L "$target" ]; then
 [ ! -L "$target" ] && [ -f "$target" ] && [ "$(sha256sum "$target" | cut -d ' ' -f1)" = "$1" ] || exit 66
else
 ln "$t/binary" "$target"
fi
ln -s "devbox-$1" "$t/link"
mv -Tf "$t/link" "$d/devbox"
"#;

/// Both first launch and explicit redeployment use the same install/setup steps.
pub async fn apply_deployment<I, S, IFut, SFut, E>(install: I, setup: S) -> Result<(), E>
where
    I: FnOnce() -> IFut,
    S: FnOnce() -> SFut,
    IFut: std::future::Future<Output = Result<(), E>>,
    SFut: std::future::Future<Output = Result<(), E>>,
{
    install().await?;
    setup().await
}
