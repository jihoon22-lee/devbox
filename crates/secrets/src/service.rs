use crate::{
    crypto::{self, Key},
    keystore::DpapiKeyStore,
    store, *,
};
use devbox_core::{Db, Paths};
use devbox_protocol::{
    Done, Fail, Internal,
    secrets::{Bootstrap, KeyStatus, Unlock},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tokio::sync::{Mutex, OwnedMutexGuard};
use zeroize::Zeroizing;
pub struct SecretsService {
    db: Db,
    paths: Paths,
    keystore: DpapiKeyStore,
    key: Arc<Mutex<Option<Key>>>,
    mismatch: Arc<std::sync::atomic::AtomicBool>,
}
impl SecretsService {
    pub fn new(db: Db, paths: Paths) -> Self {
        let keystore = DpapiKeyStore::new(&paths.data_dir);
        Self {
            db,
            paths,
            keystore,
            key: Arc::new(Mutex::new(None)),
            mismatch: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
    pub fn with_keystore(mut self, keystore: DpapiKeyStore) -> Self {
        self.keystore = keystore;
        self
    }
    pub async fn with_key(db: Db, paths: Paths, key: Key) -> R<Self> {
        let service = Self::new(db, paths);
        let id = key.id();
        service
            .db
            .write(move |tx| store::init(tx, &id))
            .await
            .internal("initialize fixture key")?;
        *service.key.lock().await = Some(key);
        Ok(service)
    }
    fn locked_error(&self) -> SecretsError {
        if self.mismatch.load(std::sync::atomic::Ordering::Acquire) {
            SecretsError::KeyMismatch
        } else {
            SecretsError::Locked
        }
    }
    async fn id(&self) -> R<Option<String>> {
        self.db
            .read(store::key_id)
            .await
            .internal("read key identity")
    }
    fn mark_mismatch(&self) {
        if !self
            .mismatch
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            tracing::warn!(code = "secrets.key_mismatch", "secret key remains locked");
        }
    }
    pub async fn initialize(&self) -> R<()> {
        let mut key = self.key.lock().await;
        if key.is_some() {
            return Ok(());
        }
        let Some(id) = self.id().await? else {
            return Ok(());
        };
        match self.keystore.stored_key(&id).blob() {
            Ok(Some(blob)) => match self.keystore.unseal(&blob).await {
                Ok(candidate) if candidate.id() == id => {
                    *key = Some(candidate);
                    self.mismatch
                        .store(false, std::sync::atomic::Ordering::Release);
                }
                Err(SecretsError::Unavailable) => {}
                _ => self.mark_mismatch(),
            },
            Err(SecretsError::Unavailable) => {}
            _ => self.mark_mismatch(),
        }
        Ok(())
    }
    pub async fn status(&self) -> R<KeyStatus> {
        let key = self.key.lock().await;
        let id = self.id().await?;
        let keystore = id
            .as_deref()
            .map(|id| self.keystore.stored_key(id))
            .unwrap_or_else(|| self.keystore.clone());
        let blob = match keystore.blob() {
            Ok(blob) => blob,
            Err(_) => {
                self.mark_mismatch();
                None
            }
        };
        if key.is_none() && id.is_some() != blob.is_some() {
            self.mark_mismatch();
        }
        Ok(KeyStatus {
            locked: key.is_none(),
            key_mismatch: self.mismatch.load(std::sync::atomic::Ordering::Acquire),
            key_id: id,
            blob,
        })
    }
    pub async fn public_status(&self) -> R<PublicKeyStatus> {
        let status = self.status().await?;
        Ok(PublicKeyStatus {
            locked: status.locked,
            key_mismatch: status.key_mismatch,
        })
    }
    pub async fn bootstrap(&self, input: Bootstrap) -> R<Done> {
        let key = self.key.clone().lock_owned().await;
        if self.id().await?.is_some() || self.keystore.blob()?.is_some() {
            return Err(SecretsError::KeyMismatch.into());
        }
        if input.key_id.len() != 64
            || !input
                .key_id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(SecretsError::InvalidInput.into());
        }
        let rev = self
            .commit_initial_key(key, input.key_id, &input.blob, None)
            .await?;
        Ok(Done {
            rev: rev.to_string(),
        })
    }
    pub async fn unlock(&self, input: Unlock) -> R<Done> {
        let mut current = self.key.lock().await;
        let key = Key::from_bytes(
            input
                .key
                .as_slice()
                .try_into()
                .map_err(|_| SecretsError::InvalidInput)?,
        );
        if self.id().await?.as_deref() != Some(&key.id()) {
            return Err(SecretsError::KeyMismatch.into());
        }
        *current = Some(key);
        self.mismatch
            .store(false, std::sync::atomic::Ordering::Release);
        Ok(Done {
            rev: self.db.rev().to_string(),
        })
    }
    async fn commit_initial_key(
        &self,
        mut current: OwnedMutexGuard<Option<Key>>,
        id: String,
        blob: &[u8],
        candidate: Option<Key>,
    ) -> R<u64> {
        // Until the identity commits, only this guard owns the prepared file.
        // Keep both it and the key lock in the writer if the request is cancelled.
        let prepared = self.keystore.prepare_key(&id, blob)?;
        let mismatch = self.mismatch.clone();
        let result = self
            .db
            .write_checked_with_commit::<_, std::convert::Infallible, _, _>(
                move |tx| store::init(tx, &id).map(Ok),
                move |_, _| {
                    prepared.commit();
                    *current = candidate;
                    mismatch.store(false, std::sync::atomic::Ordering::Release);
                },
            )
            .await
            .internal("record data key identity")?;
        match result {
            Ok(((), rev)) => Ok(rev),
            Err(never) => match never {},
        }
    }
    async fn ensure_key(&self) -> R<OwnedMutexGuard<Option<Key>>> {
        let key = self.key.clone().lock_owned().await;
        if key.is_some() {
            return Ok(key);
        }
        if self.id().await?.is_some() || self.keystore.blob()?.is_some() {
            return Err(self.locked_error().into());
        }
        let (created, blob) = self
            .keystore
            .create()
            .await
            .map_err(|_| SecretsError::Locked)?;
        let id = created.id();
        self.commit_initial_key(key, id, &blob, Some(created))
            .await?;
        Ok(self.key.clone().lock_owned().await)
    }
    pub async fn list(&self) -> R<Vec<SecretInfo>> {
        let configured = devbox_core::Config::load(&self.paths)
            .internal("read secret names")?
            .secrets
            .names;
        let values = self
            .db
            .read(store::names)
            .await
            .internal("list secret names")?;
        let names: BTreeSet<_> = configured
            .into_iter()
            .chain(values.iter().cloned())
            .collect();
        Ok(names
            .into_iter()
            .map(|name| SecretInfo {
                has_value: values.contains(&name),
                name,
            })
            .collect())
    }
    pub async fn set(&self, input: SetSecret) -> R<Done> {
        if !valid_name(&input.name) || input.value.len() > 256 * 1024 || input.value.contains('\0')
        {
            return Err(SecretsError::InvalidInput.into());
        }
        let key = self.ensure_key().await?;
        let key = key.as_ref().expect("initialized key");
        let id = key.id();
        let blob = crypto::seal(key, &input.name, &id, input.value.as_bytes())?;
        let (config_dir, name) = (self.paths.config_dir.clone(), input.name.clone());
        tokio::task::spawn_blocking(move || Self::save_name(&config_dir, &name))
            .await
            .internal("save secret name")??;
        let name = input.name.clone();
        let (_, rev) = self
            .db
            .write(move |tx| store::set(tx, &name, &id, &blob))
            .await
            .internal("store encrypted secret")?;
        Ok(Done {
            rev: rev.to_string(),
        })
    }
    pub async fn delete(&self, input: Name) -> R<Done> {
        if !valid_name(&input.name) {
            return Err(SecretsError::InvalidInput.into());
        }
        let (_, rev) = self
            .db
            .write(move |tx| store::delete(tx, &input.name))
            .await
            .internal("delete encrypted secret")?;
        Ok(Done {
            rev: rev.to_string(),
        })
    }
    pub async fn export_key(&self) -> R<Recovery> {
        let key = self.key.lock().await;
        Ok(Recovery {
            recovery: key
                .as_ref()
                .ok_or_else(|| self.locked_error())?
                .recovery()
                .to_string(),
        })
    }
    pub async fn import_key(&self, input: Recovery) -> R<Done> {
        let mut current = self.key.lock().await;
        let candidate = Key::recover(&input.recovery)?;
        if self.id().await?.as_deref() != Some(&candidate.id()) {
            return Err(SecretsError::KeyMismatch.into());
        }
        let blob = self.keystore.seal(&candidate).await?;
        self.keystore
            .stored_key(&candidate.id())
            .publish(&blob, true)?;
        *current = Some(candidate);
        self.mismatch
            .store(false, std::sync::atomic::Ordering::Release);
        Ok(Done {
            rev: self.db.rev().to_string(),
        })
    }
    pub async fn reset(&self) -> R<Done> {
        let mut current = self.key.clone().lock_owned().await;
        // Preparation never overwrites the key selected by the current DB identity.
        // Both deletion and the switch to the fully synced sealed key commit together.
        let (candidate, blob) = self.keystore.create().await?;
        let id = candidate.id();
        let prepared = self.keystore.prepare_key(&id, &blob)?;
        let mismatch = self.mismatch.clone();
        let result = self
            .db
            .write_checked_with_commit::<_, std::convert::Infallible, _, _>(
                move |tx| store::reset(tx, &id).map(Ok),
                move |_, _| {
                    prepared.commit();
                    *current = Some(candidate);
                    mismatch.store(false, std::sync::atomic::Ordering::Release);
                },
            )
            .await
            .internal("reset encrypted secrets")?;
        let (_, rev) = match result {
            Ok(value) => value,
            Err(never) => match never {},
        };
        Ok(Done {
            rev: rev.to_string(),
        })
    }
    pub async fn resolve(&self, names: &[String]) -> R<BTreeMap<String, Zeroizing<Vec<u8>>>> {
        if names.is_empty() {
            return Ok(BTreeMap::new());
        }
        if names.iter().any(|n| !valid_name(n)) {
            return Err(SecretsError::InvalidInput.into());
        }
        let key = self.key.lock().await;
        let key = key.as_ref().ok_or_else(|| self.locked_error())?;
        let id = key.id();
        let requested = names.to_vec();
        let rows = self
            .db
            .read(move |tx| {
                requested
                    .into_iter()
                    .map(|name| Ok((name.clone(), store::get(tx, &name)?)))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .await
            .internal("read encrypted secrets")?;
        let missing = rows
            .iter()
            .filter(|(_, v)| v.is_none())
            .map(|(n, _)| n.clone())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(SecretsError::Missing { names: missing }.into());
        }
        let mut values = BTreeMap::new();
        for (name, value) in rows {
            let (stored, blob) = value.expect("present");
            if stored != id {
                return Err(SecretsError::KeyMismatch.into());
            }
            values.insert(name.clone(), crypto::open(key, &name, &id, &blob)?);
        }
        Ok(values)
    }
    /// Blocking. Holds the shared config lock (`activity.set_rules` takes it too).
    fn save_name(config_dir: &std::path::Path, name: &str) -> R<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::create_dir_all(config_dir).internal("create config directory")?;
        let _lock = lock_config(config_dir).map_err(|_| SecretsError::Unavailable)?;
        let path = config_dir.join("config.toml");
        if std::fs::symlink_metadata(&path)
            .is_ok_and(|m| !m.is_file() || m.file_type().is_symlink())
        {
            return Err(SecretsError::InvalidInput.into());
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(_) => return Err(SecretsError::Unavailable.into()),
        };
        let mut document = text
            .parse::<toml_edit::DocumentMut>()
            .map_err(|_| SecretsError::InvalidInput)?;
        let section = document
            .as_table_mut()
            .entry("secrets")
            .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
        let section = section
            .as_table_like_mut()
            .ok_or(SecretsError::InvalidInput)?;
        if !section.contains_key("names") {
            section.insert("names", toml_edit::value(toml_edit::Array::new()));
        }
        let names = section
            .get_mut("names")
            .and_then(toml_edit::Item::as_array_mut)
            .ok_or(SecretsError::InvalidInput)?;
        if names.iter().any(|value| value.as_str().is_none()) {
            return Err(SecretsError::InvalidInput.into());
        }
        if names.iter().any(|n| n.as_str() == Some(name)) {
            return Ok(());
        }
        names.push(name);
        let temp = path.with_extension(format!("secret-names-{}", std::process::id()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|_| SecretsError::Unavailable)?;
        let result = file
            .write_all(document.to_string().as_bytes())
            .and_then(|()| file.sync_all())
            .and_then(|()| std::fs::rename(&temp, path));
        let _ = std::fs::remove_file(temp);
        result.map_err(|_| Fail::Domain(SecretsError::Unavailable))
    }
}
/// `config.toml` writers hold `<config_dir>/.config.toml.lock` exclusively for
/// read → edit → rename, so concurrent writers keep each other's changes.
fn lock_config(dir: &std::path::Path) -> std::io::Result<rustix::fd::OwnedFd> {
    use rustix::fs::{FlockOperation, Mode, OFlags};
    let flags = OFlags::CREATE | OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    let fd = rustix::fs::open(
        dir.join(".config.toml.lock"),
        flags,
        Mode::from_raw_mode(0o600),
    )?;
    rustix::fs::flock(&fd, FlockOperation::LockExclusive)?;
    Ok(fd)
}
