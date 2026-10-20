//! Durable exit results and per-invocation identity. No command or environment is persisted.
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use ts_rs::TS;

#[derive(Debug, thiserror::Error)]
pub enum ExitError {
    #[error("invalid run or attempt identity")]
    InvalidIdentity,
    #[error("this run attempt has already started")]
    AlreadyStarted,
    #[error("no current run attempt")]
    NoAttempt,
    #[error("run result I/O: {0}")]
    Io(#[from] io::Error),
    #[error("invalid run result: {0}")]
    Json(#[from] serde_json::Error),
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExitSource {
    Wrapper,
    Systemd,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExitRecord {
    pub attempt_id: String,
    pub code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
    pub ended_ms: u64,
    pub by: ExitSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_result: Option<String>,
}
/// Notification only identifies the durable result; Task 4 reconciles it with current run state.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Exited {
    pub run_id: String,
    pub attempt_id: String,
}

pub fn validate_identity(id: &str) -> Result<(), ExitError> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(ExitError::InvalidIdentity);
    }
    Ok(())
}
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
#[derive(Clone)]
pub struct ExitStore {
    dir: PathBuf,
    run: String,
}
impl ExitStore {
    /// Resolving a store never creates directories: callers validate all inputs first.
    pub fn new(dir: &Path, run: &str) -> Result<Self, ExitError> {
        validate_identity(run)?;
        Ok(Self {
            dir: dir.to_owned(),
            run: run.to_owned(),
        })
    }
    pub fn log_path(&self) -> PathBuf {
        self.dir.join(format!("{}.log", self.run))
    }
    fn path(&self, suffix: &str) -> PathBuf {
        self.dir.join(format!("{}.{}", self.run, suffix))
    }
    fn lock(&self) -> Result<File, ExitError> {
        fs::create_dir_all(&self.dir)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.path("lock"))?;
        file.lock()?;
        Ok(file)
    }
    pub fn current_attempt(&self) -> Result<Option<String>, ExitError> {
        match fs::read_to_string(self.path("attempt")) {
            Ok(id) => {
                validate_identity(&id)?;
                Ok(Some(id))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    /// Keep one CLI writer per run, including across different attempts.
    pub fn execution_guard(&self) -> Result<File, ExitError> {
        fs::create_dir_all(&self.dir)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.path("executing"))?;
        file.try_lock()
            .map_err(|_| io::Error::new(io::ErrorKind::WouldBlock, "run already executing"))?;
        Ok(file)
    }
    /// Called before spawning. Under one lock, retire the previous exit before publishing the new attempt.
    pub fn begin(&self, attempt: &str) -> Result<(), ExitError> {
        validate_identity(attempt)?;
        let _lock = self.lock()?;
        if self.current_attempt()?.as_deref() == Some(attempt) || self.retired(attempt).exists() {
            return Err(ExitError::AlreadyStarted);
        }
        self.advance(attempt)
    }
    fn retired(&self, attempt: &str) -> PathBuf {
        self.path(&format!("{attempt}.retired"))
    }
    fn advance(&self, attempt: &str) -> Result<(), ExitError> {
        // Retirement is durable before switching identity, including attempts with no exit.
        // A crash before the switch is recoverable: the current attempt still takes priority.
        if let Some(old) = self.current_attempt()? {
            atomic_write(&self.retired(&old), b"")?;
        }
        if let Some(old) = self.read()? {
            validate_identity(&old.attempt_id)?;
            let archive = self
                .dir
                .join(format!("{}.{}.exit", self.run, old.attempt_id));
            fs::rename(self.path("exit"), archive)?;
            File::open(&self.dir)?.sync_all()?;
        }
        atomic_write(&self.path("attempt"), attempt.as_bytes())?;
        Ok(())
    }
    pub fn read(&self) -> Result<Option<ExitRecord>, ExitError> {
        match fs::read(self.path("exit")) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    /// A stale or duplicate report is ignored. The first result for this attempt is immutable.
    pub fn finish(&self, record: &ExitRecord) -> Result<bool, ExitError> {
        self.finish_inner(record, false)
    }
    /// Only ExecStopPost with a matching systemd INVOCATION_ID may confirm an
    /// invocation which died before exec could begin. systemd runs StopPost to
    /// completion before restarting; known retired invocations are still ignored.
    pub fn finish_systemd(&self, record: &ExitRecord) -> Result<bool, ExitError> {
        self.finish_inner(record, true)
    }
    fn finish_inner(&self, record: &ExitRecord, systemd: bool) -> Result<bool, ExitError> {
        validate_identity(&record.attempt_id)?;
        let _lock = self.lock()?;
        if self.current_attempt()?.as_deref() != Some(&record.attempt_id) {
            if !systemd || self.retired(&record.attempt_id).exists() {
                return Ok(false);
            }
            self.advance(&record.attempt_id)?;
        }
        if self.read()?.is_some() {
            return Ok(false);
        }
        atomic_write(&self.path("exit"), &serde_json::to_vec(record)?)?;
        Ok(true)
    }
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), ExitError> {
    let temp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        File::open(path.parent().expect("run files have a parent"))?.sync_all()?;
        Ok::<_, io::Error>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    Ok(result?)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(attempt: &str) -> ExitRecord {
        ExitRecord {
            attempt_id: attempt.into(),
            code: Some(3),
            signal: None,
            ended_ms: 1,
            by: ExitSource::Wrapper,
            service_result: None,
        }
    }
    #[test]
    fn exit_record_new_attempt_retires_old_and_ignores_duplicate_or_stale_events() {
        let dir = tempfile::tempdir().expect("temp");
        let store = ExitStore::new(dir.path(), "run-1").expect("store");
        store.begin("first").expect("first");
        assert!(store.finish(&record("first")).expect("finish"));
        assert!(!store.finish(&record("first")).expect("duplicate"));
        store.begin("second").expect("second");
        assert!(store.read().expect("read").is_none());
        assert!(dir.path().join("run-1.first.exit").is_file());
        assert!(!store.finish(&record("first")).expect("stale"));
        assert!(store.read().expect("read").is_none());
        assert!(store.finish(&record("second")).expect("new"));
        assert_eq!(
            store.read().expect("read").expect("result").attempt_id,
            "second"
        );
        assert!(matches!(
            store.begin("second"),
            Err(ExitError::AlreadyStarted)
        ));
    }
}
