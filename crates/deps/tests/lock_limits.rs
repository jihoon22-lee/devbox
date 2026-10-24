use devbox_deps::{
    locks::{MAX_BYTES, MAX_ITEMS, parse},
    model::*,
};
use std::collections::BTreeMap;
fn ms() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "package.json".into(),
            "{\"dependencies\":{\"local\":\"file:../local\"}}".into(),
        ),
        (
            "Cargo.toml".into(),
            "[package]\nname='owned'\nversion='0.1.0'".into(),
        ),
        (
            "go.mod".into(),
            "module owned\nrequire missing.org/dependency v1.0.0\n".into(),
        ),
        (
            "pyproject.toml".into(),
            "[project]\nname='owned'\ndependencies=['some.package>=1']\n".into(),
        ),
    ])
}
#[test]
fn pnpm_local_reference_is_visible_and_not_registry() {
    let ds=parse(LockKind::Pnpm,"lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      local: {version: 'link:../local', specifier: 'link:../local'}\n",&ms()).expect("parse owned dependency fixture");
    assert_eq!(ds.len(), 1);
    assert_eq!(ds[0].versions[0].source, DependencySource::Local);
    assert!(ds[0].direct);
}
#[test]
fn absent_relationship_data_is_not_clean_empty_inventory() {
    for (kind, text) in [
        (LockKind::Pnpm, "lockfileVersion: '9.0'"),
        (LockKind::Cargo, "version=4\npackage=[]"),
        (LockKind::Go, ""),
    ] {
        assert!(parse(kind, text, &ms()).is_err(), "{kind:?}");
    }
}
#[test]
fn npm_local_link_is_visible_and_not_registry() {
    let ds=parse(LockKind::Npm,r#"{"lockfileVersion":3,"packages":{"":{"dependencies":{"local":"file:../local"}},"node_modules/local":{"resolved":"../local","link":true}}}"#,&ms()).expect("parse owned dependency fixture");
    assert_eq!(ds.len(), 1);
    assert_eq!(ds[0].versions[0].source, DependencySource::Local);
}
#[test]
fn pnpm_multiple_documents_retain_importer_relationships() {
    let text = "lockfileVersion: '9.0'\n---\nimporters:\n  .:\n    dependencies:\n      x: {version: 1.0.0, specifier: ^1}\npackages:\n  x@1.0.0: {resolution: {integrity: fixture}}\n";
    assert!(parse(LockKind::Pnpm, text, &ms()).expect("parse owned dependency fixture")[0].direct);
}
#[test]
fn yaml_alias_depth_duplicate_include_and_item_budgets_reject() {
    let inputs = [
        "lockfileVersion: '9.0'\nimporters: &a {}\npackages: *a".to_owned(),
        "lockfileVersion: '9.0'\nlockfileVersion: '9.0'\nimporters: {}".into(),
        "lockfileVersion: '9.0'\nimporters: !include /etc/passwd".into(),
        format!(
            "lockfileVersion: '9.0'\nextra: {}0{}",
            "[".repeat(40),
            "]".repeat(40)
        ),
        format!(
            "lockfileVersion: '9.0'\nimporters:\n  .: {{}}\npackages:\n{}",
            (0..=MAX_ITEMS)
                .map(|i| format!("  x{i}@1.0.0: {{resolution: {{integrity: test}}}}\n"))
                .collect::<String>()
        ),
    ];
    for text in inputs {
        assert!(parse(LockKind::Pnpm, &text, &ms()).is_err());
    }
}
#[test]
fn all_parsers_reject_oversized_inputs() {
    for kind in [
        LockKind::Pnpm,
        LockKind::Npm,
        LockKind::Yarn,
        LockKind::Cargo,
        LockKind::Go,
        LockKind::Poetry,
        LockKind::Uv,
    ] {
        assert!(matches!(
            parse(kind, &"x".repeat(MAX_BYTES + 1), &ms()),
            Err(DepsError::LimitExceeded)
        ));
    }
}
#[test]
fn unsupported_schema_versions_are_explicit() {
    for (kind, text) in [
        (LockKind::Npm, "{\"lockfileVersion\":1}"),
        (LockKind::Cargo, "version=99"),
        (LockKind::Yarn, "__metadata:\n  version: 8\n"),
        (
            LockKind::Poetry,
            "package=[]\n[metadata]\nlock-version='99'",
        ),
        (LockKind::Uv, "version=99\npackage=[]"),
    ] {
        assert!(
            matches!(
                parse(kind, text, &ms()),
                Err(DepsError::UnsupportedLock { .. })
            ),
            "{kind:?}"
        );
    }
}
#[test]
fn yarn_unicode_input_never_panics() {
    let result = std::panic::catch_unwind(|| {
        parse(
            LockKind::Yarn,
            "# yarn lockfile v1\n한글@^1:\n  version \"1.0.0\"\n  resolved \"file:../owned\"\n",
            &ms(),
        )
    });
    assert!(result.is_ok());
}
#[test]
fn uv_root_optional_and_dev_relationships_are_direct() {
    let mut manifests = ms();
    manifests.insert("pyproject.toml".into(), "[project]\nname='owned'".into());
    let text = "version=1\nrevision=3\n[[package]]\nname='owned'\nversion='0.1.0'\nsource={virtual='.'}\n[package.optional-dependencies]\ntest=[{name='some-package'}]\n[package.dev-dependencies]\ndev=[{name='other'}]\n[[package]]\nname='some-package'\nversion='1.0.0'\nsource={registry='https://pypi.org/simple'}\n[[package]]\nname='other'\nversion='2.0.0'\nsource={registry='https://pypi.org/simple'}";
    let ds = parse(LockKind::Uv, text, &manifests).expect("parse owned dependency fixture");
    assert!(ds.iter().all(|d| d.direct));
}
#[test]
fn missing_uv_root_and_yarn_workspace_relationships_are_explicit_errors() {
    let mut manifests = ms();
    let uv = "version=1\n[[package]]\nname='some-package'\nversion='1.0.0'\nsource={registry='https://pypi.org/simple'}";
    assert!(parse(LockKind::Uv, uv, &manifests).is_err());
    manifests.insert(
        "package.json".into(),
        r#"{"workspaces":["packages/*"]}"#.into(),
    );
    assert!(matches!(
        parse(
            LockKind::Yarn,
            include_str!("fixtures/yarn.lock"),
            &manifests
        ),
        Err(DepsError::UnsupportedLock { .. })
    ));
}
#[test]
fn unresolved_cargo_root_dependency_is_not_a_clean_inventory() {
    let text =
        "version=4\n[[package]]\nname='owned'\nversion='0.1.0'\ndependencies=['missing 1.0.0']\n";
    assert!(parse(LockKind::Cargo, text, &ms()).is_err());
}
#[test]
fn missing_go_indirect_requirement_is_not_clean() {
    let mut manifests = ms();
    manifests.insert(
        "go.mod".into(),
        "module owned\nrequire missing.org/dependency v1.0.0 // indirect\n".into(),
    );
    assert!(parse(LockKind::Go, "", &manifests).is_err());
}
#[test]
fn missing_yarn_manifest_reference_is_not_clean() {
    assert!(parse(LockKind::Yarn, "# yarn lockfile v1\n", &ms()).is_err());
}
#[test]
fn missing_poetry_manifest_reference_is_not_clean() {
    assert!(
        parse(
            LockKind::Poetry,
            "package=[]\n[metadata]\nlock-version='2.1'",
            &ms()
        )
        .is_err()
    );
}
#[test]
fn pnpm_boot_document_package_manager_is_direct_and_project_graph_remains_present() {
    let text = "lockfileVersion: '9.0'\nimporters:\n  .:\n    packageManagerDependencies:\n      pnpm: {specifier: 12.3.4, version: 12.3.4}\npackages:\n  pnpm@12.3.4: {resolution: {integrity: fixture}}\n---\nlockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      x: {version: 1.0.0, specifier: ^1}\npackages:\n  x@1.0.0: {resolution: {integrity: fixture}}\n";
    let ds = parse(LockKind::Pnpm, text, &ms()).expect("parse owned dependency fixture");
    assert_eq!(ds.len(), 2);
    assert!(ds.iter().all(|d| d.direct));
}
#[test]
fn pnpm_positive_registry_revision_is_explicitly_unsupported() {
    let text = "lockfileVersion: '9.0'\nimporters:\n  .: {}\npackages:\n  x@1.0.0: {resolution: {integrity: fixture, revision: 1}}\n";
    assert!(matches!(
        parse(LockKind::Pnpm, text, &ms()),
        Err(DepsError::UnsupportedLock { .. })
    ));
}
