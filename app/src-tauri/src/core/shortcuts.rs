use global_hotkey::hotkey::{HotKey, Modifiers};
use serde::Serialize;
use std::io;

pub fn default_palette(debug: bool) -> &'static str {
    if debug {
        "Ctrl+Alt+Shift+Space"
    } else {
        "Ctrl+Alt+Space"
    }
}
pub fn default_capture(debug: bool) -> &'static str {
    if debug {
        "Ctrl+Alt+Shift+N"
    } else {
        "Ctrl+Alt+N"
    }
}
pub fn parse_accelerator(input: &str) -> io::Result<String> {
    let shortcut = input.parse::<HotKey>().map_err(io::Error::other)?;
    if !shortcut
        .mods
        .intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT)
    {
        return Err(io::Error::other(
            "전역 단축키에는 Ctrl·Alt·Shift 중 하나가 필요합니다",
        ));
    }
    if input.split('+').count() != shortcut.mods.bits().count_ones() as usize + 1 {
        return Err(io::Error::other("같은 보조 키를 두 번 지정할 수 없습니다"));
    }
    let mut parts = Vec::new();
    for (modifier, name) in [
        (Modifiers::CONTROL, "Ctrl"),
        (Modifiers::ALT, "Alt"),
        (Modifiers::SHIFT, "Shift"),
        (Modifiers::SUPER, "Super"),
    ] {
        if shortcut.mods.contains(modifier) {
            parts.push(name.to_owned());
        }
    }
    let key = shortcut.key.to_string();
    parts.push(
        key.strip_prefix("Key")
            .or_else(|| key.strip_prefix("Digit"))
            .unwrap_or(&key)
            .into(),
    );
    Ok(parts.join("+"))
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutStatus {
    pub palette: String,
    pub capture: String,
    pub bottom_terminal: Option<String>,
    pub palette_error: Option<String>,
    pub capture_error: Option<String>,
    pub bottom_terminal_error: Option<String>,
}
impl Default for ShortcutStatus {
    fn default() -> Self {
        Self {
            palette: default_palette(cfg!(debug_assertions)).into(),
            capture: default_capture(cfg!(debug_assertions)).into(),
            capture_error: None,
            bottom_terminal: None,
            palette_error: None,
            bottom_terminal_error: None,
        }
    }
}
pub trait Registration {
    fn register(&mut self, accelerator: &str) -> io::Result<()>;
    fn unregister(&mut self, accelerator: &str) -> io::Result<()>;
}
/// Action ownership is committed only for accepted OS registrations. Diagnostic status may
/// contain rejected settings, so it must never decide which action a registered key invokes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutAction {
    Palette,
    BottomTerminal,
    Capture,
}
#[derive(Clone, Debug, Default)]
pub struct ShortcutOwners(Vec<(String, ShortcutAction)>);
impl ShortcutOwners {
    pub fn record(&mut self, key: String, action: ShortcutAction) {
        self.0.retain(|(_, old)| *old != action);
        self.0.push((key, action));
    }
    pub fn action(&self, id: u32, registered: &[String]) -> Option<ShortcutAction> {
        let matches = |key: &str| key.parse::<HotKey>().is_ok_and(|key| key.id == id);
        if !registered.iter().any(|key| matches(key)) {
            return None;
        }
        self.0
            .iter()
            .find(|(key, _)| matches(key))
            .map(|(_, action)| *action)
    }
}
/// Persist and replace keys first. On failure, old owners remain; dispatch additionally checks
/// the actual registration list, which can be incomplete after a failed OS rollback.
pub fn replace_action_bindings(
    backend: &mut impl Registration,
    registered: &mut Vec<String>,
    owners: &mut ShortcutOwners,
    next: &[(String, ShortcutAction)],
    persist: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let keys = next.iter().map(|(key, _)| key.clone()).collect::<Vec<_>>();
    replace_bindings(backend, registered, &keys, persist)?;
    owners.0 = next.to_vec();
    Ok(())
}

/// Canonical comparisons also reject different spellings of the same accelerator.
pub fn capture_binding(palette: &str, bottom: Option<&str>, capture: &str) -> io::Result<String> {
    let key = parse_accelerator(capture)?;
    if parse_accelerator(palette).ok().as_deref() == Some(&key)
        || bottom.and_then(|b| parse_accelerator(b).ok()).as_deref() == Some(&key)
    {
        return Err(io::Error::other("빠른 캡처와 다른 전역 단축키가 같습니다"));
    }
    Ok(key)
}
pub fn register_capture(
    backend: &mut impl Registration,
    registered: &mut Vec<String>,
    status: &mut ShortcutStatus,
    owners: &mut ShortcutOwners,
) {
    match capture_binding(
        &status.palette,
        status.bottom_terminal.as_deref(),
        &status.capture,
    )
    .and_then(|key| backend.register(&key).map(|()| key))
    {
        Ok(key) => {
            status.capture = key.clone();
            status.capture_error = None;
            owners.record(key.clone(), ShortcutAction::Capture);
            registered.push(key);
        }
        Err(error) => status.capture_error = Some(error.to_string()),
    }
}

/// Track each successful OS operation, including incomplete rollback, so retries repair missing bindings.
pub fn replace_bindings(
    backend: &mut impl Registration,
    registered: &mut Vec<String>,
    new: &[String],
    persist: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let old = registered.clone();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let result = (|| {
        for key in new.iter().filter(|key| !old.contains(key)) {
            backend.register(key)?;
            registered.push(key.clone());
            added.push(key);
        }
        for key in old.iter().filter(|key| !new.contains(key)) {
            backend.unregister(key)?;
            registered.retain(|registered| registered != key);
            removed.push(key);
        }
        persist()
    })();
    if let Err(error) = result {
        let mut failures = vec![error.to_string()];
        for key in removed {
            if let Err(error) = backend.register(key) {
                failures.push(format!("단축키 복원 실패: {error}"));
            } else {
                registered.push(key.clone());
            }
        }
        for key in added {
            if let Err(error) = backend.unregister(key) {
                failures.push(format!("새 단축키 해제 실패: {error}"));
            } else {
                registered.retain(|registered| registered != key);
            }
        }
        return Err(io::Error::other(failures.join(" · ")));
    }
    *registered = new.to_vec();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_modifiers_and_rejects_invalid_keys() {
        assert_eq!(
            parse_accelerator("alt+control+space").unwrap(),
            "Ctrl+Alt+Space"
        );
        assert_eq!(
            parse_accelerator("Ctrl+Alt+Shift+Space").unwrap(),
            "Ctrl+Alt+Shift+Space"
        );
        for bad in [
            "Space",
            "Super+A",
            "Ctrl+Alt+Nope",
            "Ctrl++A",
            "Ctrl+A+B",
            "Ctrl+Ctrl+A",
        ] {
            assert!(parse_accelerator(bad).is_err(), "{bad}");
        }
        assert_eq!(default_palette(true), "Ctrl+Alt+Shift+Space");
        assert_eq!(default_palette(false), "Ctrl+Alt+Space");
        let status = ShortcutStatus::default();
        assert_eq!(status.palette, default_palette(cfg!(debug_assertions)));
        assert!(status.bottom_terminal.is_none());
        assert!(status.palette_error.is_none() && status.bottom_terminal_error.is_none());
    }
    #[derive(Default)]
    struct Mock {
        registered: Vec<String>,
        reject: Option<String>,
        reject_remove: Option<String>,
    }
    impl Registration for Mock {
        fn register(&mut self, key: &str) -> io::Result<()> {
            if self.reject.as_deref() == Some(key) {
                return Err(io::Error::other("다른 프로그램이 쓰는 중"));
            }
            self.registered.push(key.into());
            Ok(())
        }
        fn unregister(&mut self, key: &str) -> io::Result<()> {
            if self.reject_remove.as_deref() == Some(key) {
                return Err(io::Error::other("unregister failed"));
            }
            self.registered.retain(|item| item != key);
            Ok(())
        }
    }
    #[test]
    fn conflicts_and_persistence_failure_keep_the_working_binding() {
        let old: Vec<String> = vec!["Ctrl+Alt+Space".into()];
        let new: Vec<String> = vec!["Ctrl+Alt+A".into()];
        let mut known = old.clone();
        let mut mock = Mock {
            registered: old.clone(),
            reject: Some(new[0].clone()),
            ..Mock::default()
        };
        assert!(replace_bindings(&mut mock, &mut known, &new, || Ok(())).is_err());
        assert_eq!(mock.registered, old);
        mock.reject = None;
        assert!(
            replace_bindings(&mut mock, &mut known, &new, || Err(io::Error::other(
                "disk full"
            )))
            .is_err()
        );
        assert_eq!(mock.registered, old);
        replace_bindings(&mut mock, &mut known, &new, || Ok(())).unwrap();
        assert_eq!(mock.registered, new);
    }
    #[test]
    fn failed_restore_is_tracked_so_retry_re_registers_the_missing_binding() {
        let old = vec!["Ctrl+Alt+Space".to_owned()];
        let new = vec!["Ctrl+Alt+A".to_owned()];
        let mut known = old.clone();
        let mut mock = Mock {
            registered: old.clone(),
            reject: Some(old[0].clone()),
            ..Mock::default()
        };
        let failure = replace_bindings(&mut mock, &mut known, &new, || {
            Err(io::Error::other("disk full"))
        });
        assert!(failure.unwrap_err().to_string().contains("복원 실패"));
        assert!(mock.registered.is_empty());
        assert!(
            known.is_empty(),
            "failed restore must not leave a claimed registration"
        );
        mock.reject = None;
        replace_bindings(&mut mock, &mut known, &old, || Ok(())).unwrap();
        assert_eq!(
            mock.registered, old,
            "retry must restore the missing OS registration"
        );
        assert_eq!(known, mock.registered);
        mock.reject = Some(old[0].clone());
        mock.reject_remove = Some(new[0].clone());
        let failure = replace_bindings(&mut mock, &mut known, &new, || {
            Err(io::Error::other("disk full"))
        });
        assert!(
            failure
                .unwrap_err()
                .to_string()
                .contains("새 단축키 해제 실패")
        );
        assert_eq!(
            known, new,
            "failed cleanup must retain the actual new registration"
        );
        assert_eq!(known, mock.registered);
        mock.reject = None;
        mock.reject_remove = None;
        replace_bindings(&mut mock, &mut known, &old, || Ok(())).unwrap();
        assert_eq!(known, old);
        assert_eq!(mock.registered, old);
    }
    #[test]
    fn partial_registration_and_unregister_failure_roll_back_without_saving() {
        let old = vec!["Ctrl+Alt+Space".to_owned()];
        let new = vec!["Ctrl+Alt+A".to_owned(), "Ctrl+Alt+B".to_owned()];
        let mut known = old.clone();
        let mut mock = Mock {
            registered: old.clone(),
            reject: Some(new[1].clone()),
            ..Mock::default()
        };
        assert!(replace_bindings(&mut mock, &mut known, &new, || panic!("must not save")).is_err());
        assert_eq!(mock.registered, old);
        mock.reject = None;
        mock.reject_remove = Some(old[0].clone());
        assert!(replace_bindings(&mut mock, &mut known, &new, || panic!("must not save")).is_err());
        assert_eq!(mock.registered, old);
    }
    #[test]
    fn capture_shortcut_conflicts_with_palette_are_reported() {
        let mut backend = Mock::default();
        let mut registered = vec![];
        let mut owners = ShortcutOwners::default();
        let mut status = ShortcutStatus {
            palette: "Ctrl+Alt+N".into(),
            capture: "Ctrl+Alt+N".into(),
            ..Default::default()
        };
        register_capture(&mut backend, &mut registered, &mut status, &mut owners);
        assert!(status.capture_error.is_some());
        assert!(backend.registered.is_empty());
        assert!(registered.is_empty());
        status.palette = "Ctrl+Alt+Space".into();
        status.bottom_terminal = Some("Alt+Ctrl+N".into());
        status.capture = "Ctrl+Alt+N".into();
        register_capture(&mut backend, &mut registered, &mut status, &mut owners);
        assert!(status.capture_error.is_some());
        assert!(backend.registered.is_empty());
        status.bottom_terminal = None;
        register_capture(&mut backend, &mut registered, &mut status, &mut owners);
        assert!(status.capture_error.is_none());
        assert_eq!(registered, ["Ctrl+Alt+N"]);
        assert_eq!(default_capture(false), "Ctrl+Alt+N");
        assert_eq!(default_capture(true), "Ctrl+Alt+Shift+N");
    }
    #[test]
    fn rejected_capture_alias_does_not_take_palette_or_terminal_ownership() {
        for action in [ShortcutAction::Palette, ShortcutAction::BottomTerminal] {
            let key = "Ctrl+Alt+N".to_owned();
            let mut status = ShortcutStatus {
                palette: if action == ShortcutAction::Palette {
                    key.clone()
                } else {
                    "Ctrl+Alt+Space".into()
                },
                bottom_terminal: (action == ShortcutAction::BottomTerminal).then(|| key.clone()),
                capture: "Alt+Ctrl+N".into(),
                ..Default::default()
            };
            let mut backend = Mock {
                registered: vec![key.clone()],
                ..Default::default()
            };
            let mut registered = backend.registered.clone();
            let mut owners = ShortcutOwners::default();
            owners.record(key.clone(), action);
            register_capture(&mut backend, &mut registered, &mut status, &mut owners);
            assert!(status.capture_error.is_some());
            assert_eq!(backend.registered.as_slice(), std::slice::from_ref(&key));
            let pressed = "Control+Alt+N".parse::<HotKey>().unwrap().id;
            assert_eq!(owners.action(pressed, &registered), Some(action));
        }
    }

    #[test]
    fn failed_capture_reconfiguration_preserves_registered_action_ownership() {
        let old = vec![
            ("Ctrl+Alt+Space".to_owned(), ShortcutAction::Palette),
            ("Ctrl+Alt+N".to_owned(), ShortcutAction::Capture),
        ];
        let new = vec![
            ("Ctrl+Alt+Space".to_owned(), ShortcutAction::Palette),
            ("Ctrl+Alt+Shift+F9".to_owned(), ShortcutAction::Capture),
        ];
        let mut owners = ShortcutOwners::default();
        let mut registered = Vec::new();
        let mut backend = Mock::default();
        replace_action_bindings(&mut backend, &mut registered, &mut owners, &old, || Ok(()))
            .unwrap();
        backend.reject = Some(new[1].0.clone());
        assert!(
            replace_action_bindings(&mut backend, &mut registered, &mut owners, &new, || panic!(
                "must not save"
            ))
            .is_err()
        );
        assert_eq!(
            owners.action(old[1].0.parse::<HotKey>().unwrap().id, &registered),
            Some(ShortcutAction::Capture)
        );
        backend.reject = None;
        assert!(
            replace_action_bindings(&mut backend, &mut registered, &mut owners, &new, || Err(
                io::Error::other("disk full")
            ))
            .is_err()
        );
        assert_eq!(
            owners.action(old[1].0.parse::<HotKey>().unwrap().id, &registered),
            Some(ShortcutAction::Capture)
        );
        assert_eq!(
            owners.action(new[1].0.parse::<HotKey>().unwrap().id, &registered),
            None
        );
        // If OS rollback itself fails, neither an absent old key nor an unaccepted
        // leftover new registration gains an action merely from diagnostic settings.
        backend.reject = Some(old[1].0.clone());
        backend.reject_remove = Some(new[1].0.clone());
        assert!(
            replace_action_bindings(&mut backend, &mut registered, &mut owners, &new, || Err(
                io::Error::other("disk full")
            ))
            .is_err()
        );
        assert_eq!(
            owners.action(old[1].0.parse::<HotKey>().unwrap().id, &registered),
            None
        );
        assert_eq!(
            owners.action(new[1].0.parse::<HotKey>().unwrap().id, &registered),
            None
        );
        backend.reject = None;
        backend.reject_remove = None;
        replace_action_bindings(&mut backend, &mut registered, &mut owners, &new, || Ok(()))
            .unwrap();
        assert_eq!(
            owners.action(old[1].0.parse::<HotKey>().unwrap().id, &registered),
            None
        );
        assert_eq!(
            owners.action(new[1].0.parse::<HotKey>().unwrap().id, &registered),
            Some(ShortcutAction::Capture)
        );
    }
}
