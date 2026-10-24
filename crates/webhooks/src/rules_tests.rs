use crate::matcher::precedence_reason;
use std::cmp::Ordering;

use super::*;

fn rule(id: &str, method: Option<&str>, path: &str) -> ResponseRule {
    ResponseRule {
        id: id.into(),
        priority: 0,
        method: method.map(|s| s.into()),
        path: path.into(),
        status: 200,
        headers: vec![],
        body: String::new(),
        delay_ms: 0,
        sequence: vec![],
    }
}

#[test]
fn matches_exact_and_wildcard() {
    assert!(matches(&rule("r1", Some("POST"), "/hook"), "POST", "/hook"));
    assert!(!matches(&rule("r1", Some("GET"), "/hook"), "POST", "/hook"));
    assert!(matches(&rule("r2", None, "/any"), "DELETE", "/any"));
    assert!(matches(
        &rule("r3", None, "/events/*"),
        "POST",
        "/events/123"
    ));
    assert!(!matches(&rule("r3", None, "/events/*"), "POST", "/other"));
}

#[test]
fn method_matching_is_case_insensitive_and_none_matches_all_methods() {
    assert!(matches(&rule("all", None, "/hook"), "PATCH", "/hook"));
    assert!(matches(
        &rule("post", Some("post"), "/hook"),
        "PoSt",
        "/hook"
    ));
    assert!(!matches(
        &rule("post", Some("post"), "/hook"),
        "PUT",
        "/hook"
    ));
}

#[test]
fn path_matching_is_exact_or_prefix_only_for_a_trailing_star() {
    let exact = rule("exact", None, "/events/123");
    assert!(matches(&exact, "GET", "/events/123"));
    assert!(!matches(&exact, "GET", "/events/123/extra"));
    assert!(!matches(&exact, "GET", "/events/123?source=test"));

    let prefix = rule("prefix", None, "/events/*");
    assert!(matches(&prefix, "GET", "/events/"));
    assert!(matches(&prefix, "GET", "/events/123/extra"));
    assert!(!matches(&prefix, "GET", "/eventslater"));

    // An asterisk anywhere other than the final character is literal.
    let interior_star = rule("interior", None, "/events/*/tail");
    assert!(matches(&interior_star, "GET", "/events/*/tail"));
    assert!(!matches(&interior_star, "GET", "/events/123/tail"));
}

#[test]
fn upsert_assigns_and_preserves_rule_identity() {
    let mut rules = HashMap::new();
    let generated = upsert(&mut rules, rule("", Some("POST"), "/hook")).unwrap();
    assert!(!generated.is_empty());
    assert_eq!(rules[&generated].id, generated);

    let same = upsert(&mut rules, rule(&generated, Some("GET"), "/updated")).unwrap();
    assert_eq!(same, generated);
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[&generated].path, "/updated");
}

#[test]
fn validates_status_and_delay_bounds() {
    assert!(validate_rule(&rule("r", None, "/hook")).is_ok());

    let mut invalid = rule("r", None, "/hook");
    invalid.status = MIN_RESPONSE_STATUS - 1;
    assert!(validate_rule(&invalid).is_err());
    invalid.status = MAX_RESPONSE_STATUS + 1;
    assert!(validate_rule(&invalid).is_err());
    invalid.status = MIN_RESPONSE_STATUS;
    invalid.delay_ms = MAX_RESPONSE_DELAY_MS + 1;
    assert!(validate_rule(&invalid).is_err());

    invalid.status = MAX_RESPONSE_STATUS;
    invalid.delay_ms = MAX_RESPONSE_DELAY_MS;
    assert!(validate_rule(&invalid).is_ok());
}

#[test]
fn priority_is_bounded_and_old_json_defaults_to_zero() {
    let old: ResponseRule = serde_json::from_value(serde_json::json!({
        "id": "old",
        "method": null,
        "path": "/hook",
        "status": 200,
        "headers": [],
        "body": "",
        "delayMs": 0
    }))
    .unwrap();
    assert_eq!(old.priority, 0);

    let mut candidate = rule("priority", None, "/hook");
    candidate.priority = MIN_RULE_PRIORITY;
    assert!(validate_rule(&candidate).is_ok());
    candidate.priority = MAX_RULE_PRIORITY;
    assert!(validate_rule(&candidate).is_ok());
    candidate.priority = MAX_RULE_PRIORITY + 1;
    assert!(validate_rule(&candidate).is_err());
    candidate.priority = MIN_RULE_PRIORITY - 1;
    assert!(validate_rule(&candidate).is_err());
}

#[test]
fn selector_is_insertion_order_independent_and_uses_documented_precedence() {
    let mut any_prefix = rule("z-any-prefix", None, "/events/*");
    any_prefix.priority = 5;
    let mut method_prefix = rule("y-method-prefix", Some("POST"), "/events/special/*");
    method_prefix.priority = 5;
    let mut exact = rule("x-exact", None, "/events/special/42");
    exact.priority = 5;
    let mut priority = rule("priority", None, "/events/*");
    priority.priority = 6;
    let rules = [any_prefix, method_prefix, exact, priority];

    for order in [vec![0, 1, 2, 3], vec![3, 2, 1, 0], vec![1, 3, 0, 2]] {
        let selected = select_matching_rule(
            order.iter().map(|index| &rules[*index]),
            "POST",
            "/events/special/42",
        )
        .unwrap();
        assert_eq!(selected.id, "priority");
    }

    let tied = &rules[..3];
    assert_eq!(
        select_matching_rule(tied.iter(), "POST", "/events/special/42")
            .unwrap()
            .id,
        "x-exact"
    );
    assert_eq!(
        select_matching_rule(tied.iter(), "POST", "/events/special/other")
            .unwrap()
            .id,
        "y-method-prefix"
    );
}

#[test]
fn final_tie_break_is_bytewise_ascending_id() {
    let left = rule("alpha", Some("POST"), "/hook");
    let right = rule("beta", Some("POST"), "/hook");
    assert_eq!(
        select_matching_rule([&right, &left], "POST", "/hook")
            .unwrap()
            .id,
        "alpha"
    );
    assert_eq!(
        precedence_reason(&left, &right),
        RulePrecedenceReason::IdTieBreak
    );
}

#[test]
fn conflict_preview_reports_full_and_partial_overlap_without_mutation() {
    let existing = rule("existing", None, "/events/*");
    let mut rules = HashMap::from([(existing.id.clone(), existing)]);
    let before = rules.clone();

    let mut exact = rule("candidate", Some("POST"), "/events/42");
    exact.priority = 1;
    let plan = plan_upsert(&rules, exact).unwrap();
    assert!(plan.preview.requires_confirmation);
    assert_eq!(plan.preview.conflicts.len(), 1);
    assert_eq!(
        plan.preview.conflicts[0].kind,
        RuleConflictKind::PartialOverlap
    );
    assert_eq!(
        plan.preview.conflicts[0].reason,
        RulePrecedenceReason::Priority
    );
    assert_eq!(rules, before);

    let mut covering = rule("covering", None, "/events/*");
    covering.priority = 2;
    let plan = plan_upsert(&rules, covering).unwrap();
    assert_eq!(
        plan.preview.conflicts[0].kind,
        RuleConflictKind::CandidateShadowsExisting
    );

    let unrelated = rule("other", Some("GET"), "/unrelated");
    let plan = plan_upsert(&rules, unrelated).unwrap();
    assert!(!plan.preview.requires_confirmation);
    assert!(plan.preview.conflicts.is_empty());

    // Keep the map mutable in this test so Clippy/rustc also exercise the
    // projected collection without relying on an immutable-only fixture.
    rules.clear();
    assert!(rules.is_empty());
}

#[test]
fn overlap_matrix_handles_methods_exact_paths_and_nested_prefixes() {
    let get = rule("get", Some("GET"), "/events/42");
    let post = rule("post", Some("POST"), "/events/42");
    let any = rule("any", None, "/events/*");
    let nested = rule("nested", Some("GET"), "/events/private/*");
    let elsewhere = rule("elsewhere", None, "/other/*");
    assert!(!rules_overlap(&get, &post));
    assert!(rules_overlap(&get, &any));
    assert!(rules_overlap(&any, &nested));
    assert!(!rules_overlap(&get, &nested));
    assert!(!rules_overlap(&any, &elsewhere));
}

#[test]
fn rejects_transport_response_headers_that_would_override_wire_framing() {
    for name in [
        "Connection",
        "Content-Length",
        "Transfer-Encoding",
        "Upgrade",
        "Host",
    ] {
        let mut candidate = rule("transport", None, "/hook");
        candidate.headers = vec![(name.into(), "1".into())];
        assert!(
            validate_rule(&candidate).is_err(),
            "{name} should be reserved"
        );
    }
}

#[test]
fn validates_method_path_and_body_shape_and_size() {
    let mut invalid = rule("r", Some("POST"), "/hook");
    invalid.method = Some("post-json".into());
    assert!(validate_rule(&invalid).is_ok());
    // Custom HTTP methods use the RFC token grammar; matching remains
    // case-insensitive and does not invent a first-letter restriction.
    invalid.method = Some("!custom".into());
    assert!(validate_rule(&invalid).is_ok());
    invalid.method = Some(String::new());
    assert!(validate_rule(&invalid).is_err());
    invalid.method = Some("POST JSON".into());
    assert!(validate_rule(&invalid).is_err());
    invalid.method = Some("P".repeat(MAX_METHOD_CHARS + 1));
    assert!(validate_rule(&invalid).is_err());

    invalid = rule("r", None, "hook");
    assert!(validate_rule(&invalid).is_err());
    invalid.path = format!("/{}", "p".repeat(MAX_PATH_CHARS));
    assert!(validate_rule(&invalid).is_err());
    invalid.path = "/hook\u{0085}".into();
    assert!(validate_rule(&invalid).is_err());
    invalid.path = "/hook/한글".into();
    assert!(validate_rule(&invalid).is_err());

    invalid = rule("r", None, "/hook");
    invalid.body = "b".repeat(MAX_BODY_CHARS + 1);
    assert!(validate_rule(&invalid).is_err());
    invalid.body = "🙂".repeat(MAX_BODY_BYTES / 4 + 1);
    assert!(validate_rule(&invalid).is_err());
}

#[test]
fn validates_rule_id_character_and_string_bounds() {
    let mut invalid = rule("r", None, "/hook");
    invalid.id = "i".repeat(MAX_RULE_ID_CHARS);
    assert!(validate_rule(&invalid).is_ok());
    invalid.id = "i".repeat(MAX_RULE_ID_CHARS + 1);
    assert!(validate_rule(&invalid).is_err());
    invalid.id = "stable\u{0000}".into();
    assert!(validate_rule(&invalid).is_err());

    invalid.id.clear();
    assert!(validate_rule(&invalid).is_ok());
}

#[test]
fn validates_header_count_shape_and_aggregate_limits() {
    let mut invalid = rule("r", None, "/hook");
    invalid.headers = (0..=MAX_RULE_HEADERS)
        .map(|index| (format!("X-Test-{index}"), "ok".into()))
        .collect();
    assert!(validate_rule(&invalid).is_err());

    invalid.headers = vec![("not a header".into(), "ok".into())];
    assert!(validate_rule(&invalid).is_err());
    invalid.headers = vec![("X-Test".into(), "bad\nvalue".into())];
    assert!(validate_rule(&invalid).is_err());
    invalid.headers = vec![("X-Test".into(), "한글".into())];
    assert!(validate_rule(&invalid).is_err());

    invalid.headers = vec![("X-Test".into(), "v".repeat(MAX_HEADER_VALUE_CHARS + 1))];
    assert!(validate_rule(&invalid).is_err());

    invalid.headers = vec![("N".repeat(MAX_HEADER_NAME_CHARS), "ok".into())];
    assert!(validate_rule(&invalid).is_ok());
    invalid.headers = vec![("N".repeat(MAX_HEADER_NAME_CHARS + 1), "ok".into())];
    assert!(validate_rule(&invalid).is_err());

    invalid.headers = (0..5)
        .map(|index| (format!("X-{index}"), "v".repeat(MAX_HEADER_TOTAL_CHARS / 4)))
        .collect();
    assert!(validate_rule(&invalid).is_err());
}

#[test]
fn rejects_invalid_upsert_without_mutating_storage() {
    let mut rules = HashMap::new();
    let id = upsert(&mut rules, rule("stable", Some("GET"), "/old")).unwrap();
    let before = rules.clone();

    let mut invalid = rule(&id, Some("GET"), "/new");
    invalid.status = 600;
    assert!(upsert(&mut rules, invalid).is_err());
    assert_eq!(rules, before);
}

#[test]
fn enforces_rule_count_and_collection_string_limits() {
    let mut rules = HashMap::new();
    for index in 0..MAX_RULES {
        assert!(upsert(&mut rules, rule(&format!("r-{index}"), None, "/hook")).is_ok());
    }
    let before = rules.clone();
    assert!(upsert(&mut rules, rule("new", None, "/hook")).is_err());
    assert_eq!(rules, before);

    let mut large_rules = HashMap::new();
    for index in 0..(MAX_RULE_COLLECTION_CHARS / MAX_BODY_CHARS + 1) {
        let mut candidate = rule(&format!("large-{index}"), None, "/hook");
        candidate.body = "x".repeat(MAX_BODY_CHARS);
        if index == MAX_RULE_COLLECTION_CHARS / MAX_BODY_CHARS {
            assert!(upsert(&mut large_rules, candidate).is_err());
        } else {
            assert!(upsert(&mut large_rules, candidate).is_ok());
        }
    }
}

#[test]
fn validates_complete_rule_collection() {
    let valid = rule("r", None, "/hook");
    assert!(validate_rule_collection([&valid]).is_ok());

    let invalid = rule("bad", None, "hook");
    assert!(validate_rule_collection([&valid, &invalid]).is_err());

    let too_many: Vec<ResponseRule> = (0..=MAX_RULES)
        .map(|index| rule(&format!("r-{index}"), None, "/hook"))
        .collect();
    assert!(validate_rule_collection(too_many.iter()).is_err());
}

#[test]
fn response_sequence_consumes_in_order_and_holds_the_final_step() {
    let mut candidate = rule("sequence", Some("POST"), "/hook");
    candidate.status = 202;
    candidate.body = "first".into();
    candidate.sequence = vec![
        ResponseSequenceStep {
            status: 500,
            headers: vec![],
            body: "retry".into(),
            delay_ms: 10,
        },
        ResponseSequenceStep {
            status: 204,
            headers: vec![("X-Ready".into(), "yes".into())],
            body: String::new(),
            delay_ms: 0,
        },
    ];

    assert_eq!(candidate.response_at(0).status, 202);
    assert_eq!(candidate.response_at(1).body, "retry");
    assert_eq!(candidate.response_at(2).status, 204);
    assert_eq!(candidate.response_at(99).status, 204);
    assert!(validate_rule(&candidate).is_ok());
}

#[test]
fn response_sequence_is_bounded_and_validated_like_the_base_response() {
    let mut candidate = rule("sequence", None, "/hook");
    candidate.sequence = vec![
        ResponseSequenceStep {
            status: 200,
            headers: vec![],
            body: String::new(),
            delay_ms: 0,
        };
        MAX_RESPONSE_SEQUENCE + 1
    ];
    assert!(validate_rule(&candidate).is_err());

    candidate.sequence = vec![ResponseSequenceStep {
        status: 600,
        headers: vec![],
        body: String::new(),
        delay_ms: 0,
    }];
    assert!(validate_rule(&candidate).is_err());

    candidate.sequence = vec![ResponseSequenceStep {
        status: 200,
        headers: vec![("X-Bad".into(), "line\nfeed".into())],
        body: String::new(),
        delay_ms: 0,
    }];
    assert!(validate_rule(&candidate).is_err());
}

#[test]
fn response_sequence_state_reset_starts_at_the_base_response() {
    let mut candidate = rule("sequence", Some("POST"), "/hook");
    candidate.body = "first".into();
    candidate.sequence = vec![ResponseSequenceStep {
        status: 500,
        headers: vec![],
        body: "retry".into(),
        delay_ms: 0,
    }];
    let mut state = ResponseSequenceState::default();
    assert_eq!(state.next_response(&candidate).body, "first");
    assert_eq!(state.next_response(&candidate).body, "retry");
    assert_eq!(state.next_response(&candidate).body, "retry");
    state.reset(&candidate.id);
    assert_eq!(state.next_response(&candidate).body, "first");
}

#[test]
fn matcher_rejects_non_ascii_paths_even_for_unvalidated_rules() {
    let candidate = rule("unicode", None, "/hook/한글");
    assert!(!matches(&candidate, "GET", "/hook/한글"));
    assert!(!matches(&rule("ascii", None, "/hook"), "GET", "/hook/한글"));
}
