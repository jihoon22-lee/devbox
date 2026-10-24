use super::rules::*;
use std::cmp::Ordering;
use std::collections::HashMap;
pub fn matches(rule: &ResponseRule, method: &str, path: &str) -> bool {
    // The native request parser and replay client emit/accept ASCII targets
    // only.  Keep direct matcher callers fail-closed even if an invalid rule
    // is constructed outside the storage validator.
    if !rule.path.is_ascii() || !path.is_ascii() {
        return false;
    }
    if let Some(m) = &rule.method {
        if !m.eq_ignore_ascii_case(method) {
            return false;
        }
    }
    rule.path == path
        || (rule.path.ends_with('*') && path.starts_with(&rule.path[..rule.path.len() - 1]))
}

/// Stable precedence shared by the live listener, list UI, and conflict
/// preview. `Ordering::Less` means `left` is evaluated before `right`.
pub fn compare_rule_precedence(left: &ResponseRule, right: &ResponseRule) -> Ordering {
    right
        .priority
        .cmp(&left.priority)
        .then_with(|| path_is_exact(right).cmp(&path_is_exact(left)))
        .then_with(|| right.method.is_some().cmp(&left.method.is_some()))
        .then_with(|| wildcard_prefix_len(right).cmp(&wildcard_prefix_len(left)))
        .then_with(|| left.id.as_bytes().cmp(right.id.as_bytes()))
}

pub fn select_matching_rule<'a, I>(rules: I, method: &str, path: &str) -> Option<&'a ResponseRule>
where
    I: IntoIterator<Item = &'a ResponseRule>,
{
    rules
        .into_iter()
        .filter(|rule| matches(rule, method, path))
        .min_by(|left, right| compare_rule_precedence(left, right))
}

/// Assign a stable id to a new candidate, validate its projected collection,
/// and report every overlapping existing rule. Callers must hold the rule-map
/// lock from this function through the confirmed upsert.
pub fn plan_upsert(
    rules: &HashMap<String, ResponseRule>,
    mut candidate: ResponseRule,
) -> Result<RuleUpsertPlan, RuleValidationError> {
    if candidate.id.is_empty() {
        candidate.id = uuid::Uuid::new_v4().to_string();
    }
    let mut projected = rules.clone();
    upsert(&mut projected, candidate.clone())?;

    let mut conflicts = rules
        .values()
        .filter(|existing| existing.id != candidate.id && rules_overlap(&candidate, existing))
        .map(|existing| conflict(&candidate, existing))
        .collect::<Vec<_>>();
    conflicts.sort_by(|left, right| left.existing_rule_id.cmp(&right.existing_rule_id));
    let preview = RuleConflictPreview {
        candidate_id: candidate.id.clone(),
        requires_confirmation: !conflicts.is_empty(),
        conflicts,
    };
    Ok(RuleUpsertPlan { candidate, preview })
}

pub fn rules_overlap(left: &ResponseRule, right: &ResponseRule) -> bool {
    methods_overlap(left.method.as_deref(), right.method.as_deref())
        && paths_overlap(&left.path, &right.path)
}

fn methods_overlap(left: Option<&str>, right: Option<&str>) -> bool {
    left.is_none()
        || right.is_none()
        || left.is_some_and(|left| right.is_some_and(|right| left.eq_ignore_ascii_case(right)))
}

fn paths_overlap(left: &str, right: &str) -> bool {
    match (path_pattern(left), path_pattern(right)) {
        ((left, false), (right, false)) => left == right,
        ((exact, false), (prefix, true)) | ((prefix, true), (exact, false)) => {
            exact.starts_with(prefix)
        }
        ((left, true), (right, true)) => left.starts_with(right) || right.starts_with(left),
    }
}

fn rule_covers(covering: &ResponseRule, covered: &ResponseRule) -> bool {
    let method_covers = covering.method.is_none()
        || covering.method.as_deref().is_some_and(|left| {
            covered
                .method
                .as_deref()
                .is_some_and(|right| left.eq_ignore_ascii_case(right))
        });
    if !method_covers {
        return false;
    }
    match (path_pattern(&covering.path), path_pattern(&covered.path)) {
        ((left, false), (right, false)) => left == right,
        ((_, false), (_, true)) => false,
        ((prefix, true), (exact, false)) => exact.starts_with(prefix),
        ((left, true), (right, true)) => right.starts_with(left),
    }
}

fn conflict(candidate: &ResponseRule, existing: &ResponseRule) -> RuleConflict {
    let candidate_wins = compare_rule_precedence(candidate, existing) == Ordering::Less;
    let (winner, loser) = if candidate_wins {
        (candidate, existing)
    } else {
        (existing, candidate)
    };
    let kind = if candidate_wins && rule_covers(candidate, existing) {
        RuleConflictKind::CandidateShadowsExisting
    } else if !candidate_wins && rule_covers(existing, candidate) {
        RuleConflictKind::ExistingShadowsCandidate
    } else {
        RuleConflictKind::PartialOverlap
    };
    RuleConflict {
        existing_rule_id: existing.id.clone(),
        winner_rule_id: winner.id.clone(),
        loser_rule_id: loser.id.clone(),
        kind,
        reason: precedence_reason(winner, loser),
    }
}

pub(crate) fn precedence_reason(
    winner: &ResponseRule,
    loser: &ResponseRule,
) -> RulePrecedenceReason {
    if winner.priority != loser.priority {
        RulePrecedenceReason::Priority
    } else if path_is_exact(winner) != path_is_exact(loser) {
        RulePrecedenceReason::ExactPath
    } else if winner.method.is_some() != loser.method.is_some() {
        RulePrecedenceReason::MethodSpecific
    } else if wildcard_prefix_len(winner) != wildcard_prefix_len(loser) {
        RulePrecedenceReason::LongerWildcardPrefix
    } else {
        RulePrecedenceReason::IdTieBreak
    }
}

fn path_pattern(path: &str) -> (&str, bool) {
    path.strip_suffix('*')
        .map_or((path, false), |prefix| (prefix, true))
}

fn path_is_exact(rule: &ResponseRule) -> bool {
    !rule.path.ends_with('*')
}

fn wildcard_prefix_len(rule: &ResponseRule) -> usize {
    rule.path.strip_suffix('*').map_or(0, str::len)
}
