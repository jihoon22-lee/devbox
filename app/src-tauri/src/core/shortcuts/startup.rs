use super::*;
use crate::core::settings::GlobalShortcuts;

#[derive(Default)]
pub struct Bindings {
    initialized: bool,
    pub status: ShortcutStatus,
    pub registered: Vec<String>,
    pub owners: ShortcutOwners,
}

impl Bindings {
    pub fn initialize(
        &mut self,
        backend: &mut impl Registration,
        settings: io::Result<GlobalShortcuts>,
    ) -> bool {
        if self.initialized {
            return false;
        }
        let settings = match settings {
            Ok(settings) => settings,
            Err(error) => {
                self.status.palette_error = Some(error.to_string());
                return true;
            }
        };
        self.initialized = true;
        self.status = ShortcutStatus {
            palette: settings
                .palette
                .unwrap_or_else(|| default_palette(cfg!(debug_assertions)).into()),
            capture: settings
                .capture
                .unwrap_or_else(|| default_capture(cfg!(debug_assertions)).into()),
            bottom_terminal: settings.bottom_terminal,
            ..Default::default()
        };
        let mut register = |key: &str| -> io::Result<String> {
            let canonical = parse_accelerator(key)?;
            backend.register(&canonical)?;
            Ok(canonical)
        };
        match register(&self.status.palette) {
            Ok(key) => {
                self.status.palette = key.clone();
                self.owners.record(key.clone(), ShortcutAction::Palette);
                self.registered.push(key);
            }
            Err(error) => self.status.palette_error = Some(error.to_string()),
        }
        if let Some(key) = self.status.bottom_terminal.clone() {
            let result = if self
                .registered
                .contains(&parse_accelerator(&key).unwrap_or_default())
            {
                Err(io::Error::other("팔레트와 터미널의 전역 단축키가 같습니다"))
            } else {
                register(&key)
            };
            match result {
                Ok(key) => {
                    self.status.bottom_terminal = Some(key.clone());
                    self.owners
                        .record(key.clone(), ShortcutAction::BottomTerminal);
                    self.registered.push(key);
                }
                Err(error) => self.status.bottom_terminal_error = Some(error.to_string()),
            }
        }
        register_capture(
            backend,
            &mut self.registered,
            &mut self.status,
            &mut self.owners,
        );
        true
    }
}
