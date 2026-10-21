//! Explicit recovery output is the only plaintext key output available to users.
use devbox_protocol::message::ClientKind;
use tokio::io::AsyncReadExt;
use zeroize::Zeroizing;
pub async fn recovery(import: bool) -> anyhow::Result<()> {
    let paths = crate::paths()?;
    let client =
        crate::client::Client::connect(&paths.socket, ClientKind::Cli, paths.instance.as_str())
            .await?;
    if import {
        let mut input = Zeroizing::new(String::new());
        tokio::io::stdin()
            .take(1025)
            .read_to_string(&mut input)
            .await?;
        if input.len() > 1024 {
            anyhow::bail!("invalid recovery input")
        }
        client
            .call::<devbox_secrets::ImportKey>(&devbox_secrets::Recovery {
                recovery: input.to_string(),
            })
            .await?;
    } else {
        let recovery = client
            .call::<devbox_secrets::ExportKey>(&devbox_secrets::Empty {})
            .await?;
        println!("{}", recovery.recovery);
    }
    Ok(())
}
