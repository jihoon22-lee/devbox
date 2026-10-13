use super::settings::Settings;
use std::io;

pub fn change_autostart(
    settings: &mut Settings,
    current: bool,
    enabled: bool,
    first_run: bool,
    mut change_os: impl FnMut(bool) -> io::Result<()>,
    persist: impl FnOnce(&Settings) -> io::Result<()>,
) -> io::Result<()> {
    if current != enabled
        && let Err(error) = change_os(enabled)
    {
        if let Err(rollback) = change_os(current) {
            return Err(io::Error::other(format!(
                "{error} · 자동 시작 복원 실패: {rollback}"
            )));
        }
        return Err(error);
    }
    let old = settings.clone();
    settings.autostart = enabled;
    settings.autostart_asked |= first_run;
    if let Err(error) = persist(settings) {
        *settings = old;
        if current != enabled
            && let Err(rollback) = change_os(current)
        {
            return Err(io::Error::other(format!(
                "{error} · 자동 시작 복원 실패: {rollback}"
            )));
        }
        return Err(error);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_run_is_only_completed_after_os_and_settings_succeed() {
        let mut settings = Settings::default();
        assert!(!settings.autostart_asked && settings.autostart);
        assert!(
            change_autostart(
                &mut settings,
                false,
                true,
                true,
                |_| Err(io::Error::other("registry failed")),
                |_| Ok(())
            )
            .is_err()
        );
        assert!(!settings.autostart_asked);
        let mut calls = vec![];
        assert!(
            change_autostart(
                &mut settings,
                false,
                true,
                true,
                |value| {
                    calls.push(value);
                    Ok(())
                },
                |_| Err(io::Error::other("disk full"))
            )
            .is_err()
        );
        assert_eq!(calls, [true, false]);
        assert!(!settings.autostart_asked);
        change_autostart(
            &mut settings,
            false,
            false,
            true,
            |_| panic!("already disabled"),
            |_| Ok(()),
        )
        .unwrap();
        assert!(settings.autostart_asked && !settings.autostart);
    }
}
