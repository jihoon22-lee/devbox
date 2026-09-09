//! Linux filesystem observations. Root inspection executes no Git, hook, server
//! or package manager; only the Windows owner can turn the report into Registry state.
use crate::files::{FileOwner, RootLease};
use crate::{ObjectStamp, Request, RootReport, MAX_ROOTS};
use devbox_filesystem::{ensure_no_links, project::ProjectObservation};
use product_contract::{ExecutionTarget, ProjectContext};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    os::fd::AsRawFd,
    os::unix::fs::MetadataExt,
    path::Path,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, &'static str>;
const ROOT_TTL: Duration = Duration::from_secs(180);
struct Root {
    observation: ProjectObservation,
    report: RootReport,
    touched: Instant,
    files: Option<FileAccess>,
}
struct FileAccess {
    context: ProjectContext,
    owner: FileOwner,
}
struct NativeFileLease<'a> {
    observation: &'a ProjectObservation,
    target: &'a ExecutionTarget,
}
impl RootLease for NativeFileLease<'_> {
    fn root(&self) -> &Path {
        self.observation.root()
    }
    fn target(&self) -> &ExecutionTarget {
        self.target
    }
    fn native_root_identity(&self) -> devbox_filesystem::FilesystemIdentity {
        self.observation.root_identity()
    }
    fn revalidate(&self) -> Result<()> {
        self.observation.revalidate()
    }
}
#[derive(Deserialize)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum FileMethod {
    #[serde(rename = "files_open")]
    Open {
        context: ProjectContext,
        request: code_pad_lib::commands::file::OpenFileRequest,
    },
    #[serde(rename = "files_save")]
    Save {
        context: ProjectContext,
        request: code_pad_lib::commands::file::SaveFileRequest,
        native_revision: String,
    },
    #[serde(rename = "files_close")]
    Close {
        context: ProjectContext,
        path: String,
    },
    #[serde(rename = "files_sync_editor")]
    SyncEditor {
        context: ProjectContext,
        path: String,
        native_revision: String,
        text: String,
    },
}
impl FileMethod {
    fn context(&self) -> &ProjectContext {
        match self {
            Self::Open { context, .. }
            | Self::Save { context, .. }
            | Self::Close { context, .. }
            | Self::SyncEditor { context, .. } => context,
        }
    }
}
#[derive(Default)]
pub struct Engine {
    roots: BTreeMap<String, Root>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathInput {
    path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
fn input<T: serde::de::DeserializeOwned>(args: &Value) -> Result<T> {
    serde_json::from_value(args.clone()).map_err(|_| "wsl_request_invalid")
}
fn short_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
/// Device numbers may change after a distro restart. Persist filesystem ID,
/// inode and birth time, while the live observation retains device/inode handles.
// Linux UAPI statx is a fixed 256-byte structure on every supported Linux
// architecture. Keep the unused fields opaque; libc does not expose this type
// for the repository's musl configuration. See statx(2), inode offset 32/btime 80.
#[repr(C)]
struct StatxTimestamp {
    sec: i64,
    nsec: u32,
    _reserved: i32,
}
#[repr(C)]
struct Statx {
    mask: u32,
    _block_size: u32,
    _header: [u64; 3],
    inode: u64,
    _body: [u64; 5],
    btime: StatxTimestamp,
    _tail: [u64; 20],
}
const _: () = assert!(std::mem::size_of::<Statx>() == 256);
const _: () = assert!(std::mem::offset_of!(Statx, inode) == 32);
const _: () = assert!(std::mem::offset_of!(Statx, btime) == 80);
fn stamp(handle: &File) -> Result<ObjectStamp> {
    let metadata = handle
        .metadata()
        .map_err(|_| "project_object_unavailable")?;
    // std::fs::Metadata::created is unavailable on Rust's musl target even
    // when the kernel supports birth time. Query the retained descriptor with
    // the fixed Linux statx ABI instead of reopening its pathname.
    let mut extended = std::mem::MaybeUninit::<Statx>::zeroed();
    let required: libc::c_uint = 0x0100 | 0x0800;
    let status = unsafe {
        libc::syscall(
            libc::SYS_statx,
            handle.as_raw_fd(),
            c"".as_ptr(),
            libc::AT_EMPTY_PATH | libc::AT_NO_AUTOMOUNT,
            required,
            extended.as_mut_ptr(),
        )
    };
    if status != 0 {
        return Err("wsl_identity_unavailable");
    }
    let extended = unsafe { extended.assume_init() };
    if extended.mask & required != required
        || extended.inode != metadata.ino()
        || extended.btime.nsec >= 1_000_000_000
    {
        return Err("wsl_identity_unavailable");
    }
    let mut filesystem = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // fstatfs writes this fixed native structure for the retained descriptor.
    if unsafe { libc::fstatfs(handle.as_raw_fd(), filesystem.as_mut_ptr()) } != 0 {
        return Err("wsl_identity_unavailable");
    }
    let filesystem = unsafe { filesystem.assume_init() };
    let fsid = unsafe {
        std::slice::from_raw_parts(
            (&filesystem.f_fsid as *const libc::fsid_t).cast::<u8>(),
            std::mem::size_of::<libc::fsid_t>(),
        )
    };
    if fsid.iter().all(|value| *value == 0) {
        return Err("wsl_identity_unavailable");
    }
    let mut scope = fsid.to_vec();
    scope.extend_from_slice(&filesystem.f_type.to_le_bytes());
    let mut object = metadata.ino().to_le_bytes().to_vec();
    object.extend_from_slice(&extended.btime.sec.to_le_bytes());
    object.extend_from_slice(&extended.btime.nsec.to_le_bytes());
    Ok(ObjectStamp {
        scope: short_digest(&scope),
        object: short_digest(&object),
    })
}
pub fn admit(path: &Path) -> Result<()> {
    let text = path.to_str().ok_or("invalid_root")?;
    if !text.starts_with('/')
        || text.len() > 32768
        || text.contains('\\')
        || text.chars().any(char::is_control)
        || text.split('/').any(|part| matches!(part, "." | ".."))
        || ["/proc", "/sys", "/dev"].iter().any(|root| {
            text == *root
                || text
                    .strip_prefix(root)
                    .is_some_and(|tail| tail.starts_with('/'))
        })
    {
        return Err("native_path_transport_denied");
    }
    Ok(())
}
impl Engine {
    fn file_request(&mut self, request: &Request, guard: &dyn Fn() -> Result<()>) -> Result<Value> {
        let method: FileMethod = input(&json!({"method":request.method,"args":request.args}))?;
        let root = self
            .roots
            .get_mut(request.root_token.as_ref().ok_or("wsl_root_required")?)
            .ok_or("wsl_root_expired")?;
        root.observation.revalidate()?;
        let access = root.files.as_mut().ok_or("wsl_context_required")?;
        if &access.context != method.context() {
            return Err("file_context_changed");
        }
        root.touched = Instant::now();
        let lease = NativeFileLease {
            observation: &root.observation,
            target: &access.context.target,
        };
        match method {
            FileMethod::Open { request, .. } => {
                let opened = access
                    .owner
                    .open(Some((&access.context, &lease)), request)?;
                let revision = access.owner.document_revision(&opened.path)?;
                let mut value = serde_json::to_value(opened).map_err(|_| "wsl_response_invalid")?;
                value["nativeRevision"] = json!(revision);
                Ok(value)
            }
            FileMethod::Save {
                request,
                native_revision,
                ..
            } => {
                if access.owner.document_revision(&request.path)? != native_revision {
                    return Err("file_snapshot_changed");
                }
                let saved =
                    access
                        .owner
                        .save_guarded(Some((&access.context, &lease)), request, guard)?;
                let revision = access.owner.document_revision(&saved.path).ok();
                let mut value = serde_json::to_value(saved).map_err(|_| "wsl_response_invalid")?;
                value["nativeRevision"] = json!(revision);
                Ok(value)
            }
            FileMethod::Close { path, .. } => {
                access.owner.close(&path)?;
                Ok(Value::Null)
            }
            FileMethod::SyncEditor {
                path,
                native_revision,
                text,
                ..
            } => access
                .owner
                .sync_editor_document(Some(&access.context), &path, &native_revision, &text)
                .map(|dirty| json!({"dirty":dirty})),
        }
    }
    pub fn dispatch(&mut self, request: &Request) -> Result<Value> {
        self.dispatch_guarded(request, &|| Ok(()))
    }
    pub fn dispatch_guarded(
        &mut self,
        request: &Request,
        guard: &dyn Fn() -> Result<()>,
    ) -> Result<Value> {
        guard()?;
        self.roots
            .retain(|_, root| root.touched.elapsed() < ROOT_TTL);
        if matches!(
            request.method.as_str(),
            "files_open" | "files_save" | "files_close" | "files_sync_editor"
        ) {
            return self.file_request(request, guard);
        }
        match request.method.as_str() {
            "files_attach" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Attach {
                    context: ProjectContext,
                }
                let args: Attach = input(&request.args)?;
                args.context.validate().map_err(|_| "wsl_context_invalid")?;
                if !matches!(&args.context.target, ExecutionTarget::Wsl {distro_id} if crate::token(distro_id))
                {
                    return Err("wsl_context_invalid");
                }
                let root = self
                    .roots
                    .get_mut(request.root_token.as_ref().ok_or("wsl_root_required")?)
                    .ok_or("wsl_root_expired")?;
                if let Some(access) = &root.files {
                    return if access.context == args.context {
                        root.observation.revalidate()?;
                        Ok(Value::Null)
                    } else {
                        Err("file_context_changed")
                    };
                }
                root.observation.revalidate()?;
                let guarded = ProjectObservation::capture(
                    root.observation.root(),
                    crate::linux_files::admit,
                )?;
                if guarded.root_identity() != root.observation.root_identity()
                    || guarded.repository_identity() != root.observation.repository_identity()
                {
                    return Err("project_object_changed");
                }
                root.observation.revalidate()?;
                root.observation = guarded;
                root.files = Some(FileAccess {
                    context: args.context,
                    owner: FileOwner::with_admission(crate::linux_files::admit),
                });
                root.touched = Instant::now();
                Ok(Value::Null)
            }
            "hello" => {
                let _: Empty = input(&request.args)?;
                if request.root_token.is_some() {
                    return Err("wsl_request_invalid");
                }
                Ok(
                    json!({"version":crate::VERSION,"pid":std::process::id(),"uid":unsafe {libc::geteuid()},"architecture":std::env::consts::ARCH}),
                )
            }
            "observe_root" => {
                if request.root_token.is_some() {
                    return Err("wsl_request_invalid");
                }
                let args: PathInput = input(&request.args)?;
                if args.path == "/" || self.roots.len() >= MAX_ROOTS {
                    return Err("wsl_root_limit");
                }
                let requested = Path::new(&args.path);
                admit(requested)?;
                ensure_no_links(requested).map_err(|_| "unsafe_project_object")?;
                let canonical = requested
                    .canonicalize()
                    .map_err(|_| "project_object_unavailable")?;
                admit(&canonical)?;
                let observation = ProjectObservation::capture(&canonical, admit)?;
                let token = uuid::Uuid::new_v4().to_string();
                let directories = observation.git_directories();
                let report = RootReport {
                    token: token.clone(),
                    root: canonical.to_str().ok_or("invalid_root")?.into(),
                    root_object: stamp(observation.root_handle())?,
                    repository_object: observation.repository_handle().map(stamp).transpose()?,
                    git_directory: directories
                        .map(|(git, _)| git.to_str().ok_or("invalid_root"))
                        .transpose()?
                        .map(str::to_owned),
                    common_directory: directories
                        .map(|(_, common)| common.to_str().ok_or("invalid_root"))
                        .transpose()?
                        .map(str::to_owned),
                };
                observation.revalidate()?;
                ensure_no_links(requested).map_err(|_| "unsafe_project_object")?;
                if requested
                    .canonicalize()
                    .map_err(|_| "project_object_changed")?
                    != canonical
                {
                    return Err("project_object_changed");
                }
                let value = serde_json::to_value(&report).map_err(|_| "wsl_response_invalid")?;
                self.roots.insert(
                    token,
                    Root {
                        observation,
                        report,
                        touched: Instant::now(),
                        files: None,
                    },
                );
                Ok(value)
            }
            "validate_root" => {
                let _: Empty = input(&request.args)?;
                let root = self
                    .roots
                    .get_mut(request.root_token.as_ref().ok_or("wsl_root_required")?)
                    .ok_or("wsl_root_expired")?;
                root.observation.revalidate()?;
                root.touched = Instant::now();
                serde_json::to_value(&root.report).map_err(|_| "wsl_response_invalid")
            }
            "release_root" => {
                let _: Empty = input(&request.args)?;
                self.roots
                    .remove(request.root_token.as_ref().ok_or("wsl_root_required")?);
                Ok(Value::Null)
            }
            _ => Err("wsl_request_invalid"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(method: &str, root: Option<String>, args: Value) -> Request {
        Request {
            version: crate::VERSION,
            session_id: uuid::Uuid::new_v4().to_string(),
            request_id: uuid::Uuid::new_v4().to_string(),
            sequence: 1,
            budget_ms: 5000,
            method: method.into(),
            root_token: root,
            args,
        }
    }
    #[test]
    fn file_reads_require_native_root_context_and_reject_scope_changes() {
        let directory = tempfile::Builder::new()
            .prefix(".wsl-engine-fixture-")
            .tempdir_in(env!("CARGO_MANIFEST_DIR"))
            .unwrap();
        let root = directory.path().join("한글 project");
        std::fs::create_dir(&root).unwrap();
        let file = root.join("한글.txt");
        std::fs::write(&file, b"original\r\n").unwrap();
        let outside = directory.path().join("outside.txt");
        std::fs::write(&outside, b"outside").unwrap();
        let mut engine = Engine::default();
        let report: RootReport = serde_json::from_value(
            engine
                .dispatch(&request("observe_root", None, json!({"path":root})))
                .unwrap(),
        )
        .unwrap();
        let context = ProjectContext {
            project_id: uuid::Uuid::new_v4().to_string(),
            worktree_id: uuid::Uuid::new_v4().to_string(),
            revision: 1,
            target: ExecutionTarget::Wsl {
                distro_id: uuid::Uuid::new_v4().to_string(),
            },
        };
        let open = request(
            "files_open",
            Some(report.token.clone()),
            json!({"context":context,"request":{"path":file,"encoding":null}}),
        );
        assert_eq!(engine.dispatch(&open), Err("wsl_context_required"));
        let attach = request(
            "files_attach",
            Some(report.token.clone()),
            json!({"context":context}),
        );
        engine.dispatch(&attach).unwrap();
        engine.dispatch(&attach).unwrap();
        let opened = engine.dispatch(&open).unwrap();
        assert_eq!(opened["text"], "original\n");
        assert!(crate::token(opened["nativeRevision"].as_str().unwrap()));
        let mut foreign = context.clone();
        foreign.revision += 1;
        assert_eq!(
            engine.dispatch(&request(
                "files_attach",
                Some(report.token.clone()),
                json!({"context":foreign})
            )),
            Err("file_context_changed")
        );
        assert_eq!(
            engine.dispatch(&request(
                "files_open",
                Some(report.token.clone()),
                json!({"context":foreign,"request":{"path":file,"encoding":null}})
            )),
            Err("file_context_changed")
        );
        assert_eq!(
            engine.dispatch(&request(
                "files_open",
                Some(report.token.clone()),
                json!({"context":context,"request":{"path":outside,"encoding":null}})
            )),
            Err("file_context_changed")
        );
        let sync = |revision: &str| {
            request(
                "files_sync_editor",
                Some(report.token.clone()),
                json!({"context":context,"path":opened["path"],"nativeRevision":revision,"text":"unsaved"}),
            )
        };
        assert_eq!(
            engine.dispatch(&sync("stale")),
            Err("file_snapshot_changed")
        );
        assert_eq!(
            engine
                .dispatch(&sync(opened["nativeRevision"].as_str().unwrap()))
                .unwrap()["dirty"],
            true
        );
        assert_eq!(std::fs::read(&file).unwrap(), b"original\r\n");
        let save_args = json!({"context":context,"nativeRevision":opened["nativeRevision"],"request":{
            "path":opened["path"],"text":"saved\n","encoding":opened["encoding"],"lineEnding":opened["lineEnding"],
            "expectedMtimeNanos":opened["mtimeNanos"],"expectedSize":opened["size"],"expectedContentHash":opened["contentHash"],"sourceLossy":false
        }});
        let save = request("files_save", Some(report.token.clone()), save_args);
        let staged = std::cell::Cell::new(false);
        let cancel = || {
            if std::fs::read_dir(&root).unwrap().count() > 1 {
                staged.set(true);
                Err("wsl_request_cancelled")
            } else {
                Ok(())
            }
        };
        assert_eq!(
            engine.dispatch_guarded(&save, &cancel),
            Err("wsl_request_cancelled")
        );
        assert!(staged.get());
        assert_eq!(std::fs::read(&file).unwrap(), b"original\r\n");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        let saved = engine.dispatch(&save).unwrap();
        assert_ne!(saved["nativeRevision"], opened["nativeRevision"]);
        assert_eq!(std::fs::read(&file).unwrap(), b"saved\r\n");
        assert_eq!(engine.dispatch(&save), Err("file_snapshot_changed"));
        let close = request(
            "files_close",
            Some(report.token.clone()),
            json!({"context":context,"path":opened["path"]}),
        );
        engine.dispatch(&close).unwrap();
        assert_eq!(
            engine.dispatch(&sync(opened["nativeRevision"].as_str().unwrap())),
            Err("file_selection_required")
        );
        engine
            .dispatch(&request(
                "release_root",
                Some(report.token.clone()),
                json!({}),
            ))
            .unwrap();
        assert_eq!(engine.dispatch(&open), Err("wsl_root_expired"));
        assert_eq!(std::fs::read(&outside).unwrap(), b"outside");
    }
    #[test]
    fn root_ids_survive_helper_restart_and_reject_replacement_links_and_new_git() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("한글 project");
        std::fs::create_dir(&root).unwrap();
        let mut engine = Engine::default();
        let observe = request("observe_root", None, json!({"path":root}));
        let first: RootReport = serde_json::from_value(engine.dispatch(&observe).unwrap()).unwrap();
        let second: RootReport =
            serde_json::from_value(Engine::default().dispatch(&observe).unwrap()).unwrap();
        assert_ne!(first.token, second.token);
        assert_eq!(first.root_object, second.root_object);
        std::fs::create_dir(root.join(".git")).unwrap();
        assert_eq!(
            engine.dispatch(&request("validate_root", Some(first.token), json!({}))),
            Err("project_object_changed")
        );
        assert!(engine.dispatch(&observe).is_err()); // malformed .git is never a plain folder
        let moved = directory.path().join("previous");
        std::fs::rename(&root, &moved).unwrap();
        std::fs::create_dir(&root).unwrap();
        let replaced: RootReport =
            serde_json::from_value(engine.dispatch(&observe).unwrap()).unwrap();
        assert_ne!(replaced.root_object, second.root_object);
        let link = directory.path().join("link");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        assert!(engine
            .dispatch(&request("observe_root", None, json!({"path":link})))
            .is_err());
        for unsafe_path in [
            "/proc/self/fd",
            "/dev/null",
            "/tmp/../etc",
            "relative",
            "C:\\root",
        ] {
            assert!(engine
                .dispatch(&request("observe_root", None, json!({"path":unsafe_path})))
                .is_err());
        }
    }
    #[test]
    fn expiry_release_and_foreign_tokens_do_not_grant_root_access() {
        let directory = tempfile::tempdir().unwrap();
        let mut engine = Engine::default();
        let report: RootReport = serde_json::from_value(
            engine
                .dispatch(&request(
                    "observe_root",
                    None,
                    json!({"path":directory.path()}),
                ))
                .unwrap(),
        )
        .unwrap();
        assert!(engine
            .dispatch(&request(
                "validate_root",
                Some(uuid::Uuid::new_v4().to_string()),
                json!({})
            ))
            .is_err());
        engine.roots.get_mut(&report.token).unwrap().touched = Instant::now() - ROOT_TTL;
        assert_eq!(
            engine.dispatch(&request(
                "validate_root",
                Some(report.token.clone()),
                json!({})
            )),
            Err("wsl_root_expired")
        );
        engine
            .dispatch(&request("release_root", Some(report.token), json!({})))
            .unwrap();
        assert!(engine.roots.is_empty());
    }
}
