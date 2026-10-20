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
use std::collections::{BTreeMap, BTreeSet};
use tokio::sync::Mutex;
use zeroize::Zeroizing;
pub struct SecretsService {
    db: Db,
    paths: Paths,
    keystore: DpapiKeyStore,
    key: Mutex<Option<Key>>,
    mismatch: std::sync::atomic::AtomicBool,
}
impl SecretsService {
    pub fn new(db: Db, paths: Paths) -> Self {
        let keystore = DpapiKeyStore::new(&paths.data_dir);
        Self {
            db,
            paths,
            keystore,
            key: Mutex::new(None),
            mismatch: std::sync::atomic::AtomicBool::new(false),
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
        match self.keystore.blob() {
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
        let blob = match self.keystore.blob() {
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
    pub async fn bootstrap(&self, input: Bootstrap) -> R<Done> {
        let _key = self.key.lock().await;
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
        self.keystore.publish(&input.blob, false)?;
        let (_, rev) = self
            .db
            .write(move |tx| store::init(tx, &input.key_id))
            .await
            .internal("record sealed key identity")?;
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
    async fn ensure_key(&self, key: &mut Option<Key>) -> R<()> {
        if key.is_some() {
            return Ok(());
        }
        if self.id().await?.is_some() || self.keystore.blob()?.is_some() {
            return Err(self.locked_error().into());
        }
        let (created, blob) = self
            .keystore
            .create()
            .await
            .map_err(|_| SecretsError::Locked)?;
        self.keystore.publish(&blob, false)?;
        let id = created.id();
        self.db
            .write(move |tx| store::init(tx, &id))
            .await
            .internal("record data key identity")?;
        *key = Some(created);
        Ok(())
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
        let mut key = self.key.lock().await;
        self.ensure_key(&mut key).await?;
        let key = key.as_ref().expect("initialized key");
        let id = key.id();
        let blob = crypto::seal(key, &input.name, &id, input.value.as_bytes())?;
        self.save_name(&input.name)?;
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
        self.keystore.publish(&blob, true)?;
        *current = Some(candidate);
        self.mismatch
            .store(false, std::sync::atomic::Ordering::Release);
        Ok(Done {
            rev: self.db.rev().to_string(),
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
    fn save_name(&self, name: &str) -> R<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let path = self.paths.config_dir.join("config.toml");
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
        std::fs::create_dir_all(&self.paths.config_dir).internal("create config directory")?;
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
