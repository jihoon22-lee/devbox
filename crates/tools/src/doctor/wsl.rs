use super::{ProbeIssue, Probed};
#[derive(Debug, PartialEq, Eq)]
pub struct IdleSettings {
    pub instance_ms: i64,
    pub vm_ms: i64,
}
/// .wslconfig: instanceIdleTimeout belongs to [general], vmIdleTimeout to [wsl2].
pub fn idle_settings(text: Option<&str>) -> Probed<IdleSettings> {
    let mut out = IdleSettings {
        instance_ms: 15_000,
        vm_ms: 60_000,
    };
    let mut section = String::new();
    let mut instance_seen = false;
    let mut vm_seen = false;
    for line in text
        .unwrap_or_default()
        .trim_start_matches('\u{feff}')
        .lines()
    {
        let line = line.split(['#', ';']).next().unwrap_or_default().trim();
        if line.starts_with('[') {
            section = line
                .strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .ok_or(ProbeIssue::InvalidData)?
                .trim()
                .to_ascii_lowercase();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let target = if section == "general" && key.eq_ignore_ascii_case("instanceIdleTimeout") {
            Some((&mut out.instance_ms, &mut instance_seen))
        } else if section == "wsl2" && key.eq_ignore_ascii_case("vmIdleTimeout") {
            Some((&mut out.vm_ms, &mut vm_seen))
        } else {
            None
        };
        if let Some((target, seen)) = target {
            if *seen {
                return Err(ProbeIssue::InvalidData);
            }
            *seen = true;
            *target = value.trim().parse().map_err(|_| ProbeIssue::InvalidData)?;
            if *target < -1 {
                return Err(ProbeIssue::InvalidData);
            }
        }
    }
    Ok(out)
}
