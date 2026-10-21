//! Installation identities shared by the native app and the instance-aware CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppChannel {
    Prod,
    Accept,
    Dev,
}

#[derive(Debug)]
pub struct ChannelConfig {
    pub instance: &'static str,
    pub identifier: &'static str,
    pub product_name: &'static str,
    pub protocol: &'static str,
    pub storage_name: &'static str,
    pub daemon_unit: &'static str,
    pub tmux_unit: &'static str,
}

impl AppChannel {
    pub const fn parse(value: &str) -> Option<Self> {
        match value.as_bytes() {
            b"prod" => Some(Self::Prod),
            b"accept" => Some(Self::Accept),
            b"dev" => Some(Self::Dev),
            _ => None,
        }
    }
    /// Arbitrary CLI fixtures retain the established shared configuration layout.
    pub fn for_instance(instance: &str) -> Self {
        Self::parse(instance).unwrap_or(Self::Prod)
    }
    pub const fn config(self) -> &'static ChannelConfig {
        match self {
            Self::Prod => &ChannelConfig {
                instance: "prod",
                identifier: "io.github.jihoon22lee.devbox",
                product_name: "Devbox",
                protocol: "devbox",
                storage_name: "devbox",
                daemon_unit: "devbox",
                tmux_unit: "devbox-tmux",
            },
            Self::Accept => &ChannelConfig {
                instance: "accept",
                identifier: "io.github.jihoon22lee.devbox.accept",
                product_name: "Devbox Accept",
                protocol: "devbox-accept",
                storage_name: "devbox-accept",
                daemon_unit: "devbox-accept",
                tmux_unit: "devbox-accept-tmux",
            },
            Self::Dev => &ChannelConfig {
                instance: "dev",
                identifier: "io.github.jihoon22lee.devbox.dev",
                product_name: "Devbox Dev",
                protocol: "devbox-dev",
                storage_name: "devbox-dev",
                daemon_unit: "devbox-dev",
                tmux_unit: "devbox-dev-tmux",
            },
        }
    }
}

impl ChannelConfig {
    /// These are the Tauri keys used by NSIS, single-instance, app data and toast AUMID.
    pub fn matches_tauri(&self, identifier: &str, product_name: &str, schemes: &[&str]) -> bool {
        identifier == self.identifier
            && product_name == self.product_name
            && schemes == [self.protocol]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installation_channels_have_disjoint_os_identities_and_keep_prod_paths() {
        let channels = [AppChannel::Prod, AppChannel::Accept, AppChannel::Dev];
        for (index, channel) in channels.iter().enumerate() {
            let config = channel.config();
            assert_eq!(AppChannel::parse(config.instance), Some(*channel));
            assert!(config.matches_tauri(
                config.identifier,
                config.product_name,
                &[config.protocol]
            ));
            for other in &channels[index + 1..] {
                let other = other.config();
                assert_ne!(config.identifier, other.identifier);
                assert_ne!(config.product_name, other.product_name);
                assert_ne!(config.protocol, other.protocol);
                assert_ne!(config.storage_name, other.storage_name);
                assert_ne!(config.daemon_unit, other.daemon_unit);
                assert_ne!(config.tmux_unit, other.tmux_unit);
                assert!(!config.matches_tauri(
                    other.identifier,
                    other.product_name,
                    &[other.protocol]
                ));
            }
        }
        assert_eq!(AppChannel::Prod.config().storage_name, "devbox");
        assert_eq!(AppChannel::for_instance("test-fixture"), AppChannel::Prod);
        assert_eq!(AppChannel::parse("invalid"), None);
    }
}
