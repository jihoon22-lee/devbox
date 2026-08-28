//! WSL distro resource summary parsers.
//!
//! The producer deliberately parses small, numeric-only proc/df fixtures.  Collection owns
//! the fixed argv boundary; this module never executes a command and never retains a path,
//! command line, environment or credential from the command output.

use serde::Serialize;

pub const MAX_RESOURCE_OUTPUT_BYTES: usize = 64 * 1024;
/// Dashboard IPC encodes these values as JSON numbers, so keep them within JavaScript's
/// exactly representable integer range as well as within a practical host-filesystem bound.
pub const MAX_RESOURCE_BYTES: u64 = 9_007_199_254_740_991;

const RESOURCE_PARSE_ERROR: &str = "WSL resource summary 형식이 올바르지 않습니다";

/// A bounded, numeric-only summary shown for one running distro.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSummary {
    /// Busy CPU time between two successful samples. The first sample is intentionally null.
    pub cpu_percent: Option<u8>,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
}

/// The aggregate `/proc/stat` CPU counters needed to calculate usage between snapshots.
/// Guest counters are not included in `total_ticks` because Linux already includes them in
/// user/nice and summing them again would overstate total CPU time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuSample {
    total_ticks: u64,
    idle_ticks: u64,
}

pub fn parse_cpu_sample(input: &str) -> Result<CpuSample, &'static str> {
    if input.len() > MAX_RESOURCE_OUTPUT_BYTES {
        return Err(RESOURCE_PARSE_ERROR);
    }
    let mut fields = input
        .lines()
        .next()
        .ok_or(RESOURCE_PARSE_ERROR)?
        .split_whitespace();
    if fields.next() != Some("cpu") {
        return Err(RESOURCE_PARSE_ERROR);
    }
    let counters = fields
        .map(|field| field.parse::<u64>().map_err(|_| RESOURCE_PARSE_ERROR))
        .collect::<Result<Vec<_>, _>>()?;
    if !(4..=10).contains(&counters.len()) {
        return Err(RESOURCE_PARSE_ERROR);
    }
    let mut total_ticks = 0_u64;
    // user, nice, system, idle, iowait, irq, softirq, steal. guest/guest_nice are excluded.
    for value in counters.iter().take(8) {
        total_ticks = total_ticks
            .checked_add(*value)
            .ok_or(RESOURCE_PARSE_ERROR)?;
    }
    let idle_ticks = counters[3]
        .checked_add(counters.get(4).copied().unwrap_or(0))
        .ok_or(RESOURCE_PARSE_ERROR)?;
    if total_ticks == 0 || idle_ticks > total_ticks {
        return Err(RESOURCE_PARSE_ERROR);
    }
    Ok(CpuSample {
        total_ticks,
        idle_ticks,
    })
}

pub fn cpu_usage_percent(previous: Option<CpuSample>, current: CpuSample) -> Option<u8> {
    let previous = previous?;
    let total_delta = current.total_ticks.checked_sub(previous.total_ticks)?;
    let idle_delta = current.idle_ticks.checked_sub(previous.idle_ticks)?;
    if total_delta == 0 || idle_delta > total_delta {
        return None;
    }
    let busy_delta = total_delta - idle_delta;
    Some(
        ((busy_delta as f64 / total_delta as f64) * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8,
    )
}

/// Parse MemTotal and MemAvailable from `/proc/meminfo`.  Values are accepted only in kB,
/// which is the stable Linux proc contract, and all arithmetic is checked.
pub fn parse_memory(input: &str) -> Result<(u64, u64), &'static str> {
    if input.len() > MAX_RESOURCE_OUTPUT_BYTES {
        return Err(RESOURCE_PARSE_ERROR);
    }
    let mut total_kib = None;
    let mut available_kib = None;
    for line in input.lines() {
        if line.len() > MAX_RESOURCE_OUTPUT_BYTES {
            return Err(RESOURCE_PARSE_ERROR);
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if !matches!(key, "MemTotal" | "MemAvailable") {
            continue;
        }
        let mut fields = value.split_whitespace();
        let amount = fields
            .next()
            .ok_or(RESOURCE_PARSE_ERROR)?
            .parse::<u64>()
            .map_err(|_| RESOURCE_PARSE_ERROR)?;
        if fields.next() != Some("kB") || fields.next().is_some() {
            return Err(RESOURCE_PARSE_ERROR);
        }
        if amount > MAX_RESOURCE_BYTES / 1024 {
            return Err(RESOURCE_PARSE_ERROR);
        }
        let slot = if key == "MemTotal" {
            &mut total_kib
        } else {
            &mut available_kib
        };
        if slot.replace(amount).is_some() {
            return Err(RESOURCE_PARSE_ERROR);
        }
    }

    let total = total_kib
        .ok_or(RESOURCE_PARSE_ERROR)?
        .checked_mul(1024)
        .ok_or(RESOURCE_PARSE_ERROR)?;
    let available = available_kib
        .ok_or(RESOURCE_PARSE_ERROR)?
        .checked_mul(1024)
        .ok_or(RESOURCE_PARSE_ERROR)?;
    if total == 0 || available > total {
        return Err(RESOURCE_PARSE_ERROR);
    }
    Ok((total - available, total))
}

/// Parse the first data row of `df -P -B1 -- /`.  The mount path is intentionally ignored;
/// only the three numeric byte columns before the capacity token are trusted.
pub fn parse_disk(input: &str) -> Result<(u64, u64), &'static str> {
    if input.len() > MAX_RESOURCE_OUTPUT_BYTES {
        return Err(RESOURCE_PARSE_ERROR);
    }
    for line in input.lines().skip(1) {
        if line.len() > MAX_RESOURCE_OUTPUT_BYTES {
            return Err(RESOURCE_PARSE_ERROR);
        }
        let fields = line.split_whitespace().collect::<Vec<_>>();
        let Some(capacity_index) = fields.iter().position(|field| {
            field.ends_with('%')
                && field[..field.len() - 1]
                    .parse::<u16>()
                    .is_ok_and(|capacity| capacity <= 100)
        }) else {
            continue;
        };
        if capacity_index < 4 {
            return Err(RESOURCE_PARSE_ERROR);
        }
        let total = fields[capacity_index - 3]
            .parse::<u64>()
            .map_err(|_| RESOURCE_PARSE_ERROR)?;
        let used = fields[capacity_index - 2]
            .parse::<u64>()
            .map_err(|_| RESOURCE_PARSE_ERROR)?;
        let available = fields[capacity_index - 1]
            .parse::<u64>()
            .map_err(|_| RESOURCE_PARSE_ERROR)?;
        if total == 0
            || total > MAX_RESOURCE_BYTES
            || used > total
            || available > total
            || used > MAX_RESOURCE_BYTES
            || available > MAX_RESOURCE_BYTES
            || used.checked_add(available).is_none_or(|sum| sum > total)
        {
            return Err(RESOURCE_PARSE_ERROR);
        }
        return Ok((used, total));
    }
    Err(RESOURCE_PARSE_ERROR)
}

pub fn build_summary(
    cpu_stat: &str,
    memory: &str,
    disk: &str,
    previous_cpu: Option<CpuSample>,
) -> Result<(ResourceSummary, CpuSample), &'static str> {
    let current_cpu = parse_cpu_sample(cpu_stat)?;
    let cpu_percent = cpu_usage_percent(previous_cpu, current_cpu);
    let (memory_used_bytes, memory_total_bytes) = parse_memory(memory)?;
    let (disk_used_bytes, disk_total_bytes) = parse_disk(disk)?;
    Ok((
        ResourceSummary {
            cpu_percent,
            memory_used_bytes,
            memory_total_bytes,
            disk_used_bytes,
            disk_total_bytes,
        },
        current_cpu,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEMINFO: &str =
        "MemTotal:       16384 kB\nMemFree:         8192 kB\nMemAvailable:   12288 kB\n";
    const DF: &str = "Filesystem 1B-blocks Used Available Use% Mounted on\n/dev/sdb 100000000 25000000 75000000 25% /\n";

    #[test]
    fn parses_numeric_resource_fixtures() {
        let previous = parse_cpu_sample("cpu 100 10 20 870 0 0 0 0 0 0\n").unwrap();
        let (summary, current) = build_summary(
            "cpu 120 10 30 940 0 0 0 0 0 0\n",
            MEMINFO,
            DF,
            Some(previous),
        )
        .unwrap();
        assert_eq!(summary.cpu_percent, Some(30));
        assert_eq!(cpu_usage_percent(None, current), None);
        assert_eq!(summary.memory_total_bytes, 16_384 * 1024);
        assert_eq!(summary.memory_used_bytes, 4_096 * 1024);
        assert_eq!(summary.disk_used_bytes, 25_000_000);
        assert_eq!(summary.disk_total_bytes, 100_000_000);
    }

    #[test]
    fn rejects_duplicate_or_invalid_memory_fields() {
        assert!(parse_memory("MemTotal: 1 kB\nMemTotal: 2 kB\nMemAvailable: 1 kB\n").is_err());
        assert!(parse_memory("MemTotal: 1 MB\nMemAvailable: 1 MB\n").is_err());
        assert!(parse_memory("MemTotal: 1 kB\nMemAvailable: 2 kB\n").is_err());
        assert!(parse_memory(&format!(
            "MemTotal: {} kB\nMemAvailable: 1 kB\n",
            MAX_RESOURCE_BYTES / 1024 + 1
        ))
        .is_err());
    }

    #[test]
    fn rejects_malformed_disk_rows_and_unsafe_cpu_values() {
        assert!(parse_disk(
            "Filesystem 1B-blocks Used Available Use% Mounted on\n/dev/sdb 10 12 0 100% /\n"
        )
        .is_err());
        assert!(parse_disk(
            "Filesystem 1B-blocks Used Available Use% Mounted on\n/dev/sdb 10 8 8 100% /\n"
        )
        .is_err());
        assert!(parse_cpu_sample("cpu 0 0 0 0\n").is_err());
        assert!(parse_cpu_sample("cpu 1 2 three 4\n").is_err());
        assert!(parse_cpu_sample("cpu0 1 2 3 4\n").is_err());
        assert!(parse_disk(&format!(
            "Filesystem 1B-blocks Used Available Use% Mounted on\n/dev/sdb {} 1 1 1% /\n",
            MAX_RESOURCE_BYTES + 1
        ))
        .is_err());
        assert!(parse_memory(&"x".repeat(MAX_RESOURCE_OUTPUT_BYTES + 1)).is_err());
    }

    #[test]
    fn cpu_delta_handles_first_sample_counter_reset_and_guest_fields() {
        let first = parse_cpu_sample("cpu 100 0 20 880 0 0 0 0 10 5\n").unwrap();
        let second = parse_cpu_sample("cpu 130 0 30 940 0 0 0 0 50 40\n").unwrap();
        // guest and guest_nice are already part of user/nice and must not affect the delta.
        assert_eq!(cpu_usage_percent(Some(first), second), Some(40));
        assert_eq!(cpu_usage_percent(None, second), None);
        assert_eq!(cpu_usage_percent(Some(second), first), None);
    }

    #[test]
    fn disk_parser_ignores_mount_path_content() {
        let fixture = "Filesystem 1B-blocks Used Available Use% Mounted on\n/dev/sdb 10 2 8 20% /path with spaces\n";
        assert_eq!(parse_disk(fixture).unwrap(), (2, 10));
    }
}
