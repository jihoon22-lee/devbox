//! `devbox note add [--daily] <text>`: appends to the inbox or today's daily note
//! through the daemon (`notes.append`), for scripts and agents.
use devbox_notes::{AppendFormat, AppendNote, AppendTarget, NotesAppend};
use devbox_protocol::message::ClientKind;

pub async fn add(daily: bool, text: String) -> anyhow::Result<()> {
    let paths = crate::paths()?;
    let client =
        crate::client::Client::connect(&paths.socket, ClientKind::Cli, paths.instance.as_str())
            .await?;
    let target = if daily {
        AppendTarget::Daily
    } else {
        AppendTarget::Inbox
    };
    let appended = client
        .call::<NotesAppend>(&AppendNote {
            target,
            text,
            format: AppendFormat::Plain,
        })
        .await?;
    println!("{}", appended.path);
    Ok(())
}
