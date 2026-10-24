//! Report config keys, never credential-bearing configuration values.
use crate::{GitError, run::output};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GitConfigWarning {
    pub key: String,
}
pub async fn warnings(dir: &Path) -> Result<Vec<GitConfigWarning>, GitError> {
    let (code, out, _) = output(
        dir,
        &[
            "config",
            "--show-scope",
            "--name-only",
            "--get-regexp",
            r"^(core\.(fsmonitor|hookspath)|filter\..*|diff\.external)$",
        ],
    )
    .await?;
    match code {
        0 => Ok(out
            .lines()
            .filter_map(|line| {
                let (scope, key) = line.split_once('\t')?;
                (scope != "command").then(|| GitConfigWarning { key: key.into() })
            })
            .collect()),
        1 => Ok(Vec::new()),
        _ => Err(GitError::InvalidOutput),
    }
}
