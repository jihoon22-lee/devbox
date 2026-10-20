//! DPAPI CurrentUser only. No plaintext key file, argument, or diagnostic.
use crate::{SecretsError, crypto::Key};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeroize::Zeroizing;
pub const POWERSHELL: &str = "/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe";
const PREFIX: &str = "$ErrorActionPreference='Stop';Add-Type -AssemblyName System.Security;$b=$null;$r=$null;$m=$null;$failed=$false;try{$i=[Console]::OpenStandardInput();$m=New-Object IO.MemoryStream;$i.CopyTo($m);$b=$m.ToArray();";
const SUFFIX: &str = ";$o=[Console]::OpenStandardOutput();$o.Write($r,0,$r.Length)}catch{$failed=$true}finally{if($null -ne $b){[Array]::Clear($b,0,$b.Length)};if($null -ne $r){[Array]::Clear($r,0,$r.Length)};if($null -ne $m){$buffer=$m.GetBuffer();[Array]::Clear($buffer,0,$buffer.Length);$m.Dispose()}};if($failed){exit 42}";

pub struct DpapiKeyStore {
    path: PathBuf,
    executable: PathBuf,
}
impl DpapiKeyStore {
    pub fn new(data: &Path) -> Self {
        Self {
            path: data.join("keys/data.key.dpapi"),
            executable: POWERSHELL.into(),
        }
    }
    pub fn with_executable(mut self, path: PathBuf) -> Self {
        self.executable = path;
        self
    }
    pub fn blob(&self) -> Result<Option<Vec<u8>>, SecretsError> {
        use std::os::unix::fs::OpenOptionsExt;
        if std::fs::symlink_metadata(self.path.parent().expect("keys directory"))
            .is_ok_and(|m| m.file_type().is_symlink() || !m.is_dir())
        {
            return Err(SecretsError::KeyMismatch);
        }
        let mut file = match std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&self.path)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(SecretsError::KeyMismatch),
        };
        if !file
            .metadata()
            .is_ok_and(|m| m.is_file() && m.len() <= 65536)
        {
            return Err(SecretsError::KeyMismatch);
        }
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut bytes).map_err(|_| SecretsError::Unavailable)?;
        if bytes.is_empty() {
            return Err(SecretsError::KeyMismatch);
        }
        Ok(Some(bytes))
    }
    pub fn publish(&self, blob: &[u8], replace: bool) -> Result<(), SecretsError> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        if blob.is_empty() || blob.len() > 65536 {
            return Err(SecretsError::InvalidInput);
        }
        let parent = self.path.parent().expect("keys directory");
        if std::fs::symlink_metadata(parent)
            .is_ok_and(|m| !m.is_dir() || m.file_type().is_symlink())
        {
            return Err(SecretsError::Unavailable);
        }
        std::fs::create_dir_all(parent).map_err(|_| SecretsError::Unavailable)?;
        if !replace {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&self.path)
                .map_err(|_| SecretsError::KeyMismatch)?;
            file.write_all(blob)
                .and_then(|()| file.sync_all())
                .map_err(|_| SecretsError::Unavailable)?;
        } else {
            let temp = parent.join(format!(".sealed-{}", std::process::id()));
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp)
                .map_err(|_| SecretsError::Unavailable)?;
            let result = file
                .write_all(blob)
                .and_then(|()| file.sync_all())
                .and_then(|()| std::fs::rename(&temp, &self.path));
            let _ = std::fs::remove_file(temp);
            result.map_err(|_| SecretsError::Unavailable)?;
        }
        std::fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|_| SecretsError::Unavailable)
    }
    pub async fn create(&self) -> Result<(Key, Vec<u8>), SecretsError> {
        let key = Key::random();
        let blob = self.seal(&key).await?;
        Ok((key, blob))
    }
    pub async fn seal(&self, key: &Key) -> Result<Vec<u8>, SecretsError> {
        self.exchange(true, key.bytes()).await.map(|b| b.to_vec())
    }
    pub async fn unseal(&self, blob: &[u8]) -> Result<Key, SecretsError> {
        let bytes = self.exchange(false, blob).await?;
        Ok(Key::from_bytes(
            bytes
                .as_slice()
                .try_into()
                .map_err(|_| SecretsError::KeyMismatch)?,
        ))
    }
    async fn exchange(
        &self,
        protect: bool,
        input: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, SecretsError> {
        let operation = if protect { "Protect" } else { "Unprotect" };
        let script = format!(
            "{PREFIX}$r=[Security.Cryptography.ProtectedData]::{operation}($b,$null,[Security.Cryptography.DataProtectionScope]::CurrentUser){SUFFIX}"
        );
        let executable = self.executable.clone();
        let input = Zeroizing::new(input.to_vec());
        let cancel = tokio_util::sync::CancellationToken::new();
        let _cancel_on_drop = cancel.clone().drop_guard();
        let task = tokio::spawn(async move {
            let mut child = tokio::process::Command::new(&executable)
                .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .map_err(|_| SecretsError::Unavailable)?;
            let mut stdin = child.stdin.take().ok_or(SecretsError::Unavailable)?;
            let mut stdout = child
                .stdout
                .take()
                .ok_or(SecretsError::Unavailable)?
                .take(65537);
            let operation = async {
                let mut output = Zeroizing::new(Vec::new());
                let (write, read) = tokio::join!(
                    async move {
                        stdin.write_all(&input).await?;
                        drop(stdin);
                        Ok::<(), std::io::Error>(())
                    },
                    stdout.read_to_end(&mut output)
                );
                write.map_err(|_| SecretsError::Unavailable)?;
                read.map_err(|_| SecretsError::Unavailable)?;
                if output.len() > 65536 {
                    return Err(SecretsError::KeyMismatch);
                }
                Ok(output)
            };
            let result = tokio::select! {biased;_ = cancel.cancelled()=>Err(SecretsError::Unavailable),result=tokio::time::timeout(std::time::Duration::from_secs(10),operation)=>result.map_err(|_|SecretsError::Unavailable).and_then(|r|r)};
            if result.is_err() {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return result;
            }
            let status = tokio::select! {biased;_ = cancel.cancelled()=>None,result=tokio::time::timeout(std::time::Duration::from_secs(2),child.wait())=>result.ok().and_then(Result::ok)};
            match status {
                Some(status) if status.success() => result.and_then(|output| {
                    if output.is_empty() {
                        Err(SecretsError::KeyMismatch)
                    } else {
                        Ok(output)
                    }
                }),
                Some(status) if status.code() == Some(42) => Err(SecretsError::KeyMismatch),
                Some(_) => Err(SecretsError::Unavailable),
                None => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    Err(SecretsError::Unavailable)
                }
            }
        });
        task.await.map_err(|_| SecretsError::Unavailable)?
    }
}
