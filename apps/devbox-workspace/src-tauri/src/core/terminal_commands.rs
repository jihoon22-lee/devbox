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
