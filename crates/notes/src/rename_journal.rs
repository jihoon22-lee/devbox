//! The rename progress journal `<data>/notes-rename/<opId>.json`: what moves where,
//! which notes need link rewrites and which are done. Written atomically (temp +
//! fsync + rename + folder fsync) before every visible step; 0600 in a 0700 folder.
use crate::rename_edits::Move;
use devbox_documents::DiskRev;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;

const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePlan {
    /// Where the note is after the move (the moved note itself is listed at `to`).
    pub path: String,
    /// The bytes the plan was computed from; a different disk rev is not overwritten.
    pub base_rev: Option<DiskRev>,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub version: u32,
    pub op: String,
    #[serde(rename = "move")]
    pub mv: Move,
    pub moved: bool,
    pub files: Vec<FilePlan>,
}
impl Plan {
    pub fn new(op: String, mv: Move, files: Vec<FilePlan>) -> Self {
        Self {
            version: VERSION,
            op,
            mv,
            moved: false,
            files,
        }
    }
}

/// Operation ids are what `uuid::Uuid::simple` prints; anything else from a client
/// is refused before it can name a file.
pub fn valid_op_id(op: &str) -> bool {
    op.len() == 32 && op.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub struct Journals {
    dir: PathBuf,
}
impl Journals {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }
    fn file(&self, op: &str) -> PathBuf {
        self.dir.join(format!("{op}.json"))
    }
    pub fn write(&self, plan: &Plan) -> io::Result<()> {
        if !valid_op_id(&plan.op) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "rename op id"));
        }
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)?;
        let tmp = self
            .dir
            .join(format!(".{}.{}", plan.op, uuid::Uuid::new_v4().simple()));
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)?;
            file.write_all(&serde_json::to_vec(plan)?)?;
            file.sync_all()?;
            std::fs::rename(&tmp, self.file(&plan.op))?;
            std::fs::File::open(&self.dir)?.sync_all()
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }
    pub fn remove(&self, op: &str) -> io::Result<()> {
        match std::fs::remove_file(self.file(op)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => std::fs::File::open(&self.dir)
                .and_then(|d| d.sync_all())
                .or(Ok(())),
        }
    }
    /// Journals left by an earlier run, in name order, and the ids of journals that
    /// could not be read (`set_aside` them). Foreign file names are ignored.
    pub fn list(&self) -> (Vec<Plan>, Vec<String>) {
        let Ok(read) = std::fs::read_dir(&self.dir) else {
            return (Vec::new(), Vec::new());
        };
        let (mut plans, mut unreadable) = (Vec::new(), Vec::new());
        for entry in read.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(op) = name.strip_suffix(".json").filter(|op| valid_op_id(op)) else {
                continue;
            };
            let plan = std::fs::read(entry.path())
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Plan>(&bytes).ok())
                .filter(|plan| plan.op == op && plan.version == VERSION);
            match plan {
                Some(plan) => plans.push(plan),
                None => unreadable.push(op.to_owned()),
            }
        }
        plans.sort_by(|a, b| a.op.cmp(&b.op));
        unreadable.sort();
        (plans, unreadable)
    }
    /// Renames an unreadable journal to `<op>.json.corrupt` (kept, never deleted).
    pub fn set_aside(&self, op: &str) -> io::Result<()> {
        let aside = self.dir.join(format!("{op}.json.corrupt"));
        std::fs::rename(self.file(op), aside)?;
        std::fs::File::open(&self.dir)?.sync_all()
    }
}
