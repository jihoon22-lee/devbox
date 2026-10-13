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
    pub bottom_terminal: Option<String>,
    pub palette_error: Option<String>,
    pub bottom_terminal_error: Option<String>,
}
impl Default for ShortcutStatus {
    fn default() -> Self {
        Self {
            palette: default_palette(cfg!(debug_assertions)).into(),
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
}
