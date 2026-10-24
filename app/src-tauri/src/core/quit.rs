use std::collections::BTreeMap;

pub fn note_window(label: &str) -> bool {
    label == "main" || label.starts_with("popout-note-") || label.starts_with("popout-code-")
}
#[derive(Default)]
pub struct Quit {
    sequence: u64,
    registrations: BTreeMap<String, u64>,
    round: Option<Round>,
    pub approved: bool,
}
struct Round {
    id: u64,
    windows: BTreeMap<String, (u64, Option<bool>)>,
    failed: bool,
}
#[derive(Debug, PartialEq)]
pub enum Status {
    Waiting,
    Failed,
    Ready,
}
impl Quit {
    pub fn register(&mut self, label: &str) -> u64 {
        self.sequence += 1;
        self.invalidate(label);
        self.registrations.insert(label.into(), self.sequence);
        self.sequence
    }
    pub fn unregister(&mut self, label: &str, lease: u64) {
        if self.registrations.get(label) == Some(&lease) {
            self.invalidate(label);
            self.registrations.remove(label);
        }
    }
    pub fn invalidate(&mut self, label: &str) {
        if let Some(round) = &mut self.round
            && round.windows.contains_key(label)
        {
            round.failed = true;
        }
    }
    pub fn start(&mut self, labels: Vec<String>) -> Option<u64> {
        if self.round.is_some() || self.approved {
            return None;
        }
        let windows: Option<BTreeMap<_, _>> = labels
            .into_iter()
            .map(|label| {
                self.registrations
                    .get(&label)
                    .map(|lease| (label.clone(), (*lease, None)))
            })
            .collect();
        self.sequence += 1;
        self.round = Some(Round {
            id: self.sequence,
            windows: windows?,
            failed: false,
        });
        Some(self.sequence)
    }
    pub fn ack(&mut self, label: &str, lease: u64, id: u64, safe: bool) {
        let Some(round) = &mut self.round else {
            return;
        };
        if round.id != id {
            return;
        }
        if let Some((expected, answer)) = round.windows.get_mut(label)
            && *expected == lease
            && answer.is_none()
        {
            *answer = Some(safe);
        }
    }
    pub fn status(&self, id: u64, labels: &[String]) -> Status {
        let Some(round) = &self.round else {
            return Status::Failed;
        };
        if round.id != id
            || round.failed
            || labels.len() != round.windows.len()
            || labels
                .iter()
                .any(|label| !round.windows.contains_key(label))
            || round.windows.values().any(|(_, ack)| *ack == Some(false))
        {
            return Status::Failed;
        }
        if round.windows.values().all(|(_, ack)| *ack == Some(true)) {
            Status::Ready
        } else {
            Status::Waiting
        }
    }
    pub fn finish(&mut self, id: u64, approved: bool) {
        if self.round.as_ref().is_some_and(|round| round.id == id) {
            self.round = None;
            self.approved = approved;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn windows() -> Vec<String> {
        vec!["main".into(), "popout-note-a".into()]
    }
    #[test]
    fn all_windows_must_ack_same_round_and_lease() {
        assert!(note_window("main"));
        assert!(note_window("popout-note-a"));
        assert!(note_window("popout-code-a"));
        assert!(!note_window("popout-terminal-a"));
        let mut quit = Quit::default();
        let main = quit.register("main");
        let popout = quit.register("popout-note-a");
        let round = quit.start(windows()).expect("registered window round");
        assert!(quit.start(windows()).is_none());
        quit.ack("main", main, round, true);
        quit.ack("popout-note-a", main, round, true);
        quit.ack("other", popout, round, true);
        quit.ack("popout-note-a", popout, round - 1, true);
        assert_eq!(quit.status(round, &windows()), Status::Waiting);
        assert!(!quit.approved);
        quit.ack("popout-note-a", popout, round, true);
        assert_eq!(quit.status(round, &windows()), Status::Ready);
        quit.finish(round, true);
        assert!(quit.approved);
        assert!(quit.start(windows()).is_none());
    }
    #[test]
    fn failure_rolls_back_and_stale_ack_cannot_approve_retry() {
        let mut quit = Quit::default();
        let main = quit.register("main");
        let popout = quit.register("popout-note-a");
        let first = quit.start(windows()).expect("registered window round");
        quit.ack("main", main, first, true);
        quit.ack("popout-note-a", popout, first, false);
        assert_eq!(quit.status(first, &windows()), Status::Failed);
        quit.finish(first, false);
        let next = quit.start(windows()).expect("registered window round");
        quit.ack("main", main, first, true);
        quit.ack("popout-note-a", popout, first, true);
        assert_eq!(quit.status(next, &windows()), Status::Waiting);
        assert!(!quit.approved);
    }
    #[test]
    fn startup_without_a_document_session_and_timeout_retry() {
        let mut quit = Quit::default();
        let lease = quit.register("main");
        let labels = vec!["main".into()];
        let timed_out = quit.start(labels.clone()).expect("startup round");
        assert_eq!(quit.status(timed_out, &labels), Status::Waiting);
        quit.finish(timed_out, false);
        assert!(!quit.approved);
        let next = quit.start(labels.clone()).expect("retry round");
        quit.ack("main", lease, timed_out, true);
        assert_eq!(quit.status(next, &labels), Status::Waiting);
        quit.ack("main", lease, next, true);
        assert_eq!(quit.status(next, &labels), Status::Ready);
    }
    #[test]
    fn unavailable_reloaded_destroyed_or_new_windows_cannot_exit() {
        let mut quit = Quit::default();
        assert!(quit.start(windows()).is_none());
        let main = quit.register("main");
        let popout = quit.register("popout-note-a");
        let round = quit.start(windows()).expect("registered window round");
        quit.ack("main", main, round, true);
        quit.ack("popout-note-a", popout, round, true);
        let mut extra = windows();
        extra.push("popout-note-new".into());
        assert_eq!(quit.status(round, &extra), Status::Failed);
        let next_lease = quit.register("popout-note-a");
        quit.unregister("popout-note-a", popout);
        assert_eq!(quit.status(round, &windows()), Status::Failed);
        quit.finish(round, false);
        let next = quit.start(windows()).expect("registered window round");
        quit.ack("popout-note-a", next_lease, next, true);
        quit.invalidate("main");
        assert_eq!(quit.status(next, &windows()), Status::Failed);
    }
}
