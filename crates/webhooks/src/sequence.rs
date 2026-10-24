use crate::{ResponseRule, ResponseSequenceStep};
use std::collections::HashMap;
/// Process-local cursor state for response sequences.  Keeping this separate
/// from `ResponseRule` makes it explicit that current position is ephemeral:
/// it is never serialized, persisted, or shared with another process.
#[derive(Debug, Default)]
pub struct ResponseSequenceState {
    cursors: HashMap<String, usize>,
}

impl ResponseSequenceState {
    pub fn next_response(&mut self, rule: &ResponseRule) -> ResponseSequenceStep {
        if rule.sequence.is_empty() {
            return rule.response_at(0);
        }
        let cursor = self.cursors.entry(rule.id.clone()).or_insert(0);
        let index = *cursor;
        *cursor = cursor.saturating_add(1);
        rule.response_at(index)
    }

    pub fn reset(&mut self, rule_id: &str) {
        self.cursors.remove(rule_id);
    }
}
