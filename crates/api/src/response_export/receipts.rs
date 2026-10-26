use crate::{collections::export::Authority, history::Receipt};
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
const CAPACITY: usize = 1024;
#[derive(Clone)]
enum State {
    Pending,
    Failed(&'static str),
    Ready(Box<(Receipt, Authority)>),
}
#[derive(Default)]
pub(crate) struct Receipts(Mutex<(HashMap<String, State>, VecDeque<String>)>);
impl Receipts {
    pub(crate) fn begin(&self, id: &str) {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        while state.0.len() >= CAPACITY {
            if let Some(old) = state.1.pop_front() {
                state.0.remove(&old);
            }
        }
        state.1.push_back(id.to_owned());
        state.0.insert(id.to_owned(), State::Pending);
    }
    pub(crate) fn failed(&self, id: &str, reason: &'static str) {
        self.set(id, State::Failed(reason));
    }
    pub(crate) fn ready(&self, id: &str, receipt: Receipt, authority: Authority) {
        self.set(id, State::Ready(Box::new((receipt, authority))));
    }
    fn set(&self, id: &str, next: State) {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(current) = state.0.get_mut(id) {
            *current = next;
        }
    }
    pub(crate) fn get(&self, id: &str) -> Result<(Receipt, Authority), &'static str> {
        match self.0.lock().unwrap_or_else(|p| p.into_inner()).0.get(id) {
            Some(State::Ready(ready)) => Ok((ready.0.clone(), ready.1.clone())),
            Some(State::Pending) => Err("pending"),
            Some(State::Failed(reason)) => Err(reason),
            None => Err("provenance"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipt_eviction_is_bounded_and_late_completion_cannot_restore_authority() {
        let receipts = Receipts::default();
        for n in 0..=CAPACITY {
            receipts.begin(&format!("fixture-{n}"));
        }
        assert_eq!(receipts.get("fixture-0").err(), Some("provenance"));
        receipts.failed("fixture-0", "history");
        assert_eq!(receipts.get("fixture-0").err(), Some("provenance"));
        assert_eq!(
            receipts.get(&format!("fixture-{CAPACITY}")).err(),
            Some("pending")
        );
        let state = receipts.0.lock().expect("lock");
        assert_eq!(state.0.len(), CAPACITY);
        assert_eq!(state.1.len(), CAPACITY);
    }
}
