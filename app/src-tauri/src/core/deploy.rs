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
        let mut cmd = Command::new("sh");
        cmd.env("HOME", home).args(["-c", SCRIPT, "sh", sha]);
        super::super::process::run(
            cmd,
            bytes.to_vec(),
            Duration::from_secs(3),
            CancellationToken::new(),
        )
        .await
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

use sha2::{Digest, Sha256};
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
/// Version strings are not identities: development builds share the same version.
/// The private directory and ownership marker belong exclusively to this installer.
pub const SCRIPT: &str = r#"set -eu
umask 077
case "$1" in *[!0-9a-f]*|'') exit 61;; esac
[ "${#1}" = 64 ] || exit 61
p="$HOME"
while [ "$p" != / ] && [ "$p" != . ]; do [ ! -L "$p" ] || exit 62; p=$(dirname "$p"); done
for p in "$HOME/.local" "$HOME/.local/share" "$HOME/.local/share/devbox"; do
 [ ! -L "$p" ] || exit 62
 mkdir -p "$p"
 [ -d "$p" ] || exit 62
done
d="$HOME/.local/share/devbox/bin"
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
