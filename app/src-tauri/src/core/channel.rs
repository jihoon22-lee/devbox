pub use devbox_protocol::channel::AppChannel;
pub const CHANNEL: AppChannel = match AppChannel::parse(env!("DEVBOX_COMPILED_CHANNEL")) {
    Some(channel) => channel,
    None => panic!("build validated channel"),
};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tauri_overrides_match_the_shared_installation_identities() {
        for (channel, source) in [
            (AppChannel::Prod, include_str!("../../tauri.conf.json")),
            (
                AppChannel::Accept,
                include_str!("../../tauri.accept.conf.json"),
            ),
            (AppChannel::Dev, include_str!("../../tauri.dev.conf.json")),
        ] {
            let value: serde_json::Value = serde_json::from_str(source).unwrap();
            let schemes: Vec<_> = value["plugins"]["deep-link"]["desktop"]["schemes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            assert!(channel.config().matches_tauri(
                value["identifier"].as_str().unwrap(),
                value["productName"].as_str().unwrap(),
                &schemes
            ));
            for other in [AppChannel::Prod, AppChannel::Accept, AppChannel::Dev] {
                assert_eq!(
                    other.config().matches_tauri(
                        value["identifier"].as_str().unwrap(),
                        value["productName"].as_str().unwrap(),
                        &schemes
                    ),
                    other == channel
                );
            }
        }
    }
    #[test]
    fn compiled_channel_selects_connection_instance() {
        assert_eq!(
            super::super::connection::INSTANCE,
            CHANNEL.config().instance
        );
    }
}
