//! Verified NSIS replacement staged during Health, published only at commit.
//! The original shortcut Registration remains readable by its older helper.
use super::suite_removal::Plan;
use devbox_filesystem::{atomic_write, ensure_no_links, open_filesystem_object};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};
type Result<T> = std::result::Result<T, &'static str>;
const NEXT: &str = "suite-uninstaller-next.json";
const DISPATCHER: &str = "suite-uninstaller-dispatcher.json";
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pending {
    schema_version: u32,
    revision: String,
    prepared: bool,
    staged: Plan,
    original: Plan,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Dispatcher {
    schema_version: u32,
    revision: String,
    current: Plan,
    previous_revision: Option<String>,
    previous: Plan,
}
fn valid_dispatcher(value: &Dispatcher, key: &str) -> bool {
    value.schema_version == 1
        && product_contract::commands::revision(&value.revision)
        && value
            .previous_revision
            .as_deref()
            .is_none_or(product_contract::commands::revision)
        && value.current.installation_key == key
        && value.previous.installation_key == key
        && value.current.root_identity == value.previous.root_identity
        && [&value.current, &value.previous]
            .iter()
            .all(|plan| plan.files.len() == 1 && plan.files[0].relative == "Uninstall.exe")
}
fn same(a: &Plan, b: &Plan) -> Result<bool> {
    Ok(
        serde_json::to_vec(a).map_err(|_| "suite_uninstaller_invalid")?
            == serde_json::to_vec(b).map_err(|_| "suite_uninstaller_invalid")?,
    )
}
fn complete(root: &Path, plan: &Plan, key: &str) -> Result<()> {
    if plan.installation_key != key || plan.files.len() != 1 {
        return Err("suite_uninstaller_invalid");
    }
    plan.verify_remaining(root)?;
    if !root.join(&plan.files[0].relative).is_file() {
        return Err("suite_uninstaller_missing");
    }
    Ok(())
}
fn read<T: serde::de::DeserializeOwned>(root: &Path, name: &str) -> Result<Option<T>> {
    let path = root.join(name);
    if matches!(fs::symlink_metadata(&path),Err(e) if e.kind()==std::io::ErrorKind::NotFound) {
        return Ok(None);
    }
    ensure_no_links(&path).map_err(|_| "suite_uninstaller_invalid")?;
    let (mut file, _) =
        open_filesystem_object(&path, false).map_err(|_| "suite_uninstaller_missing")?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "suite_uninstaller_invalid")?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("suite_uninstaller_invalid");
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "suite_uninstaller_invalid")
}
fn save<T: Serialize>(root: &Path, name: &str, value: &T) -> Result<()> {
    atomic_write(
        root.join(name),
        &serde_json::to_vec(value).map_err(|_| "suite_uninstaller_invalid")?,
    )
    .map_err(|_| "suite_uninstaller_invalid")
}
fn valid_pending(pending: &Pending, revision: &str) -> bool {
    let prefix = format!("setup/{revision}/Uninstall-");
    pending.schema_version == 1
        && pending.revision == revision
        && pending.staged.root_identity == pending.original.root_identity
        && pending.staged.installation_key == pending.original.installation_key
        && pending.staged.files.len() == 1
        && pending.original.files.len() == 1
        && pending.original.files[0].relative == "Uninstall.exe"
        && pending.staged.files[0]
            .relative
            .strip_prefix(&prefix)
            .and_then(|name| name.strip_suffix(".exe"))
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id))
}
pub fn stage(root: &Path, key: &str, revision: &str, source: &Path, original: &Plan) -> Result<()> {
    if !product_contract::commands::revision(revision)
        || original.files.len() != 1
        || original.files[0].relative != "Uninstall.exe"
    {
        return Err("suite_uninstaller_invalid");
    }
    complete(root, original, key)?;
    let source_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("suite_uninstaller_invalid")?;
    let source_proof = Plan::capture(
        source.parent().ok_or("suite_uninstaller_invalid")?,
        key,
        &[source_name.into()],
    )?;
    let (mut input, input_identity) =
        open_filesystem_object(source, false).map_err(|_| "suite_uninstaller_missing")?;
    let mut pending = if let Some(value) = read::<Pending>(root, NEXT)? {
        if !valid_pending(&value, revision)
            || !same(&value.original, original)?
            || value.staged.installation_key != key
            || value.staged.files[0].sha256 != source_proof.files[0].sha256
            || value.staged.files[0].bytes != source_proof.files[0].bytes
        {
            return Err("suite_uninstaller_invalid");
        }
        if value.prepared {
            return complete(root, &value.staged, key);
        }
        value
    } else {
        // A crash before publishing ownership can leave only a unique orphan,
        // never block a fixed destination or authorize adopting an unknown file.
        let relative = format!("setup/{revision}/Uninstall-{}.exe", uuid::Uuid::new_v4());
        let target = root.join(&relative);
        ensure_no_links(target.parent().ok_or("suite_uninstaller_invalid")?)
            .map_err(|_| "suite_uninstaller_invalid")?;
        let output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|_| "suite_uninstaller_invalid")?;
        let mut staged = Plan::capture(root, key, &[relative])?;
        staged.files[0].sha256 = source_proof.files[0].sha256.clone();
        staged.files[0].bytes = source_proof.files[0].bytes;
        let value = Pending {
            schema_version: 1,
            revision: revision.into(),
            prepared: false,
            staged,
            original: original.clone(),
        };
        // Persist the exact created file ID and expected content before copying;
        // incomplete copies can resume only into this same owned file object.
        save(root, NEXT, &value)?;
        drop(output);
        value
    };
    let target = root.join(&pending.staged.files[0].relative);
    ensure_no_links(&target).map_err(|_| "suite_uninstaller_invalid")?;
    let mut options = fs::OpenOptions::new();
    options.write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let mut output = options
        .open(&target)
        .map_err(|_| "suite_uninstaller_missing")?;
    if devbox_filesystem::opened_filesystem_identity(&output, false)
        .map_err(|_| "suite_uninstaller_invalid")?
        .components()
        != pending.staged.files[0].identity
    {
        return Err("suite_uninstaller_invalid");
    }
    output.set_len(0).map_err(|_| "suite_uninstaller_invalid")?;
    let copied = std::io::copy(
        &mut Read::by_ref(&mut input).take(512 * 1024 * 1024 + 1),
        &mut output,
    )
    .map_err(|_| "suite_uninstaller_invalid")?;
    if copied > 512 * 1024 * 1024
        || devbox_filesystem::filesystem_identity(source, false)
            .map_err(|_| "suite_uninstaller_invalid")?
            != input_identity
    {
        return Err("suite_uninstaller_invalid");
    }
    output
        .flush()
        .and_then(|()| output.sync_all())
        .map_err(|_| "suite_uninstaller_invalid")?;
    drop(output);
    source_proof.verify_remaining(source.parent().ok_or("suite_uninstaller_invalid")?)?;
    complete(root, &pending.staged, key)?;
    pending.prepared = true;
    save(root, NEXT, &pending)
}
pub fn remember(root: &Path, key: &str, revision: &str, registered: &Plan) -> Result<()> {
    complete(root, registered, key)?;
    if let Some(value) = read::<Dispatcher>(root, DISPATCHER)? {
        if !valid_dispatcher(&value, key) || !same(&value.current, registered)? {
            return Err("suite_uninstaller_invalid");
        }
        return complete(root, &value.current, key);
    }
    if !product_contract::commands::revision(revision) {
        return Err("suite_uninstaller_invalid");
    }
    save(
        root,
        DISPATCHER,
        &Dispatcher {
            schema_version: 1,
            revision: revision.into(),
            current: registered.clone(),
            previous_revision: None,
            previous: registered.clone(),
        },
    )
}
pub fn trusted(root: &Path, key: &str, revision: &str, registered: &Plan) -> Result<bool> {
    let Some(value) = read::<Dispatcher>(root, DISPATCHER)? else {
        return Ok(false);
    };
    if !valid_dispatcher(&value, key) {
        return Err("suite_uninstaller_invalid");
    }
    let selected = if value.revision == revision && same(&value.current, registered)? {
        Some(&value.current)
    } else if value.previous_revision.as_deref() == Some(revision)
        && same(&value.previous, registered)?
    {
        Some(&value.previous)
    } else {
        None
    };
    if let Some(plan) = selected {
        complete(root, plan, key)?;
        return Ok(true);
    }
    Ok(false)
}
/// `replace` must rename on the same volume, preserving the captured file ID.
/// Caller holds the installation writer gate and verified commit journal.
pub fn adopt(
    root: &Path,
    key: &str,
    revision: &str,
    registered: &Plan,
    replace: impl FnOnce(&Path, &Path) -> Result<()>,
) -> Result<Option<Plan>> {
    let Some(pending) = read::<Pending>(root, NEXT)? else {
        return Ok(None);
    };
    if !valid_pending(&pending, revision) || !pending.prepared {
        return Err("suite_uninstaller_invalid");
    }
    let mut adopted = pending.staged.clone();
    adopted.files[0].relative = "Uninstall.exe".into();
    if pending.original.files.len() != 1 || pending.original.files[0].relative != "Uninstall.exe" {
        return Err("suite_uninstaller_invalid");
    }
    if !same(registered, &pending.original)? && !same(registered, &adopted)? {
        return Err("suite_uninstaller_invalid");
    }
    if root.join(&pending.staged.files[0].relative).exists() {
        complete(root, &pending.original, key)?;
        complete(root, &pending.staged, key)?;
        let prior = read::<Dispatcher>(root, DISPATCHER)?;
        let previous_revision = match prior {
            Some(value)
                if valid_dispatcher(&value, key) && same(&value.current, &pending.original)? =>
            {
                Some(value.revision)
            }
            Some(value)
                if valid_dispatcher(&value, key)
                    && same(&value.current, &adopted)?
                    && same(&value.previous, &pending.original)? =>
            {
                value.previous_revision
            }
            None => None,
            _ => return Err("suite_uninstaller_invalid"),
        };
        save(
            root,
            DISPATCHER,
            &Dispatcher {
                schema_version: 1,
                revision: revision.into(),
                current: adopted.clone(),
                previous_revision,
                previous: pending.original.clone(),
            },
        )?;
        replace(
            &root.join(&pending.staged.files[0].relative),
            &root.join("Uninstall.exe"),
        )?;
    } else {
        // A crashed rename is recoverable only from the previously published exact
        // destination identity, never by accepting arbitrary same-name bytes.
        let value = read::<Dispatcher>(root, DISPATCHER)?.ok_or("suite_uninstaller_invalid")?;
        if !valid_dispatcher(&value, key)
            || value.revision != revision
            || !same(&value.current, &adopted)?
            || !same(&value.previous, &pending.original)?
        {
            return Err("suite_uninstaller_invalid");
        }
    }
    complete(root, &adopted, key)?;
    Ok(Some(adopted))
}
pub fn finish(root: &Path) -> Result<()> {
    if matches!(fs::symlink_metadata(root.join(NEXT)),Err(e) if e.kind()==std::io::ErrorKind::NotFound)
    {
        return Ok(());
    }
    ensure_no_links(root.join(NEXT)).map_err(|_| "suite_uninstaller_invalid")?;
    fs::remove_file(root.join(NEXT)).map_err(|_| "suite_uninstaller_invalid")
}

fn remaining_staged(root: &Path, pending: &Pending, key: &str) -> Result<Option<Plan>> {
    if pending.staged.installation_key != key {
        return Err("suite_uninstaller_invalid");
    }
    pending.staged.validate()?;
    let path = root.join(&pending.staged.files[0].relative);
    if matches!(fs::symlink_metadata(&path),Err(e) if e.kind()==std::io::ErrorKind::NotFound) {
        pending.staged.verify_remaining(root)?;
        return Ok(None);
    }
    let (_pin, identity) =
        open_filesystem_object(&path, false).map_err(|_| "suite_uninstaller_invalid")?;
    if identity.components() != pending.staged.files[0].identity {
        return Err("suite_uninstaller_invalid");
    }
    if pending.prepared {
        complete(root, &pending.staged, key)?;
        return Ok(Some(pending.staged.clone()));
    }
    // Only this exact native-created staging object was authorized for copying.
    // Its incomplete bytes are not adopted; rollback may remove its partial
    // content after pinning and rechecking the captured object identity.
    let actual = Plan::capture(root, key, &[pending.staged.files[0].relative.clone()])?;
    if actual.root_identity != pending.staged.root_identity
        || actual.files[0].identity != identity.components()
    {
        return Err("suite_uninstaller_invalid");
    }
    Ok(Some(actual))
}
pub fn discard(root: &Path, key: &str, revision: &str, registered: &Plan) -> Result<()> {
    let Some(pending) = read::<Pending>(root, NEXT)? else {
        return Ok(());
    };
    if !valid_pending(&pending, revision) || !same(&pending.original, registered)? {
        return Err("suite_uninstaller_invalid");
    }
    complete(root, registered, key)?;
    if pending.staged.installation_key != key {
        return Err("suite_uninstaller_invalid");
    }
    if let Some(remaining) = remaining_staged(root, &pending, key)? {
        remaining.remove(root)?;
    }
    finish(root)
}
pub fn owned_files(root: &Path, key: &str) -> Result<Vec<String>> {
    let mut files = Vec::new();
    if let Some(pending) = read::<Pending>(root, NEXT)? {
        if !product_contract::commands::revision(&pending.revision)
            || !valid_pending(&pending, &pending.revision)
        {
            return Err("suite_uninstaller_invalid");
        }
        files.push(NEXT.into());
        if remaining_staged(root, &pending, key)?.is_some() {
            files.push(pending.staged.files[0].relative.clone())
        }
    }
    if let Some(dispatcher) = read::<Dispatcher>(root, DISPATCHER)? {
        if !valid_dispatcher(&dispatcher, key) {
            return Err("suite_uninstaller_invalid");
        }
        complete(root, &dispatcher.current, key)?;
        files.push(DISPATCHER.into());
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "devbox-uninstaller-adoption-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(root.join("setup").join("a".repeat(64))).unwrap();
            fs::create_dir_all(root.join("incoming")).unwrap();
            fs::write(root.join("Uninstall.exe"), b"old-owned-uninstaller").unwrap();
            fs::write(
                root.join("incoming/Uninstall.exe"),
                b"current-owned-uninstaller",
            )
            .unwrap();
            Self(root)
        }
        fn original(&self) -> Plan {
            Plan::capture(&self.0, &"b".repeat(64), &["Uninstall.exe".into()]).unwrap()
        }
        fn stage(&self, original: &Plan) {
            stage(
                &self.0,
                &"b".repeat(64),
                &"a".repeat(64),
                &self.0.join("incoming/Uninstall.exe"),
                original,
            )
            .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn rename(source: &Path, target: &Path) -> Result<()> {
        fs::rename(source, target).map_err(|_| "suite_uninstaller_invalid")
    }
    #[test]
    fn health_stages_without_changing_original_then_commit_publishes_exact_identity() {
        let f = Fixture::new();
        let original = f.original();
        f.stage(&original);
        complete(&f.0, &original, &"b".repeat(64)).unwrap();
        assert!(!trusted(&f.0, &"b".repeat(64), &"a".repeat(64), &original).unwrap());
        let adopted = adopt(&f.0, &"b".repeat(64), &"a".repeat(64), &original, rename)
            .unwrap()
            .unwrap();
        assert!(trusted(&f.0, &"b".repeat(64), &"a".repeat(64), &adopted).unwrap());
        assert_eq!(
            fs::read(f.0.join("Uninstall.exe")).unwrap(),
            b"current-owned-uninstaller"
        );
        finish(&f.0).unwrap();
        assert!(!f.0.join(NEXT).exists());
        assert_eq!(
            owned_files(&f.0, &"b".repeat(64)).unwrap(),
            vec![DISPATCHER.to_string()]
        );
    }
    #[test]
    fn no_nsis_stage_and_rollback_preserve_original() {
        let f = Fixture::new();
        let original = f.original();
        assert!(adopt(
            &f.0,
            &"b".repeat(64),
            &"a".repeat(64),
            &original,
            |_, _| panic!("No NSIS stage must not mutate")
        )
        .unwrap()
        .is_none());
        f.stage(&original);
        discard(&f.0, &"b".repeat(64), &"a".repeat(64), &original).unwrap();
        complete(&f.0, &original, &"b".repeat(64)).unwrap();
        assert!(!f.0.join(NEXT).exists());
        assert!(!f.0.join(DISPATCHER).exists());
    }
    #[test]
    fn resume_before_and_after_identity_preserving_rename() {
        for after in [false, true] {
            let f = Fixture::new();
            let original = f.original();
            f.stage(&original);
            assert_eq!(
                adopt(
                    &f.0,
                    &"b".repeat(64),
                    &"a".repeat(64),
                    &original,
                    |source, target| {
                        if after {
                            rename(source, target)?;
                        }
                        Err("interrupted")
                    }
                )
                .unwrap_err(),
                "interrupted"
            );
            let adopted = adopt(&f.0, &"b".repeat(64), &"a".repeat(64), &original, rename)
                .unwrap()
                .unwrap();
            complete(&f.0, &adopted, &"b".repeat(64)).unwrap();
            let again = adopt(&f.0, &"b".repeat(64), &"a".repeat(64), &adopted, |_, _| {
                panic!("Already published")
            })
            .unwrap()
            .unwrap();
            assert!(same(&again, &adopted).unwrap());
        }
    }
    #[test]
    fn foreign_target_staged_path_and_wrong_generation_are_never_overwritten() {
        let f = Fixture::new();
        let original = f.original();
        fs::write(
            f.0.join("setup").join("a".repeat(64)).join("Uninstall.exe"),
            b"foreign",
        )
        .unwrap();
        stage(
            &f.0,
            &"b".repeat(64),
            &"a".repeat(64),
            &f.0.join("incoming/Uninstall.exe"),
            &original,
        )
        .unwrap();
        assert_eq!(
            fs::read(f.0.join("setup").join("a".repeat(64)).join("Uninstall.exe")).unwrap(),
            b"foreign"
        );
        let f = Fixture::new();
        let original = f.original();
        f.stage(&original);
        assert!(adopt(
            &f.0,
            &"b".repeat(64),
            &"c".repeat(64),
            &original,
            |_, _| panic!("Wrong generation")
        )
        .is_err());
        fs::write(f.0.join("Uninstall.exe"), b"foreign").unwrap();
        assert!(adopt(
            &f.0,
            &"b".repeat(64),
            &"a".repeat(64),
            &original,
            |_, _| panic!("Foreign uninstaller")
        )
        .is_err());
        assert_eq!(fs::read(f.0.join("Uninstall.exe")).unwrap(), b"foreign");
    }
    #[test]
    fn owned_staged_files_are_explicit_and_missing_stage_cannot_be_recaptured() {
        let f = Fixture::new();
        let original = f.original();
        f.stage(&original);
        let files = owned_files(&f.0, &"b".repeat(64)).unwrap();
        assert_eq!(files[0], NEXT);
        assert!(files[1].starts_with(&format!("setup/{}/Uninstall-", "a".repeat(64))));
        fs::remove_file(f.0.join(&files[1])).unwrap();
        assert!(adopt(
            &f.0,
            &"b".repeat(64),
            &"a".repeat(64),
            &original,
            |_, _| panic!("Missing captured stage")
        )
        .is_err());
    }
    #[test]
    fn interrupted_copy_resumes_only_owned_id_and_rollback_deletion_resumes() {
        let f = Fixture::new();
        let original = f.original();
        f.stage(&original);
        let mut pending = read::<Pending>(&f.0, NEXT).unwrap().unwrap();
        pending.prepared = false;
        save(&f.0, NEXT, &pending).unwrap();
        fs::write(f.0.join(&pending.staged.files[0].relative), b"partial").unwrap();
        f.stage(&original);
        complete(&f.0, &pending.staged, &"b".repeat(64)).unwrap();
        pending.staged.remove(&f.0).unwrap();
        discard(&f.0, &"b".repeat(64), &"a".repeat(64), &original).unwrap();
        assert!(!f.0.join(NEXT).exists());
        complete(&f.0, &original, &"b".repeat(64)).unwrap();
    }
    #[test]
    fn rollback_removes_only_owned_incomplete_stage_and_preserves_replacement() {
        for foreign in [false, true] {
            let f = Fixture::new();
            let original = f.original();
            f.stage(&original);
            let mut pending = read::<Pending>(&f.0, NEXT).unwrap().unwrap();
            pending.prepared = false;
            save(&f.0, NEXT, &pending).unwrap();
            let target = f.0.join(&pending.staged.files[0].relative);
            if foreign {
                fs::rename(&target, target.with_extension("retained")).unwrap();
                fs::write(&target, b"foreign").unwrap();
            } else {
                fs::write(&target, b"partial").unwrap();
            }
            let result = discard(&f.0, &"b".repeat(64), &"a".repeat(64), &original);
            if foreign {
                assert!(result.is_err());
                assert_eq!(fs::read(target).unwrap(), b"foreign");
            } else {
                result.unwrap();
                assert!(!target.exists());
                assert!(!f.0.join(NEXT).exists());
            }
            complete(&f.0, &original, &"b".repeat(64)).unwrap();
        }
    }
    #[test]
    fn in_app_shortcut_revision_change_retains_only_verified_uninstaller_dispatcher() {
        let f = Fixture::new();
        let original = f.original();
        remember(&f.0, &"b".repeat(64), &"c".repeat(64), &original).unwrap();
        assert!(trusted(&f.0, &"b".repeat(64), &"c".repeat(64), &original).unwrap());
        // Advancing links alone must not claim the old uninstaller embeds the
        // newer payload, nor replace its independently remembered revision.
        remember(&f.0, &"b".repeat(64), &"a".repeat(64), &original).unwrap();
        assert!(trusted(&f.0, &"b".repeat(64), &"c".repeat(64), &original).unwrap());
        assert!(!trusted(&f.0, &"b".repeat(64), &"a".repeat(64), &original).unwrap());
        fs::write(f.0.join("Uninstall.exe"), b"foreign").unwrap();
        assert!(trusted(&f.0, &"b".repeat(64), &"c".repeat(64), &original).is_err());
    }
}
