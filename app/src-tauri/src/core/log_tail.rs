//! The newest journal lines of this channel's daemon, for the connection screen's details.
use devbox_protocol::channel::AppChannel;

pub const MAX_LINES: u16 = 200;
pub const MAX_BYTES: usize = 64 * 1024;

/// Arguments after `wsl.exe -d <distro>`. The unit is the one `devbox setup` enables for
/// `instance`; `--exec` runs journalctl without a Linux shell. More than `MAX_LINES` is clamped.
pub fn args(instance: &str, lines: u16) -> Vec<String> {
    let unit = AppChannel::for_instance(instance).config().daemon_unit;
    let unit = format!("{unit}@{instance}.service");
    let lines = lines.min(MAX_LINES).to_string();
    [
        "--exec",
        "journalctl",
        "--user",
        "-u",
        &unit,
        "-n",
        &lines,
        "--no-pager",
        "-o",
        "cat",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// Keeps the newest `MAX_BYTES` at most, starting on a character boundary.
pub fn cap(text: &str) -> &str {
    let mut start = text.len().saturating_sub(MAX_BYTES);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn log_tail_command_targets_the_instance_unit_and_caps_output() {
        for (instance, unit) in [
            ("prod", "devbox@prod.service"),
            ("accept", "devbox-accept@accept.service"),
            ("dev", "devbox-dev@dev.service"),
        ] {
            assert_eq!(
                args(instance, 500),
                [
                    "--exec",
                    "journalctl",
                    "--user",
                    "-u",
                    unit,
                    "-n",
                    "200",
                    "--no-pager",
                    "-o",
                    "cat"
                ]
            );
        }
        // The app reads the unit that setup enabled for its own DEVBOX_INSTANCE.
        let channel = crate::core::channel::CHANNEL.config();
        let own = args(crate::core::connection::INSTANCE, 20);
        assert_eq!(
            own[4],
            format!("{}@{}.service", channel.daemon_unit, channel.instance)
        );
        assert_eq!(own[6], "20");

        assert_eq!(cap("short log\n"), "short log\n");
        // 90,004 bytes; the newest 64 KiB would start inside a three-byte character.
        let long = format!("a{}끝", "가".repeat(30_000));
        let kept = cap(&long);
        assert_eq!(MAX_BYTES, 64 * 1024);
        assert_eq!(kept.len(), 65_535);
        assert!(kept.starts_with('가') && kept.ends_with('끝'));
        assert!(long.ends_with(kept));
    }
}
