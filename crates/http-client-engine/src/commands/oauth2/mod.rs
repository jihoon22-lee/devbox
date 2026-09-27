pub mod cache;
pub mod config;
pub mod flows;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::watch;
#[derive(Default)]
pub struct OAuth2State {
    active: Mutex<Option<(String, watch::Sender<bool>)>>,
    cache: OnceLock<cache::TokenCache>,
}
struct FlowLease {
    state: Arc<OAuth2State>,
    id: String,
}
impl Drop for FlowLease {
    fn drop(&mut self) {
        if let Ok(mut active) = self.state.active.lock() {
            if active.as_ref().is_some_and(|(id, _)| id == &self.id) {
                *active = None;
            }
        }
    }
}
impl OAuth2State {
    fn begin(
        self: &Arc<Self>,
        id: &str,
    ) -> Result<(FlowLease, watch::Receiver<bool>), &'static str> {
        if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
            return Err("oauth2_config_invalid");
        }
        let mut active = self.active.lock().map_err(|_| "oauth2_busy")?;
        if active.is_some() {
            return Err("oauth2_busy");
        }
        let (sender, receiver) = watch::channel(false);
        *active = Some((id.into(), sender));
        Ok((
            FlowLease {
                state: self.clone(),
                id: id.into(),
            },
            receiver,
        ))
    }
    pub fn cancel(&self, id: &str) -> Result<(), &'static str> {
        let active = self.active.lock().map_err(|_| "oauth2_busy")?;
        if let Some((current, sender)) = active.as_ref().filter(|(current, _)| current == id) {
            let _ = current;
            let _ = sender.send(true);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_flow_owns_cancellation_until_its_lease_is_released() {
        let state = Arc::new(OAuth2State::default());
        let (lease, receiver) = state.begin("a").unwrap();
        assert!(matches!(state.begin("b"), Err("oauth2_busy")));
        state.cancel("other").unwrap();
        assert!(!*receiver.borrow());
        state.cancel("a").unwrap();
        assert!(*receiver.borrow());
        drop(lease);
        assert!(state.begin("b").is_ok());
    }
}
