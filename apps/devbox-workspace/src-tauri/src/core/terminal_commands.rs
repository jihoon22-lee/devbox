//! A retry repeats a resolved visibility action, never toggles a second time.
use product_contract::ProjectContext;
use std::collections::BTreeMap;
type Result<T> = std::result::Result<T, &'static str>;
#[derive(Clone)]
pub struct Receipt {
    pub window: String,
    pub context: Option<ProjectContext>,
    pub hide: bool,
    deadline: u64,
}
#[derive(Default)]
pub struct Summons {
    receipts: BTreeMap<String, Receipt>,
    latest: BTreeMap<String, String>,
}
impl Summons {
    pub fn reserve(
        &mut self,
        operation: &str,
        window: &str,
        context: Option<&ProjectContext>,
        deadline: u64,
        now: u64,
        hide: bool,
    ) -> Result<Receipt> {
        if !uuid::Uuid::parse_str(operation).is_ok_and(|id| id.to_string() == operation)
            || now >= deadline
            || deadline.saturating_sub(now) > 30_000
        {
            return Err("terminal_command_expired");
        }
        self.receipts.retain(|_, receipt| receipt.deadline > now);
        self.latest
            .retain(|_, operation| self.receipts.contains_key(operation));
        if let Some(receipt) = self.receipts.get(operation) {
            if receipt.window != window
                || receipt.context.as_ref() != context
                || receipt.deadline != deadline
            {
                return Err("terminal_command_conflict");
            }
            if self.latest.get(window).map(String::as_str) != Some(operation) {
                return Err("terminal_command_superseded");
            }
            return Ok(receipt.clone());
        }
        if self.receipts.len() >= 1024 {
            return Err("terminal_command_limit");
        }
        let receipt = Receipt {
            window: window.into(),
            context: context.cloned(),
            hide,
            deadline,
        };
        self.receipts.insert(operation.into(), receipt.clone());
        self.latest.insert(window.into(), operation.into());
        Ok(receipt)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_never_reverses_visibility_and_older_actions_cannot_override_newer_ones() {
        let one = "10000000-0000-4000-8000-000000000001";
        let two = "20000000-0000-4000-8000-000000000001";
        let mut owner = Summons::default();
        assert!(
            owner
                .reserve(one, "window", None, 30_000, 1, true)
                .unwrap()
                .hide
        );
        assert!(
            owner
                .reserve(one, "window", None, 30_000, 2, false)
                .unwrap()
                .hide
        );
        assert!(
            !owner
                .reserve(two, "window", None, 30_000, 3, false)
                .unwrap()
                .hide
        );
        assert_eq!(
            owner.reserve(one, "window", None, 30_000, 4, true).err(),
            Some("terminal_command_superseded")
        );
        assert_eq!(
            owner
                .reserve(two, "window", None, 30_000, 30_000, true)
                .err(),
            Some("terminal_command_expired")
        );
    }
}

/// Durable restore CAS: retries observe the first attempt, including its failure.
pub fn restore_generation(
    current: u64,
    last: Option<&str>,
    state: &str,
    expected: u64,
    operation: &str,
) -> Result<Option<u64>> {
    if !uuid::Uuid::parse_str(operation).is_ok_and(|id| id.to_string() == operation)
        || expected >= 9_007_199_254_740_991
    {
        return Err("terminal_restore_invalid");
    }
    if current == expected + 1 && last == Some(operation) {
        return Ok(None);
    }
    if current != expected || last == Some(operation) || !matches!(state, "stopped" | "interrupted")
    {
        return Err("terminal_restore_conflict");
    }
    Ok(Some(current + 1))
}
#[cfg(test)]
mod restore_tests {
    use super::restore_generation as reserve;
    const ONE: &str = "10000000-0000-4000-8000-000000000001";
    const TWO: &str = "20000000-0000-4000-8000-000000000001";
    #[test]
    fn retry_does_not_recreate_failed_or_stopped_windows() {
        assert_eq!(reserve(0, None, "stopped", 0, ONE), Ok(Some(1)));
        for state in ["preparing", "active", "interrupted", "stopped"] {
            assert_eq!(reserve(1, Some(ONE), state, 0, ONE), Ok(None));
        }
        assert!(reserve(1, Some(ONE), "active", 1, TWO).is_err());
        assert!(reserve(1, Some(ONE), "stopped", 0, TWO).is_err());
        assert_eq!(reserve(1, Some(ONE), "interrupted", 1, TWO), Ok(Some(2)));
        assert!(reserve(2, Some(TWO), "stopped", 0, ONE).is_err());
    }
}
