//! Pure parsing of native stash records; indices remain bounded selectors.
use serde::Serialize;
pub const STASH_FORMAT: &str = "%gd%x00%H%x00%ct%x00%gs";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct StashEntry {
    pub index: u32,
    pub commit: String,
    pub created_at: i64,
    pub message: String,
}
pub fn parse_list(output: &str) -> Result<Vec<StashEntry>, String> {
    output
        .lines()
        .map(|line| {
            let fields = line.split('\0').collect::<Vec<_>>();
            let [selector, commit, time, message] = fields.as_slice() else {
                return Err("stash_operation_failed".into());
            };
            let index = selector
                .strip_prefix("stash@{")
                .and_then(|s| s.strip_suffix('}'))
                .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|n| *n <= 999)
                .ok_or("stash_operation_failed")?;
            if commit.is_empty() {
                return Err("stash_operation_failed".into());
            }
            Ok(StashEntry {
                index,
                commit: (*commit).into(),
                created_at: time.parse().map_err(|_| "stash_operation_failed")?,
                message: (*message).into(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stash_list_parses_index_commit_time_and_message() {
        let output = "stash@{0}\0dddd\01790000000\0On main: wip login\nstash@{1}\0eeee\01789990000\0WIP on main: 1234567 base\n";
        let entries = parse_list(output).unwrap();
        assert_eq!(
            entries[0],
            StashEntry {
                index: 0,
                commit: "dddd".into(),
                created_at: 1_790_000_000,
                message: "On main: wip login".into()
            }
        );
        assert_eq!(entries[1].index, 1);
        for invalid in [
            "stash@{x}\0d\01\0m\n",
            "stash@{1000}\0d\01\0m\n",
            "stash@{0}\0d\0bad\0m\n",
            "stash@{0}\0d\01\0m\0extra\n",
        ] {
            assert!(parse_list(invalid).is_err());
        }
        assert!(parse_list("").unwrap().is_empty());
    }
}
