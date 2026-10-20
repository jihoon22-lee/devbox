use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Navigation {
    pub sequence: u64,
    #[serde(flatten)]
    pub target: NavigationTarget,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "to", content = "search")]
pub enum NavigationTarget {
    #[serde(rename = "/agents")]
    Agents(AgentSearch),
    #[serde(rename = "/runs")]
    Runs(RunSearch),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSearch {
    pub id: String,
    pub view: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunSearch {
    pub project_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
}
impl RunSearch {
    fn valid(&self) -> bool {
        !self.project_id.trim().is_empty()
            && self.run_id.as_ref().is_none_or(|id| !id.trim().is_empty())
            && self.task.as_ref().is_none_or(|id| !id.trim().is_empty())
            && (self.run_id.is_some() || self.task.is_some())
    }
}
#[derive(Default)]
pub struct PendingNavigation {
    sequence: u64,
    pending: Option<Navigation>,
}
fn encode(value: &str) -> String {
    use std::fmt::Write;
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            output.push(char::from(byte));
        } else {
            let _ = write!(output, "%{byte:02X}");
        }
    }
    output
}
fn decode(value: &str) -> Option<String> {
    let mut output = Vec::new();
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = char::from(bytes.next()?).to_digit(16)?;
            let low = char::from(bytes.next()?).to_digit(16)?;
            output.push((high * 16 + low) as u8);
        } else if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            output.push(byte);
        } else {
            return None;
        }
    }
    String::from_utf8(output).ok()
}
pub fn run_uri(search: &RunSearch) -> Option<String> {
    if !search.valid() {
        return None;
    }
    let mut uri = format!("devbox://runs?projectId={}", encode(&search.project_id));
    if let Some(id) = &search.run_id {
        uri.push_str(&format!("&runId={}", encode(id)));
    }
    if let Some(task) = &search.task {
        uri.push_str(&format!("&task={}", encode(task)));
    }
    Some(uri)
}
pub fn route_for(uri: &str) -> Option<NavigationTarget> {
    if let Some(id) = uri.strip_prefix("devbox://agents/") {
        if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return None;
        }
        return Some(NavigationTarget::Agents(AgentSearch {
            id: id.into(),
            view: "list".into(),
        }));
    }
    let query = uri.strip_prefix("devbox://runs?")?;
    let mut project_id = None;
    let mut run_id = None;
    let mut task = None;
    for part in query.split('&') {
        let (key, value) = part.split_once('=')?;
        let slot = match key {
            "projectId" => &mut project_id,
            "runId" => &mut run_id,
            "task" => &mut task,
            _ => return None,
        };
        if slot.is_some() {
            return None;
        }
        *slot = Some(decode(value)?);
    }
    let search = RunSearch {
        project_id: project_id?,
        run_id,
        task,
    };
    search.valid().then_some(NavigationTarget::Runs(search))
}
impl PendingNavigation {
    fn enqueue_target(&mut self, target: NavigationTarget) -> Option<Navigation> {
        self.sequence = self.sequence.checked_add(1)?;
        let navigation = Navigation {
            sequence: self.sequence,
            target,
        };
        self.pending = Some(navigation.clone());
        Some(navigation)
    }
    pub fn enqueue_agents(&mut self) -> Option<Navigation> {
        self.enqueue_target(NavigationTarget::Agents(AgentSearch {
            id: String::new(),
            view: "list".into(),
        }))
    }
    pub fn enqueue(&mut self, uri: &str) -> Option<Navigation> {
        self.enqueue_target(route_for(uri)?)
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
            NavigationTarget::Agents(AgentSearch {
                id: "a1".into(),
                view: "list".into()
            })
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
        assert!(queue.enqueue("devbox://agents/b2").unwrap().sequence > second.sequence);
    }
    #[test]
    fn tray_agents_navigation_is_retained_and_replaces_a_previous_deep_link() {
        let mut queue = PendingNavigation::default();
        let link = queue.enqueue("devbox://agents/a1").unwrap();
        let summary = queue.enqueue_agents().unwrap();
        assert_eq!(
            summary.target,
            NavigationTarget::Agents(AgentSearch {
                id: String::new(),
                view: "list".into()
            })
        );
        queue.acknowledge(link.sequence);
        assert_eq!(queue.pending(), Some(summary));
    }
    #[test]
    fn run_targets_roundtrip_unicode_slashes_and_delimiters_and_preserve_pending_ack() {
        for run_id in [Some("run/한글?=&%".into()), None] {
            let search = RunSearch {
                project_id: "프로젝트/a".into(),
                run_id,
                task: Some("test/검사 ?&=#%".into()),
            };
            let uri = run_uri(&search).unwrap();
            assert!(uri.is_ascii());
            assert_eq!(
                route_for(&uri),
                Some(NavigationTarget::Runs(search.clone()))
            );
            let mut queue = PendingNavigation::default();
            let agent = queue.enqueue("devbox://agents/a1").unwrap();
            let run = queue.enqueue(&uri).unwrap();
            queue.acknowledge(agent.sequence);
            assert_eq!(queue.pending(), Some(run.clone()));
            let json = serde_json::to_value(&run).unwrap();
            assert_eq!(json["to"], "/runs");
            assert_eq!(json["search"]["projectId"], search.project_id);
            assert_eq!(json["search"]["task"], search.task.unwrap());
            assert_eq!(
                json["search"].get("runId").is_some(),
                search.run_id.is_some()
            );
            queue.acknowledge(run.sequence);
            assert!(queue.pending().is_none());
        }
    }
    #[test]
    fn run_links_reject_missing_empty_duplicated_unknown_or_malformed_fields() {
        for query in [
            "",
            "projectId=p",
            "runId=r",
            "projectId=&runId=r",
            "projectId=p&task=",
            "projectId=p&task=t&projectId=q",
            "projectId=p&task=t&x=y",
            "projectId=p&runId=%GG",
            "projectId=p&runId=%FF",
            "projectId=p&runId=%",
            "projectId=p&task=a/b",
            "projectId=p&task=a#b",
        ] {
            assert!(
                route_for(&format!("devbox://runs?{query}")).is_none(),
                "{query}"
            );
        }
    }
}
