use devbox_deps::{
    locks::{Manifests, parse},
    model::*,
};
const ROOT: &str = "version=4\n[[package]]\nname='owned'\nversion='0.1.0'\n";
fn cargo(manifest: &str, lock: &str) -> Result<Vec<Dependency>, DepsError> {
    parse(
        LockKind::Cargo,
        lock,
        &Manifests::from([(
            "Cargo.toml".into(),
            format!("[package]\nname='owned'\nversion='0.1.0'\n{manifest}"),
        )]),
    )
}
fn matching() -> String {
    format!(
        "{ROOT}dependencies=['serde 1.0.0']\n[[package]]\nname='serde'\nversion='1.0.0'\nsource='registry+https://github.com/rust-lang/crates.io-index'\n"
    )
}
#[test]
fn cargo_ordinary_declaration_missing_from_lock_refuses() {
    assert!(cargo("[dependencies]\nserde='1'", ROOT).is_err());
}
#[test]
fn cargo_matching_and_dependency_free_controls() {
    assert!(cargo("", ROOT).unwrap().is_empty());
    let ds = cargo("[dependencies]\nserde='1'", &matching()).unwrap();
    assert!(ds[0].direct);
}
#[test]
fn cargo_alias_must_match_its_actual_package_reference() {
    let manifest = "[dependencies]\nrenamed={package='serde',version='1'}";
    assert!(cargo(manifest, ROOT).is_err());
    assert!(cargo(manifest, &matching()).unwrap()[0].direct);
}
#[test]
fn cargo_dev_build_and_target_declarations_are_checked() {
    for table in [
        "dev-dependencies",
        "build-dependencies",
        "target.'cfg(unix)'.dependencies",
    ] {
        let manifest = format!("[{table}]\nserde='1'");
        assert!(cargo(&manifest, ROOT).is_err(), "{table}");
        assert!(cargo(&manifest, &matching()).unwrap()[0].direct);
    }
}
#[test]
fn cargo_explicit_workspace_member_inherits_package_alias() {
    let mut ms=Manifests::from([("Cargo.toml".into(),"[workspace]\nmembers=['app']\n[workspace.dependencies]\nrenamed={package='serde',version='1'}".into()),("app/Cargo.toml".into(),"[package]\nname='owned'\nversion='0.1.0'\n[dependencies]\nrenamed={workspace=true}".into())]);
    assert!(parse(LockKind::Cargo, ROOT, &ms).is_err());
    assert!(parse(LockKind::Cargo, &matching(), &ms).unwrap()[0].direct);
    ms.insert("Cargo.toml".into(), "[workspace]\nmembers=['app']".into());
    assert!(parse(LockKind::Cargo, &matching(), &ms).is_err());
}
fn poetry(section: &str, present: bool) -> Result<Vec<Dependency>, DepsError> {
    let ms = Manifests::from([(
        "pyproject.toml".into(),
        format!("[tool.poetry]\nname='owned'\nversion='0.1.0'\n[{section}]\nPy_Test='^7'"),
    )]);
    let packages = if present {
        "[[package]]\nname='py-test'\nversion='7.0.0'\ncategory='dev'"
    } else {
        "package=[]"
    };
    parse(
        LockKind::Poetry,
        &format!("{packages}\n[metadata]\nlock-version='1.1'"),
        &ms,
    )
}
#[test]
fn poetry_legacy_dev_present_is_direct() {
    assert!(poetry("tool.poetry.dev-dependencies", true).unwrap()[0].direct);
}
#[test]
fn poetry_missing_legacy_dev_refuses() {
    assert!(poetry("tool.poetry.dev-dependencies", false).is_err());
}
#[test]
fn poetry_modern_groups_remain_direct_and_require_lock_entry() {
    assert!(poetry("tool.poetry.group.dev.dependencies", true).unwrap()[0].direct);
    assert!(poetry("tool.poetry.group.dev.dependencies", false).is_err());
}
