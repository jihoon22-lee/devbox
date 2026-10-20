use crate::{
    Problem, ProblemSeverity,
    problems::{ProblemParser, extract},
};
use std::path::Path;

fn assert_problem(
    problem: Problem,
    file: &str,
    line: u32,
    col: Option<u32>,
    severity: ProblemSeverity,
    message: &str,
) {
    assert_eq!(problem.file, file);
    assert_eq!(problem.line, line);
    assert_eq!(problem.col, col);
    assert_eq!(problem.severity, severity);
    assert_eq!(problem.message, message);
    assert!(problem.source.is_empty());
}

#[test]
fn problems_plan_assertions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    assert_eq!(
        extract("src/x.ts(10,5): error TS1005: expected", root)
            .unwrap()
            .line,
        10
    );
    assert!(extract("../outside.go:10:5: error", root).is_none());
}

#[test]
fn problems_single_line_tool_fixtures() {
    let temp = tempfile::tempdir().unwrap();
    for (text, file, line, col, severity, message) in [
        (
            "src/x.ts(10,5): error TS1005: ';' expected.",
            "src/x.ts",
            10,
            Some(5),
            ProblemSeverity::Error,
            "TS1005: ';' expected.",
        ),
        (
            "src/app.ts:4:9: warning Unexpected console statement. [Warning/no-console]",
            "src/app.ts",
            4,
            Some(9),
            ProblemSeverity::Warning,
            "Unexpected console statement. [Warning/no-console]",
        ),
        (
            "src/app.ts:4:9: error 'missing' is not defined. [Error/no-undef]",
            "src/app.ts",
            4,
            Some(9),
            ProblemSeverity::Error,
            "'missing' is not defined. [Error/no-undef]",
        ),
        (
            "src/app.ts:4:9 lint/style/useConst  FIXABLE  ━━━━━━━━━━",
            "src/app.ts",
            4,
            Some(9),
            ProblemSeverity::Error,
            "lint/style/useConst  FIXABLE  ━━━━━━━━━━",
        ),
        (
            "./main.go:10:5: undefined: missing",
            "main.go",
            10,
            Some(5),
            ProblemSeverity::Error,
            "undefined: missing",
        ),
        (
            "main.go:10: undefined: missing",
            "main.go",
            10,
            None,
            ProblemSeverity::Error,
            "undefined: missing",
        ),
        (
            "src/한 글.ts(2,3): error TS2304: Cannot find name 'x'.",
            "src/한 글.ts",
            2,
            Some(3),
            ProblemSeverity::Error,
            "TS2304: Cannot find name 'x'.",
        ),
        (
            "src/main.rs:20:8: warning: unused variable: `x`",
            "src/main.rs",
            20,
            Some(8),
            ProblemSeverity::Warning,
            "unused variable: `x`",
        ),
    ] {
        assert_problem(
            extract(text, temp.path()).unwrap_or_else(|| panic!("unparsed: {text}")),
            file,
            line,
            col,
            severity,
            message,
        );
    }
}

#[test]
fn problems_rust_cargo_header_retains_message_until_location() {
    let temp = tempfile::tempdir().unwrap();
    let mut parser = ProblemParser::default();
    assert!(
        parser
            .push(
                "error[E0425]: cannot find value `missing` in this scope",
                temp.path()
            )
            .is_none()
    );
    assert_problem(
        parser.push("  --> src/main.rs:10:5", temp.path()).unwrap(),
        "src/main.rs",
        10,
        Some(5),
        ProblemSeverity::Error,
        "E0425: cannot find value `missing` in this scope",
    );
    assert!(
        parser
            .push("warning: unused variable: `x`", temp.path())
            .is_none()
    );
    assert_problem(
        parser.push(" --> src/main.rs:12:9", temp.path()).unwrap(),
        "src/main.rs",
        12,
        Some(9),
        ProblemSeverity::Warning,
        "unused variable: `x`",
    );
}

#[test]
fn problems_python_traceback_uses_last_frame_and_exception() {
    let temp = tempfile::tempdir().unwrap();
    let mut parser = ProblemParser::default();
    for text in [
        "Traceback (most recent call last):",
        "  File \"app.py\", line 12, in <module>",
        "    main()",
        "  File \"src/한 글.py\", line 7, in main",
        "    raise ValueError('bad value')",
    ] {
        assert!(parser.push(text, temp.path()).is_none());
    }
    assert_problem(
        parser.push("ValueError: bad value", temp.path()).unwrap(),
        "src/한 글.py",
        7,
        None,
        ProblemSeverity::Error,
        "ValueError: bad value",
    );
    assert!(parser.push("ValueError: another", temp.path()).is_none());
}

#[test]
fn problems_vitest_and_jest_stack_locations_keep_failure_message() {
    let temp = tempfile::tempdir().unwrap();
    for (header, location, message) in [
        (
            "AssertionError: expected 1 to be 2",
            " ❯ src/math.test.ts:8:17",
            "AssertionError: expected 1 to be 2",
        ),
        (
            "Error: expect(received).toBe(expected)",
            "    at Object.<anonymous> (src/math.test.ts:8:17)",
            "Error: expect(received).toBe(expected)",
        ),
    ] {
        let mut parser = ProblemParser::default();
        assert!(parser.push(header, temp.path()).is_none());
        assert_problem(
            parser.push(location, temp.path()).unwrap(),
            "src/math.test.ts",
            8,
            Some(17),
            ProblemSeverity::Error,
            message,
        );
    }
}

#[test]
fn problems_ansi_sequences_do_not_enter_fields() {
    let temp = tempfile::tempdir().unwrap();
    let text = "\u{1b}[31msrc/x.ts\u{1b}[0m(10,5): \u{1b}[31merror\u{1b}[0m TS1005: expected";
    assert_problem(
        extract(text, temp.path()).unwrap(),
        "src/x.ts",
        10,
        Some(5),
        ProblemSeverity::Error,
        "TS1005: expected",
    );
    let linked = "\u{1b}]8;;file:///project/src/x.ts\u{1b}\\src/x.ts\u{1b}]8;;\u{1b}\\(10,5): error TS1005: expected";
    assert_eq!(extract(linked, temp.path()).unwrap().file, "src/x.ts");
}

#[test]
fn problems_absolute_inside_root_is_relative_and_outside_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let text = format!("{}:4:2: broken", temp.path().join("src/main.go").display());
    assert_eq!(extract(&text, temp.path()).unwrap().file, "src/main.go");
    let text = format!("{}:4:2: broken", outside.path().join("main.go").display());
    assert!(extract(&text, temp.path()).is_none());
    assert!(extract("src/../../outside.go:4:2: broken", temp.path()).is_none());
    assert!(extract("C:\\outside\\main.go:4:2: broken", temp.path()).is_none());
}

#[test]
fn problems_normalizes_existing_and_missing_files_within_root() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    std::fs::write(temp.path().join("src/main.go"), "package main").unwrap();
    for text in [
        "./src/main.go:2:1: broken",
        "src/../src/main.go:2:1: broken",
    ] {
        assert_eq!(extract(text, temp.path()).unwrap().file, "src/main.go");
    }
    assert_eq!(
        extract("src/new.go:2:1: broken", temp.path()).unwrap().file,
        "src/new.go"
    );
}

#[cfg(unix)]
#[test]
fn problems_symlink_escape_including_missing_leaf_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("existing.go"), "package main").unwrap();
    std::os::unix::fs::symlink(outside.path(), temp.path().join("linked")).unwrap();
    assert!(extract("linked/existing.go:1:2: broken", temp.path()).is_none());
    assert!(extract("linked/missing.go:1:2: broken", temp.path()).is_none());
    assert!(extract("linked/../outside.go:1:2: broken", temp.path()).is_none());
}

#[test]
fn problems_invalid_and_unbounded_fields_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    for text in [
        "normal output",
        "src/x.ts(0,2): error bad",
        "x.go:4294967296:2: bad",
        "x.go:1:0: bad",
        "x.go:1:4294967296: bad",
        "https://example.com/x.go:1:2: bad",
        "src/\u{0}x.go:1:2: bad",
    ] {
        assert!(extract(text, temp.path()).is_none(), "accepted: {text:?}");
    }
    assert!(extract(&format!("{}.go:1:2: bad", "x".repeat(4097)), temp.path()).is_none());
    assert!(extract(&format!("x.go:1:2: {}", "x".repeat(4097)), temp.path()).is_none());
    assert!(
        extract(
            "x.go:1:2: bad",
            Path::new("/nonexistent-devbox-problems-fixture-root")
        )
        .is_none()
    );
}

#[test]
fn problems_parser_context_is_bounded_and_cleared_by_new_diagnostics() {
    let temp = tempfile::tempdir().unwrap();
    let mut parser = ProblemParser::default();
    assert!(parser.push("error: stale", temp.path()).is_none());
    for _ in 0..64 {
        assert!(parser.push("ordinary output", temp.path()).is_none());
    }
    let eventual = parser.push(" --> x.rs:1:2", temp.path());
    assert!(eventual.is_none_or(|problem| problem.message != "stale"));
    assert!(
        parser
            .push(&format!("error: {}", "x".repeat(4097)), temp.path())
            .is_none()
    );
    assert!(parser.push(" --> x.rs:1:2", temp.path()).is_none());
}

#[test]
fn problems_python_outside_last_frame_cannot_reuse_an_inside_frame() {
    let temp = tempfile::tempdir().unwrap();
    let mut parser = ProblemParser::default();
    assert!(
        parser
            .push("Traceback (most recent call last):", temp.path())
            .is_none()
    );
    assert!(
        parser
            .push("  File \"app.py\", line 12, in main", temp.path())
            .is_none()
    );
    assert!(
        parser
            .push("  File \"../outside.py\", line 7, in external", temp.path())
            .is_none()
    );
    assert!(parser.push("ValueError: bad value", temp.path()).is_none());
}

#[test]
fn problems_source_context_preserves_project_boundary_and_never_guesses_old_cwd() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let cwd = project.join("nested");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(project.join("shared.rs"), "owned").unwrap();
    std::fs::write(temp.path().join("outside.rs"), "owned outside").unwrap();
    let mut run = crate::ProblemRun {
        run_id: "source".into(),
        project_id: "project".into(),
        root: project.clone(),
        source_cwd: Some(cwd.clone()),
        windows_directory: Some(r"C:\owned\nested".into()),
        path: temp.path().join("fixture.log"),
        attempt_id: Some("a".into()),
        started_ms: 1,
    };
    let mut parser = ProblemParser::default();
    let problem = parser
        .push_source("../shared.rs:10:2: error: shared", &run)
        .unwrap();
    assert_eq!(
        problem.file,
        project.join("shared.rs").display().to_string()
    );
    for diagnostic in [
        "../../outside.rs:1:2: error: escape",
        r"C:\owned\nested-other\x.rs:1:2: error: copy escape",
        "https://bad/x.rs:1:2: error: url",
    ] {
        assert!(
            parser.push_source(diagnostic, &run).is_none(),
            "accepted {diagnostic}"
        );
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(temp.path(), cwd.join("outside")).unwrap();
        assert!(
            parser
                .push_source("outside/outside.rs:1:2: error: symlink", &run)
                .is_none()
        );
    }
    run.source_cwd = None;
    assert!(
        parser
            .push_source("shared.rs:1:2: error: unknown cwd", &run)
            .is_none()
    );
    let absolute = format!(
        "{}:1:2: error: old absolute",
        project.join("shared.rs").display()
    );
    assert_eq!(
        parser.push_source(&absolute, &run).unwrap().file,
        project.join("shared.rs").display().to_string()
    );
}
