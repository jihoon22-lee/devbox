//! Commit-only, resumable publication of the four registered shortcut files.
use super::suite_removal::Plan;
use devbox_filesystem::{atomic_write, ensure_no_links, open_filesystem_object};
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, path::Path};
type Result<T> = std::result::Result<T, &'static str>;
const RECEIPT: &str = "suite-shortcuts-next.json";
const NAMES: [&str; 4] = [
    "Workspace.lnk",
    "API Studio.lnk",
    "Knowledge.lnk",
    "Control Center.lnk",
];
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pending {
    schema_version: u32,
    revision: String,
    original: Plan,
    staged: Plan,
    directory: String,
}
fn same(a: &Plan, b: &Plan) -> Result<bool> {
    Ok(
        serde_json::to_vec(a).map_err(|_| "suite_shortcut_unavailable")?
            == serde_json::to_vec(b).map_err(|_| "suite_shortcut_unavailable")?,
    )
}
fn load(root: &Path) -> Result<Option<Pending>> {
    let path = root.join(RECEIPT);
    if matches!(fs::symlink_metadata(&path),Err(e) if e.kind()==std::io::ErrorKind::NotFound) {
        return Ok(None);
    }
    ensure_no_links(&path).map_err(|_| "suite_shortcut_directory_unsafe")?;
    let (mut file, _) =
        open_filesystem_object(&path, false).map_err(|_| "suite_shortcut_unavailable")?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "suite_shortcut_unavailable")?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("suite_shortcut_conflict");
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "suite_shortcut_conflict")
}
fn validate(pending: &Pending, key: &str, revision: &str) -> Result<Plan> {
    let uuid = pending
        .directory
        .strip_prefix(".devbox-shortcuts-")
        .ok_or("suite_shortcut_conflict")?;
    if pending.schema_version != 1
        || pending.revision != revision
        || !product_contract::commands::revision(revision)
        || !uuid::Uuid::parse_str(uuid).is_ok_and(|value| value.to_string() == uuid)
        || pending.original.installation_key != key
        || pending.staged.installation_key != key
        || pending.original.root_identity != pending.staged.root_identity
        || pending.original.files.len() != 4
        || pending.staged.files.len() != 4
    {
        return Err("suite_shortcut_conflict");
    }
    pending.original.validate()?;
    pending.staged.validate()?;
    let mut adopted = pending.staged.clone();
    for name in NAMES {
        if pending
            .original
            .files
            .iter()
            .filter(|file| file.relative == name)
            .count()
            != 1
        {
            return Err("suite_shortcut_conflict");
        }
        let relative = format!("{}/{name}", pending.directory);
        let file = adopted
            .files
            .iter_mut()
            .find(|file| file.relative == relative)
            .ok_or("suite_shortcut_conflict")?;
        file.relative = name.into();
    }
    Ok(adopted)
}
fn one(plan: &Plan, name: &str) -> Result<Plan> {
    let mut result = plan.clone();
    result.files = vec![plan
        .files
        .iter()
        .find(|file| file.relative == name)
        .ok_or("suite_shortcut_conflict")?
        .clone()];
    Ok(result)
}
fn required(dir: &Path, plan: &Plan) -> Result<()> {
    plan.verify_remaining(dir)?;
    if plan
        .files
        .iter()
        .any(|file| !dir.join(&file.relative).is_file())
    {
        return Err("suite_shortcut_conflict");
    }
    Ok(())
}
/// Caller has verified the current payload/helper and native committed journal.
/// `prepare` writes only the four known links in the unique new directory.
pub fn adopt(
    root: &Path,
    dir: &Path,
    key: &str,
    revision: &str,
    registered: &Plan,
    prepare: impl FnOnce(&Path) -> Result<()>,
    mut replace: impl FnMut(&Path, &Path) -> Result<()>,
) -> Result<Plan> {
    let pending = if let Some(value) = load(root)? {
        value
    } else {
        if !product_contract::commands::revision(revision) || registered.installation_key != key {
            return Err("suite_shortcut_conflict");
        }
        required(dir, registered)?;
        if registered.files.len() != 4
            || NAMES
                .iter()
                .any(|name| !registered.files.iter().any(|file| file.relative == *name))
        {
            return Err("suite_shortcut_conflict");
        }
        let directory = format!(".devbox-shortcuts-{}", uuid::Uuid::new_v4());
        ensure_no_links(dir).map_err(|_| "suite_shortcut_directory_unsafe")?;
        let staging = dir.join(&directory);
        fs::create_dir(&staging).map_err(|_| "suite_shortcut_unavailable")?;
        // An interruption before receipt publication leaves a unique orphan. It is
        // preserved and never selected, overwritten or treated as registered links.
        prepare(&staging)?;
        let staged = Plan::capture(
            dir,
            key,
            &NAMES
                .iter()
                .map(|name| format!("{directory}/{name}"))
                .collect::<Vec<_>>(),
        )?;
        let value = Pending {
            schema_version: 1,
            revision: revision.into(),
            original: registered.clone(),
            staged,
            directory,
        };
        atomic_write(
            root.join(RECEIPT),
            &serde_json::to_vec(&value).map_err(|_| "suite_shortcut_unavailable")?,
        )
        .map_err(|_| "suite_shortcut_unavailable")?;
        value
    };
    let adopted = validate(&pending, key, revision)?;
    if !same(registered, &pending.original)? && !same(registered, &adopted)? {
        return Err("suite_shortcut_conflict");
    }
    let mut remaining = Vec::new();
    // Validate the entire mixed old/new set before any additional mutation.
    for name in NAMES {
        let old = one(&pending.original, name)?;
        let new = one(&adopted, name)?;
        if required(dir, &new).is_ok() {
            continue;
        }
        required(dir, &old)?;
        let staged = one(&pending.staged, &format!("{}/{name}", pending.directory))?;
        required(dir, &staged)?;
        remaining.push((name, staged));
    }
    for (name, staged) in remaining {
        // Recheck each owned object immediately before its one native rename.
        required(dir, &one(&pending.original, name)?)?;
        required(dir, &staged)?;
        replace(&dir.join(&staged.files[0].relative), &dir.join(name))?;
        required(dir, &one(&adopted, name)?)?;
    }
    required(dir, &adopted)?;
    Ok(adopted)
}
pub fn has_pending(root: &Path) -> Result<bool> {
    Ok(load(root)?.is_some())
}
pub fn finish(root: &Path, dir: &Path, key: &str, revision: &str, registered: &Plan) -> Result<()> {
    let Some(pending) = load(root)? else {
        return Ok(());
    };
    let adopted = validate(&pending, key, revision)?;
    if !same(registered, &adopted)? {
        return Err("suite_shortcut_conflict");
    }
    required(dir, &adopted)?;
    let staging = dir.join(pending.directory);
    if staging.exists() {
        ensure_no_links(&staging).map_err(|_| "suite_shortcut_directory_unsafe")?;
        // Unknown entries are never recursively removed.
        if fs::read_dir(&staging)
            .map_err(|_| "suite_shortcut_unavailable")?
            .next()
            .is_none()
        {
            fs::remove_dir(&staging).map_err(|_| "suite_shortcut_unavailable")?;
        }
    }
    ensure_no_links(root.join(RECEIPT)).map_err(|_| "suite_shortcut_directory_unsafe")?;
    fs::remove_file(root.join(RECEIPT)).map_err(|_| "suite_shortcut_unavailable")
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir()
                .join(format!("devbox-shortcut-adoption-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(root.join("links")).unwrap();
            for name in NAMES {
                fs::write(root.join("links").join(name), format!("old-{name}")).unwrap();
            }
            Self(root)
        }
        fn plan(&self) -> Plan {
            Plan::capture(
                &self.0.join("links"),
                &"b".repeat(64),
                &NAMES.map(String::from),
            )
            .unwrap()
        }
        fn prepare(dir: &Path) -> Result<()> {
            for name in NAMES {
                fs::write(dir.join(name), format!("current-{name}")).unwrap();
            }
            Ok(())
        }
        fn rename(source: &Path, target: &Path) -> Result<()> {
            fs::rename(source, target).map_err(|_| "suite_shortcut_unavailable")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn publishes_all_four_and_finishes_only_after_registration_save() {
        let f = Fixture::new();
        let original = f.plan();
        let next = adopt(
            &f.0,
            &f.0.join("links"),
            &"b".repeat(64),
            &"a".repeat(64),
            &original,
            Fixture::prepare,
            Fixture::rename,
        )
        .unwrap();
        assert!(has_pending(&f.0).unwrap());
        assert!(finish(
            &f.0,
            &f.0.join("links"),
            &"b".repeat(64),
            &"a".repeat(64),
            &original
        )
        .is_err());
        finish(
            &f.0,
            &f.0.join("links"),
            &"b".repeat(64),
            &"a".repeat(64),
            &next,
        )
        .unwrap();
        assert!(!has_pending(&f.0).unwrap());
        required(&f.0.join("links"), &next).unwrap();
    }
    #[test]
    fn every_interrupted_link_position_resumes_mixed_exact_identities() {
        for index in 0..4 {
            for after in [false, true] {
                let f = Fixture::new();
                let original = f.plan();
                let mut count = 0;
                let first = adopt(
                    &f.0,
                    &f.0.join("links"),
                    &"b".repeat(64),
                    &"a".repeat(64),
                    &original,
                    Fixture::prepare,
                    |source, target| {
                        let fail = count == index;
                        count += 1;
                        if !fail || after {
                            Fixture::rename(source, target)?;
                        }
                        if fail {
                            Err("interrupted")
                        } else {
                            Ok(())
                        }
                    },
                );
                assert_eq!(first.unwrap_err(), "interrupted");
                let next = adopt(
                    &f.0,
                    &f.0.join("links"),
                    &"b".repeat(64),
                    &"a".repeat(64),
                    &original,
                    |_| panic!("Receipt must retain prepared links"),
                    Fixture::rename,
                )
                .unwrap();
                let saved = adopt(
                    &f.0,
                    &f.0.join("links"),
                    &"b".repeat(64),
                    &"a".repeat(64),
                    &next,
                    |_| panic!("Already published"),
                    |_, _| panic!("Already published"),
                )
                .unwrap();
                assert!(same(&saved, &next).unwrap());
            }
        }
    }
    #[test]
    fn foreign_fourth_link_and_wrong_revision_reject_before_any_more_mutation() {
        let f = Fixture::new();
        let original = f.plan();
        assert!(adopt(
            &f.0,
            &f.0.join("links"),
            &"b".repeat(64),
            &"a".repeat(64),
            &original,
            Fixture::prepare,
            |_, _| Err("interrupted")
        )
        .is_err());
        assert!(adopt(
            &f.0,
            &f.0.join("links"),
            &"b".repeat(64),
            &"c".repeat(64),
            &original,
            |_| panic!("Wrong revision"),
            |_, _| panic!("Wrong revision")
        )
        .is_err());
        fs::write(f.0.join("links/Control Center.lnk"), b"foreign").unwrap();
        assert!(adopt(
            &f.0,
            &f.0.join("links"),
            &"b".repeat(64),
            &"a".repeat(64),
            &original,
            |_| panic!("Receipt exists"),
            |_, _| panic!("Preflight must reject before first replacement")
        )
        .is_err());
        assert_eq!(
            fs::read(f.0.join("links/Workspace.lnk")).unwrap(),
            b"old-Workspace.lnk"
        );
        assert_eq!(
            fs::read(f.0.join("links/Control Center.lnk")).unwrap(),
            b"foreign"
        );
    }
    #[test]
    fn unreceipted_unique_orphan_does_not_authorize_or_block_publication() {
        let f = Fixture::new();
        let original = f.plan();
        let mut orphan = None;
        assert!(adopt(
            &f.0,
            &f.0.join("links"),
            &"b".repeat(64),
            &"a".repeat(64),
            &original,
            |dir| {
                orphan = Some(dir.to_path_buf());
                fs::write(dir.join("Workspace.lnk"), b"partial").unwrap();
                Err("interrupted")
            },
            |_, _| panic!("Preparation failed")
        )
        .is_err());
        let next = adopt(
            &f.0,
            &f.0.join("links"),
            &"b".repeat(64),
            &"a".repeat(64),
            &original,
            Fixture::prepare,
            Fixture::rename,
        )
        .unwrap();
        required(&f.0.join("links"), &next).unwrap();
        assert_eq!(
            fs::read(orphan.unwrap().join("Workspace.lnk")).unwrap(),
            b"partial"
        );
    }
}
