use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Navigation {
    pub sequence: u64,
    pub to: &'static str,
    pub search: AgentSearch,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AgentSearch {
    pub id: String,
    pub view: &'static str,
}
#[derive(Default)]
pub struct PendingNavigation {
    sequence: u64,
    pending: Option<Navigation>,
}
pub fn route_for(uri: &str) -> Option<AgentSearch> {
    let id = uri.strip_prefix("devbox://agents/")?;
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return None;
    }
    Some(AgentSearch {
        id: id.into(),
        view: "list",
    })
}
impl PendingNavigation {
    pub fn enqueue(&mut self, uri: &str) -> Option<Navigation> {
        let search = route_for(uri)?;
        self.sequence = self.sequence.checked_add(1)?;
        let navigation = Navigation {
            sequence: self.sequence,
            to: "/agents",
            search,
        };
        self.pending = Some(navigation.clone());
        Some(navigation)
    }
    pub fn pending(&self) -> Option<Navigation> {
        self.pending.clone()
    }
    pub fn acknowledge(&mut self, sequence: u64) {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.sequence == sequence)
        {
            self.pending = None;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn links_accept_only_one_ascii_agent_id_without_query_or_extra_segments() {
        assert_eq!(
            route_for("devbox://agents/a1").unwrap(),
            AgentSearch {
                id: "a1".into(),
                view: "list"
            }
        );
        for uri in [
            "devbox://agents/",
            "https://agents/a1",
            "devbox://other/a1",
            "devbox://agents/a1/",
            "devbox://agents/a1/b2",
            "devbox://agents/a1?q=b",
            "devbox://agents/a1#x",
            "devbox://agents/a%31",
            "devbox://agents/한글",
            "devbox://agents/a-1",
            "devbox://agents/a1\n",
        ] {
            assert!(route_for(uri).is_none(), "{uri}");
        }
    }
    #[test]
    fn cold_start_is_retained_until_ack_and_stale_ack_cannot_clear_second_instance() {
        let mut queue = PendingNavigation::default();
        let cold = queue.enqueue("devbox://agents/a1").unwrap();
        assert_eq!(queue.pending(), Some(cold.clone()));
        assert!(queue.enqueue("devbox://agents/").is_none());
        assert_eq!(queue.pending(), Some(cold.clone()));
        let second = queue.enqueue("devbox://agents/b2").unwrap();
        assert!(second.sequence > cold.sequence);
        queue.acknowledge(cold.sequence);
        assert_eq!(queue.pending(), Some(second.clone()));
        queue.acknowledge(second.sequence);
        assert_eq!(queue.pending(), None);
        let repeated = queue.enqueue("devbox://agents/b2").unwrap();
        assert!(repeated.sequence > second.sequence);
    }
}
