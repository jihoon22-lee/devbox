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
/// [나중에] on the first-run dialog: the question was asked, and that is all that is saved. It
/// takes no OS closure on purpose, so the autostart registration cannot be read, set or turned on
/// from here, and the recorded choice (`autostart`) stays as it was. If the save fails, the
/// settings are left as they were.
pub fn mark_asked(
    settings: &mut Settings,
    persist: impl FnOnce(&Settings) -> io::Result<()>,
) -> io::Result<()> {
    if settings.autostart_asked {
        return Ok(());
    }
    settings.autostart_asked = true;
    if let Err(error) = persist(settings) {
        settings.autostart_asked = false;
        return Err(error);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::settings;

    #[test]
    fn asking_later_saves_the_question_and_nothing_else() {
        for autostart in [true, false] {
            let mut settings = Settings {
                autostart,
                distro: Some("Ubuntu".into()),
                ..Settings::default()
            };
            let mut saved = None;
            mark_asked(&mut settings, |value| {
                saved = Some((value.autostart_asked, value.autostart, value.distro.clone()));
                Ok(())
            })
            .unwrap();
            assert!(settings.autostart_asked);
            assert_eq!(settings.autostart, autostart);
            assert_eq!(saved, Some((true, autostart, Some("Ubuntu".into()))));
        }
    }

    #[test]
    fn a_question_already_asked_is_not_saved_again() {
        let mut settings = Settings {
            autostart_asked: true,
            ..Settings::default()
        };
        mark_asked(&mut settings, |_| {
            panic!("nothing changed, nothing to save")
        })
        .unwrap();
        assert!(settings.autostart_asked);
    }

    #[test]
    fn a_failed_save_leaves_the_question_unasked() {
        let mut settings = Settings::default();
        assert!(mark_asked(&mut settings, |_| Err(io::Error::other("disk full"))).is_err());
        assert!(!settings.autostart_asked);
    }

    #[test]
    fn the_saved_file_keeps_every_other_setting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        settings::update(&path, |settings| {
            settings.distro = Some("Ubuntu".into());
            settings.autostart = false;
            settings.tray_notice_shown = true;
        })
        .unwrap();
        settings::with_settings(&path, |settings| {
            mark_asked(settings, |settings| settings::save(&path, settings))
        })
        .unwrap();
        let saved = settings::load(&path).unwrap();
        assert!(saved.autostart_asked && saved.tray_notice_shown);
        assert!(!saved.autostart);
        assert_eq!(saved.distro.as_deref(), Some("Ubuntu"));
    }

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
