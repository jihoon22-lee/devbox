use crate::{
    host::{ProcFs, read_text},
    *,
};
use std::{collections::HashMap, path::Path};
pub(crate) fn parse_pressure(text: &str) -> Option<Pressure> {
    let mut result = Pressure::default();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let kind = fields.next()?;
        let values: HashMap<_, _> = fields.filter_map(|s| s.split_once('=')).collect();
        let value = PressureLine {
            avg10: values.get("avg10")?.parse().ok()?,
            avg60: values.get("avg60")?.parse().ok()?,
            avg300: values.get("avg300")?.parse().ok()?,
            total_us: values.get("total")?.parse().ok()?,
        };
        if [value.avg10, value.avg60, value.avg300]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
        {
            return None;
        }
        match kind {
            "some" => result.some = Some(value),
            "full" => result.full = Some(value),
            _ => (),
        }
    }
    (result.some.is_some() || result.full.is_some()).then_some(result)
}
pub(crate) fn filesystem(home: &Path) -> Option<FilesystemInfo> {
    // statvfs supplies df's filesystem counters without a subprocess or locale parser.
    let stat = nix::sys::statvfs::statvfs(home).ok()?;
    let size = stat.fragment_size();
    Some(FilesystemInfo {
        total_bytes: stat.blocks().saturating_mul(size),
        available_bytes: stat.blocks_available().saturating_mul(size),
        used_bytes: stat
            .blocks()
            .saturating_sub(stat.blocks_free())
            .saturating_mul(size),
    })
}
pub(crate) fn system_snapshot(proc: &ProcFs, home: &Path) -> R<SystemInfo> {
    let text = read_text(&proc.root.join("meminfo"), 65536)
        .map_err(|_| ObserveError::SourceUnavailable)?;
    let values: HashMap<_, _> = text
        .lines()
        .filter_map(|l| {
            let (key, rest) = l.split_once(':')?;
            let bytes = rest
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()?
                .checked_mul(1024)?;
            Some((key, bytes))
        })
        .collect();
    let value = |key| {
        values
            .get(key)
            .copied()
            .ok_or(ObserveError::SourceUnavailable)
    };
    let pressure = |name| {
        read_text(&proc.root.join("pressure").join(name), 65536)
            .ok()
            .and_then(|s| parse_pressure(&s))
    };
    let swap_total_bytes = value("SwapTotal")?;
    Ok(SystemInfo {
        memory_total_bytes: value("MemTotal")?,
        memory_available_bytes: value("MemAvailable")?,
        swap_total_bytes,
        swap_used_bytes: swap_total_bytes.saturating_sub(value("SwapFree")?),
        cpu_pressure: pressure("cpu"),
        memory_pressure: pressure("memory"),
        io_pressure: pressure("io"),
        home_filesystem: filesystem(home),
    })
}
