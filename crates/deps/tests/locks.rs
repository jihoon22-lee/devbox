use devbox_deps::{locks::parse, model::*};
use std::collections::BTreeMap;
fn manifests() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("package.json".into(), r#"{"dependencies":{"@scope/pkg":"^1"}}"#.into()),
        ("Cargo.toml".into(), "[package]\nname='owned'\nversion='0.1.0'\n[dependencies]\ncrate-a='1.2'\ngit-crate={git='https://example.invalid/owned'}\n".into()),
        ("go.mod".into(), "module example.org/owned\ngo 1.24\nrequire (\nexample.org/module v1.2.3\nexample.org/transitive v2.0.0 // indirect\n)\n".into()),
        ("pyproject.toml".into(), "[project]\nname='owned'\nversion='0.1.0'\ndependencies=['some.package>=1']\n".into()),
    ])
}
#[test]
fn pnpm_importers_scopes_peers_and_versions() {
    let ds = parse(
        LockKind::Pnpm,
        include_str!("fixtures/pnpm.yaml"),
        &manifests(),
    )
    .expect("parse owned dependency fixture");
    let d = ds
        .iter()
        .find(|d| d.name == "@scope/pkg")
        .expect("matching owned dependency");
    assert!(d.direct);
    assert_eq!(d.versions.len(), 2);
    assert!(d.versions.iter().any(|v| v.version == "1.2.3" && v.direct));
    assert!(
        ds.iter()
            .find(|d| d.name == "other")
            .expect("matching owned dependency")
            .direct
    );
}
#[test]
fn npm_nested_instances_and_yarn_manifest_directness() {
    for (kind, text) in [
        (LockKind::Npm, include_str!("fixtures/npm.json")),
        (LockKind::Yarn, include_str!("fixtures/yarn.lock")),
    ] {
        let ds = parse(kind, text, &manifests()).expect("parse owned dependency fixture");
        assert!(
            ds.iter()
                .find(|d| d.name == "@scope/pkg")
                .expect("matching owned dependency")
                .direct
        );
        if kind == LockKind::Npm {
            assert_eq!(ds[0].versions.len(), 2);
        }
    }
}
#[test]
fn cargo_keeps_versions_and_marks_git_unqueryable() {
    let ds = parse(
        LockKind::Cargo,
        include_str!("fixtures/Cargo.lock"),
        &manifests(),
    )
    .expect("parse owned dependency fixture");
    let d = ds
        .iter()
        .find(|d| d.name == "crate-a")
        .expect("matching owned dependency");
    assert_eq!(d.versions.len(), 2);
    assert!(d.versions.iter().any(|v| v.version == "1.0.0" && !v.direct));
    assert_eq!(
        ds.iter()
            .find(|d| d.name == "git-crate")
            .expect("matching owned dependency")
            .versions[0]
            .source,
        DependencySource::Git
    );
}
#[test]
fn go_deduplicates_module_checksums_and_uses_require_relationship() {
    let ds = parse(LockKind::Go, include_str!("fixtures/go.sum"), &manifests())
        .expect("parse owned dependency fixture");
    assert_eq!(ds.len(), 2);
    assert_eq!(ds[0].versions.len(), 1);
    assert!(ds[0].direct);
    assert!(!ds[1].direct);
}
#[test]
fn python_locks_normalize_names_without_guessing_directness() {
    for (kind, text) in [
        (LockKind::Poetry, include_str!("fixtures/poetry.lock")),
        (LockKind::Uv, include_str!("fixtures/uv.lock")),
    ] {
        let ds = parse(kind, text, &manifests()).expect("parse owned dependency fixture");
        let d = ds
            .iter()
            .find(|d| d.name == "some-package")
            .expect("matching owned dependency");
        assert!(d.direct);
        assert_eq!(d.versions[0].version, "1.2.3");
    }
}
#[test]
fn unsupported_and_malformed_are_not_empty_success() {
    assert!(matches!(
        parse(LockKind::Pnpm, "lockfileVersion: '999'", &manifests()),
        Err(DepsError::UnsupportedLock { .. })
    ));
    for kind in [
        LockKind::Pnpm,
        LockKind::Npm,
        LockKind::Yarn,
        LockKind::Cargo,
        LockKind::Go,
        LockKind::Poetry,
        LockKind::Uv,
    ] {
        assert!(parse(kind, "not a valid lock {{", &manifests()).is_err());
    }
}
