//! A bare menu-key command has no action when the host has no native menu bar.
//! Alt+Space, menu mnemonics, and actual window commands must still reach Windows.
pub(crate) fn ignore_empty_menu_key(
    message: u32,
    command: usize,
    detail: isize,
    has_menu: bool,
) -> bool {
    message == 0x0112 && command & 0xfff0 == 0xf100 && detail == 0 && !has_menu
}

#[cfg(test)]
mod tests {
    use super::ignore_empty_menu_key;
    const SYSTEM_COMMAND: u32 = 0x0112;
    const KEY_MENU: usize = 0xf100;

    #[test]
    fn bare_alt_or_f10_does_not_enter_a_menu_loop_without_a_menu_bar() {
        for reserved_bits in 0..16 {
            assert!(ignore_empty_menu_key(
                SYSTEM_COMMAND,
                KEY_MENU | reserved_bits,
                0,
                false
            ));
        }
    }

    #[test]
    fn native_menu_access_and_window_controls_are_preserved() {
        assert!(!ignore_empty_menu_key(SYSTEM_COMMAND, KEY_MENU, 0, true));
        for detail in [32, 'f' as isize, -1] {
            assert!(!ignore_empty_menu_key(
                SYSTEM_COMMAND,
                KEY_MENU,
                detail,
                false
            ));
        }
        for command in [0xf060, 0xf020, 0xf030, 0xf120, 0xf090] {
            assert!(!ignore_empty_menu_key(SYSTEM_COMMAND, command, 0, false));
        }
        for message in [0x0105, 0x0100, 0x0082, 0x0010] {
            assert!(!ignore_empty_menu_key(message, KEY_MENU, 0, false));
        }
    }
}
