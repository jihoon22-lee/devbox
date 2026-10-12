use crate::core::{
    process::{self, ShellError},
    wsl::{Distro, decode_output},
};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
pub fn command(distro: &str) -> tokio::process::Command {
    let mut c = tokio::process::Command::new("wsl.exe");
    c.args(["-d", distro]).creation_flags(0x0800_0000);
    c
}
pub fn distros() -> std::io::Result<Vec<Distro>> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let key = match RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Lxss")
    {
        Ok(k) => k,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    let default: String = key.get_value("DefaultDistribution").unwrap_or_default();
    key.enum_keys()
        .map(|entry| {
            let guid = entry?;
            let k = key.open_subkey(&guid)?;
            Ok(Distro {
                name: k.get_value("DistributionName")?,
                version: k.get_value("Version")?,
                default: guid == default,
            })
        })
        .collect()
}
pub async fn systemd(distro: &str, cancel: CancellationToken) -> Result<(), ShellError> {
    let mut c = command(distro);
    c.args(["--exec", "cat", "/proc/1/comm"]);
    let out = process::run(c, vec![], Duration::from_secs(15), cancel.clone()).await?;
    if decode_output(&out.stdout).trim() != "systemd" {
        return Err(ShellError::SystemdOff);
    }
    let mut c = command(distro);
    c.args([
        "--exec",
        "systemctl",
        "--user",
        "show",
        "--property=Version",
        "--value",
    ]);
    process::run(c, vec![], Duration::from_secs(15), cancel).await?;
    Ok(())
}
