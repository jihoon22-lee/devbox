//! Suite connection preference (pure). Products of one installation connect
//! automatically unless the user turned the connection off for it.
use serde::{Deserialize, Serialize};

pub const FILE: &str = "suite-connection-v2.json";
pub const LEGACY_FILE: &str = "suite-connection-v1.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Auto,
    Off,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preference {
    pub schema: u32,
    pub product: String,
    pub installation: String,
    pub mode: Mode,
}

impl Preference {
    pub fn new(product: &str, installation: &str, mode: Mode) -> Self {
        Self {
            schema: 2,
            product: product.into(),
            installation: installation.into(),
            mode,
        }
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        let value: Self = serde_json::from_slice(bytes).map_err(|_| "suite_preference_invalid")?;
        if value.schema != 2 || value.product.is_empty() || value.installation.is_empty() {
            return Err("suite_preference_invalid");
        }
        Ok(value)
    }
}

/// A preference written for another product or installation does not apply.
pub fn should_auto_connect(
    preference: Option<&Preference>,
    product: &str,
    installation: &str,
) -> bool {
    match preference {
        Some(value) if value.product == product && value.installation == installation => {
            value.mode == Mode::Auto
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connects_by_default_and_after_updates() {
        assert!(should_auto_connect(None, "workspace", "install-a"));
        let auto = Preference::new("workspace", "install-a", Mode::Auto);
        assert!(should_auto_connect(Some(&auto), "workspace", "install-a"));
    }

    #[test]
    fn stays_off_only_for_the_same_product_and_installation() {
        let off = Preference::new("workspace", "install-a", Mode::Off);
        assert!(!should_auto_connect(Some(&off), "workspace", "install-a"));
        assert!(should_auto_connect(Some(&off), "workspace", "install-b"));
        assert!(should_auto_connect(Some(&off), "knowledge", "install-a"));
    }

    #[test]
    fn parses_v2_and_rejects_v1_files() {
        let bytes =
            serde_json::to_vec(&Preference::new("workspace", "install-a", Mode::Off)).unwrap();
        assert_eq!(Preference::parse(&bytes).unwrap().mode, Mode::Off);
        assert!(Preference::parse(
            br#"{"schema":1,"product":"workspace","installation":"a","generation":"g"}"#
        )
        .is_err());
    }
}
