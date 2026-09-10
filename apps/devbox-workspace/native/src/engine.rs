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
pub type SourceAuthorization = std::sync::Arc<dyn Fn(&str) -> Result<()> + Send + Sync>;
const ROOT_TTL: Duration = Duration::from_secs(180);
struct Root {
    observation: ProjectObservation,
    report: RootReport,
    touched: Instant,
    files: Option<FileAccess>,
    definitions: Option<crate::definitions::Definitions>,
    source: Option<crate::git_review::Review>,
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
    #[serde(rename = "files_poll")]
    Poll {
        context: ProjectContext,
        paths: Vec<String>,
    },
    #[serde(rename = "files_recover")]
    Recover {
        context: ProjectContext,
        path: String,
        content: String,
        native_revision: String,
    },
    #[serde(rename = "files_list")]
    List {
        context: ProjectContext,
        path: String,
    },
    #[serde(rename = "files_preview")]
    Preview {
        context: ProjectContext,
        path: String,
        content: String,
        workspace_root: String,
    },
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
    #[serde(rename = "files_rename")]
    Rename {
        context: ProjectContext,
        request: code_pad_lib::commands::file::RenameFileRequest,
        native_revision: String,
    },
    #[serde(rename = "files_delete")]
    Delete {
        context: ProjectContext,
        request: code_pad_lib::commands::file::FileActionRequest,
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
            Self::Poll { context, .. }
            | Self::Recover { context, .. }
            | Self::List { context, .. }
            | Self::Preview { context, .. }
            | Self::Open { context, .. }
            | Self::Save { context, .. }
            | Self::Rename { context, .. }
            | Self::Delete { context, .. }
            | Self::Close { context, .. }
            | Self::SyncEditor { context, .. } => context,
        }
    }
}
#[derive(Default)]
pub struct Engine {
    source_environment: crate::git_environment::SourceEnvironment,
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
fn persistent_object(
    inode: u64,
    wslfs: bool,
    extended: std::result::Result<&Statx, i32>,
) -> Result<Vec<u8>> {
    match extended {
        // WSL1's native wslfs exposes the NT file ID (including its sequence)
        // as st_ino, but does not implement statx. The Windows owner separately
        // binds this filesystem to the retained distro/backing-directory IDs.
        Err(libc::ENOSYS) if wslfs && inode >> 48 != 0 => {
            let mut object = b"wslfs-file-id-v1\0".to_vec();
            object.extend_from_slice(&inode.to_le_bytes());
            Ok(object)
        }
        Ok(extended)
            if extended.mask & 0x0900 == 0x0900
                && extended.inode == inode
                && extended.btime.nsec < 1_000_000_000 =>
        {
            let mut object = inode.to_le_bytes().to_vec();
            object.extend_from_slice(&extended.btime.sec.to_le_bytes());
            object.extend_from_slice(&extended.btime.nsec.to_le_bytes());
            Ok(object)
        }
        _ => Err("wsl_identity_unavailable"),
    }
}
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
    let extended = if status == 0 {
        Ok(unsafe { extended.assume_init() })
    } else {
        Err(std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EIO))
    };
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
    let object = persistent_object(
        metadata.ino(),
        filesystem.f_type == 0x5346_4846,
        extended.as_ref().map_err(|error| *error),
    )?;
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
    /// Only the binary's process/pipe owner supplies these native capabilities.
    pub fn execute_source<T: Send + Sync + 'static>(
        &mut self,
        request: &Request,
        guard: &dyn Fn() -> Result<()>,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
        authorize: SourceAuthorization,
        retained: T,
    ) -> Result<Value> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Execute {
            context: ProjectContext,
            digest: String,
            method: String,
            args: Value,
            #[serde(default)]
            members: Vec<crate::git_review::CleanupMember>,
        }
        if request.method != "source_execute" {
            return Err("wsl_request_invalid");
        }
        guard()?;
        let args: Execute = input(&request.args)?;
        let root = self
            .roots
            .get_mut(request.root_token.as_ref().ok_or("wsl_root_required")?)
            .ok_or("wsl_root_expired")?;
        let source = root.source.as_mut().ok_or("wsl_context_required")?;
        if source.context() != &args.context {
            return Err("source_context_changed");
        }
        root.observation.revalidate()?;
        let value = source.execute(crate::git_review::Execution {
            expected: &args.digest,
            method: &args.method,
            args: args.args,
            budget_ms: request.budget_ms,
            cancelled,
            authorize,
            retained,
            members: args.members,
            source: &self.source_environment,
        })?;
        guard()?;
        root.observation.revalidate()?;
        root.touched = Instant::now();
        Ok(value)
    }
    fn dependency_inventory(
        &mut self,
        request: &Request,
        guard: &dyn Fn() -> Result<()>,
    ) -> Result<Value> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Inventory {
            context: ProjectContext,
            budget_ms: u32,
        }
        let args: Inventory = input(&request.args)?;
        if !(1..=10_000).contains(&args.budget_ms) {
            return Err("invalid_request");
        }
        let root = self
            .roots
            .get_mut(request.root_token.as_ref().ok_or("wsl_root_required")?)
            .ok_or("wsl_root_expired")?;
        if root.files.as_ref().map(|files| &files.context) != Some(&args.context) {
            return Err("dependency_context_changed");
        }
        guard()?;
        root.observation.revalidate()?;
        let budget = Duration::from_millis(u64::from(request.budget_ms));
        let expires = Instant::now() + budget;
        let report = repo_manager_lib::component::native_dependency_inventory(
            root.observation.root(),
            budget.min(Duration::from_millis(u64::from(args.budget_ms))),
            &|path| {
                guard().map_err(str::to_owned)?;
                if Instant::now() >= expires {
                    return Err("request_expired".into());
                }
                crate::linux_files::admit(path).map_err(str::to_owned)
            },
        )
        .map_err(|error| {
            if error == "request_expired" {
                "request_expired"
            } else {
                "dependency_operation_failed"
            }
        })?;
        guard()?;
        if Instant::now() >= expires {
            return Err("request_expired");
        }
        root.observation.revalidate()?;
        root.touched = Instant::now();
        Ok(report)
    }
    fn source_request(
        &mut self,
        request: &Request,
        guard: &dyn Fn() -> Result<()>,
    ) -> Result<Value> {
        use crate::git_review::{Method, Review};
        let method: Method = input(&json!({"method":request.method,"args":request.args}))?;
        method
            .context()
            .validate()
            .map_err(|_| "wsl_context_invalid")?;
        if !matches!(&method.context().target, ExecutionTarget::Wsl { distro_id } if crate::token(distro_id))
        {
            return Err("wsl_context_invalid");
        }
        let count = self
            .roots
            .values()
            .filter(|root| root.source.is_some())
            .count();
        let root = self
            .roots
            .get_mut(request.root_token.as_ref().ok_or("wsl_root_required")?)
            .ok_or("wsl_root_expired")?;
        if root
            .files
            .as_ref()
            .is_some_and(|access| &access.context != method.context())
            || root
                .definitions
                .as_ref()
                .is_some_and(|access| access.context() != method.context())
            || root
                .source
                .as_ref()
                .is_some_and(|access| access.context() != method.context())
        {
            return Err("source_context_changed");
        }
        root.observation.revalidate()?;
        let deadline = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "request_expired")?
            .as_millis()
            .saturating_add(u128::from(request.budget_ms))
            .min(u128::from(u64::MAX)) as u64;
        let result = match method {
            Method::Capture { context } => {
                // Each pipe retains at most four 48-MiB Git snapshots. A second
                // attach never replaces reviewed sources or clears a conflict.
                if root.source.is_none() {
                    if count >= 4 {
                        return Err("git_source_limit");
                    }
                    root.source = Some(Review::capture(
                        &root.observation,
                        context,
                        &self.source_environment,
                        deadline,
                        guard,
                    )?);
                }
                let access = root.source.as_ref().ok_or("wsl_context_required")?;
                let view = access.view();
                access.revalidate(
                    view["digest"].as_str().ok_or("wsl_response_invalid")?,
                    deadline,
                )?;
                view
            }
            Method::Worktree {
                digest,
                branch,
                target_dir,
                ..
            } => root
                .source
                .as_mut()
                .ok_or("wsl_context_required")?
                .preview_worktree(&digest, branch, &target_dir, deadline)?,
            Method::Validate { digest, .. } => {
                root.source
                    .as_ref()
                    .ok_or("wsl_context_required")?
                    .revalidate(&digest, deadline)?;
                Value::Null
            }
        };
        guard()?;
        root.observation.revalidate()?;
        root.touched = Instant::now();
        Ok(result)
    }
    fn definition_request(
        &mut self,
        request: &Request,
        guard: &dyn Fn() -> Result<()>,
    ) -> Result<Value> {
        use crate::definitions::{Definitions, Method};
        let method: Method = input(&json!({"method":request.method,"args":request.args}))?;
        method
            .context()
            .validate()
            .map_err(|_| "wsl_context_invalid")?;
        if !matches!(&method.context().target, ExecutionTarget::Wsl {distro_id} if crate::token(distro_id))
        {
            return Err("wsl_context_invalid");
        }
        let root = self
            .roots
            .get_mut(request.root_token.as_ref().ok_or("wsl_root_required")?)
            .ok_or("wsl_root_expired")?;
        root.observation.revalidate()?;
        if root
            .files
            .as_ref()
            .is_some_and(|access| &access.context != method.context())
            || root
                .definitions
                .as_ref()
                .is_some_and(|access| access.context() != method.context())
            || root
                .source
                .as_ref()
                .is_some_and(|access| access.context() != method.context())
        {
            return Err("file_context_changed");
        }
        let result = match method {
            Method::Attach { context } => {
                if let Some(access) = &root.definitions {
                    // Reattaching never replaces reviewed bytes or clears a conflict.
                    access.revalidate(guard)?;
                } else {
                    root.definitions =
                        Some(Definitions::capture(&root.observation, context, guard)?);
                }
                Value::Null
            }
            Method::Read { path, optional, .. } => {
                let access = root.definitions.as_mut().ok_or("wsl_context_required")?;
                serde_json::to_value(access.read(&path, optional, guard)?)
                    .map_err(|_| "wsl_response_invalid")?
            }
            Method::Write { content, .. } => {
                let warning = root
                    .definitions
                    .as_mut()
                    .ok_or("wsl_context_required")?
                    .write(&content, guard)?;
                json!({"warning": warning})
            }
            Method::Validate { .. } => {
                root.definitions
                    .as_ref()
                    .ok_or("wsl_context_required")?
                    .revalidate(guard)?;
                Value::Null
            }
        };
        guard()?;
        root.observation.revalidate()?;
        root.touched = Instant::now();
        Ok(result)
    }
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
            FileMethod::Poll { paths, .. } => {
                if paths.len() > 64 || paths.iter().any(|path| path.len() > 32768) {
                    return Err("file_limit");
                }
                access
                    .owner
                    .validate_open_paths(Some(&access.context), &paths)?;
                let mut snapshots = Vec::new();
                for path in paths {
                    guard()?;
                    // Like the standalone watcher, a missing/unreadable leaf
                    // cannot publish content evidence. Root loss still fails
                    // the entire request at the final lease check.
                    if let Ok(snapshot) = access
                        .owner
                        .watch_snapshot(Some((&access.context, &lease)), &path)
                    {
                        snapshots.push(snapshot);
                    }
                }
                guard()?;
                lease.revalidate()?;
                serde_json::to_value(snapshots).map_err(|_| "wsl_response_invalid")
            }
            FileMethod::Recover {
                path,
                content,
                native_revision,
                ..
            } => {
                let saved = access.owner.apply_recovery_guarded(
                    Some((&access.context, &lease)),
                    &path,
                    &content,
                    &native_revision,
                    guard,
                )?;
                let revision = access.owner.document_revision(&saved.path).ok();
                let mut result = serde_json::to_value(saved).map_err(|_| "wsl_response_invalid")?;
                result["nativeRevision"] = json!(revision);
                Ok(result)
            }
            FileMethod::Preview {
                path,
                content,
                workspace_root,
                ..
            } => {
                if Path::new(&workspace_root) != lease.root() {
                    return Err("file_context_changed");
                }
                access
                    .owner
                    .admitted_path(Some((&access.context, &lease)), &path)?;
                let result = code_pad_lib::commands::preview::render_preview_guarded(
                    &path,
                    &content,
                    &workspace_root,
                    &|path| access.owner.ensure_user_path(path).map_err(str::to_string),
                    &|| guard().map_err(str::to_string),
                )
                .map_err(|_| "file_preview_unavailable")?;
                access
                    .owner
                    .admitted_path(Some((&access.context, &lease)), &path)?;
                lease.revalidate()?;
                serde_json::to_value(result).map_err(|_| "wsl_response_invalid")
            }
            FileMethod::List { path, .. } => {
                if Path::new(&path) != lease.root() {
                    return Err("file_context_changed");
                }
                let result = code_pad_lib::commands::folder::list_workspace_files_guarded(
                    lease.root(),
                    &|path| access.owner.ensure_user_path(path).map_err(str::to_string),
                    &|| guard().map_err(str::to_string),
                )
                .map_err(|_| "file_listing_unavailable")?;
                lease.revalidate()?;
                serde_json::to_value(result).map_err(|_| "wsl_response_invalid")
            }
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
            FileMethod::Rename {
                request,
                native_revision,
                ..
            } => {
                if access.owner.document_revision(&request.file.path)? != native_revision {
                    return Err("file_snapshot_changed");
                }
                let renamed =
                    access
                        .owner
                        .rename_guarded(Some((&access.context, &lease)), request, guard)?;
                let revision = access.owner.document_revision(&renamed.path).ok();
                let mut value =
                    serde_json::to_value(renamed).map_err(|_| "wsl_response_invalid")?;
                value["nativeRevision"] = json!(revision);
                Ok(value)
            }
            FileMethod::Delete {
                request,
                native_revision,
                ..
            } => {
                if access.owner.document_revision(&request.path)? != native_revision {
                    return Err("file_snapshot_changed");
                }
                access
                    .owner
                    .delete_guarded(Some((&access.context, &lease)), request, guard)?;
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
            // Attached native owners retain their bounded roots until release
            // or EOF. The Windows preview owner enforces its separate TTL;
            // idle editor/execution evidence must not lose its root by itself.
            .retain(|_, root| {
                root.files.is_some()
                    || root.definitions.is_some()
                    || root.source.is_some()
                    || root.touched.elapsed() < ROOT_TTL
            });
        if matches!(
            request.method.as_str(),
            "files_poll"
                | "files_recover"
                | "files_list"
                | "files_preview"
                | "files_open"
                | "files_save"
                | "files_rename"
                | "files_delete"
                | "files_close"
                | "files_sync_editor"
        ) {
            return self.file_request(request, guard);
        }
        if matches!(
            request.method.as_str(),
            "definitions_attach"
                | "definitions_read"
                | "definitions_validate"
                | "definitions_write"
        ) {
            return self.definition_request(request, guard);
        }
        if matches!(
            request.method.as_str(),
            "source_capture" | "source_validate" | "source_worktree_preview"
        ) {
            return self.source_request(request, guard);
        }
        if request.method == "dependency_inventory" {
            return self.dependency_inventory(request, guard);
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
                if root
                    .definitions
                    .as_ref()
                    .is_some_and(|access| access.context() != &args.context)
                {
                    return Err("file_context_changed");
                }
                if root
                    .source
                    .as_ref()
                    .is_some_and(|access| access.context() != &args.context)
                {
                    return Err("file_context_changed");
                }
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
                        definitions: None,
                        source: None,
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
    #[test]
    fn source_review_is_context_bound_and_never_executes_the_discovered_tool() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = tempfile::Builder::new()
            .prefix(".wsl-source-fixture-")
            .tempdir_in(env!("CARGO_MANIFEST_DIR"))
            .unwrap();
        let root = fixture.path();
        std::fs::create_dir_all(root.join(".git/objects")).unwrap();
        std::fs::create_dir(root.join(".git/hooks")).unwrap();
        std::fs::create_dir(root.join("bin")).unwrap();
        std::fs::write(root.join(".git/HEAD"), b"ref: refs/heads/main\n").unwrap();
        std::fs::write(
            root.join(".git/config"),
            b"[core]\nrepositoryformatversion=0\n",
        )
        .unwrap();
        std::fs::write(root.join(".git/hooks/pre-commit"), b"not executed").unwrap();
        let program = root.join("bin/git");
        let marker = root.join("must-not-exist");
        std::fs::write(
            &program,
            format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let source_environment = crate::git_environment::SourceEnvironment::from_values(vec![
            ("HOME".into(), root.into()),
            ("PATH".into(), root.join("bin").into_os_string()),
        ]);
        let mut engine = Engine {
            source_environment,
            ..Engine::default()
        };
        let report = engine
            .dispatch(&request("observe_root", None, json!({"path":root})))
            .unwrap();
        let token = report["token"].as_str().unwrap().to_owned();
        let context = ProjectContext {
            project_id: "fixture-project".into(),
            worktree_id: "fixture-worktree".into(),
            revision: 1,
            target: ExecutionTarget::Wsl {
                distro_id: uuid::Uuid::new_v4().to_string(),
            },
        };
        let capture = request(
            "source_capture",
            Some(token.clone()),
            json!({"context":context}),
        );
        let view = engine.dispatch(&capture).unwrap();
        assert_eq!(
            view["review"]["executable"],
            program.to_string_lossy().as_ref()
        );
        assert!(!marker.exists());
        let validate = request(
            "source_validate",
            Some(token.clone()),
            json!({"context":context,"digest":view["digest"]}),
        );
        engine.dispatch(&validate).unwrap();
        let mut foreign = context.clone();
        foreign.revision += 1;
        assert_eq!(
            engine
                .dispatch(&request(
                    "source_capture",
                    Some(token.clone()),
                    json!({"context":foreign})
                ))
                .unwrap_err(),
            "source_context_changed"
        );
        assert!(engine
            .dispatch(&request(
                "definitions_attach",
                Some(token.clone()),
                json!({"context":foreign})
            ))
            .is_err());
        assert!(engine
            .dispatch(&request(
                "files_attach",
                Some(token.clone()),
                json!({"context":foreign})
            ))
            .is_err());
        assert_eq!(
            engine
                .dispatch(&request(
                    "source_execute",
                    Some(token),
                    json!({"context":context})
                ))
                .unwrap_err(),
            "wsl_request_invalid"
        );
        std::fs::write(
            root.join(".git/hooks/pre-commit"),
            b"changed without execution",
        )
        .unwrap();
        assert_eq!(
            engine.dispatch(&validate).unwrap_err(),
            "git_sources_changed"
        );
        assert_eq!(
            engine.dispatch(&capture).unwrap_err(),
            "git_sources_changed"
        );
        assert!(!marker.exists());
    }
    #[test]
    fn wsl1_native_file_ids_do_not_enable_a_generic_birth_time_fallback() {
        let inode = 0x0005_0000_0000_0042;
        let first = persistent_object(inode, true, Err(libc::ENOSYS)).unwrap();
        assert_eq!(
            first,
            persistent_object(inode, true, Err(libc::ENOSYS)).unwrap()
        );
        assert_ne!(
            first,
            persistent_object(inode + (1 << 48), true, Err(libc::ENOSYS)).unwrap()
        );
        assert!(persistent_object(inode, false, Err(libc::ENOSYS)).is_err());
        assert!(persistent_object(inode, true, Err(libc::EIO)).is_err());
        assert!(persistent_object(42, true, Err(libc::ENOSYS)).is_err());
        let mut extended = unsafe { std::mem::MaybeUninit::<Statx>::zeroed().assume_init() };
        extended.inode = inode;
        extended.mask = 0x0100;
        assert!(persistent_object(inode, false, Ok(&extended)).is_err());
        extended.mask = 0x0900;
        assert!(persistent_object(inode, false, Ok(&extended)).is_ok());
        extended.inode += 1;
        assert!(persistent_object(inode, false, Ok(&extended)).is_err());
    }
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
    fn file_operations_require_native_root_context_and_reject_scope_changes() {
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
        let listed = engine
            .dispatch(&request(
                "files_list",
                Some(report.token.clone()),
                json!({
                    "context":context,"path":root
                }),
            ))
            .unwrap();
        assert_eq!(listed["files"].as_array().unwrap().len(), 1);
        assert_eq!(listed["files"][0]["path"], opened["path"]);
        assert_eq!(
            engine.dispatch(&request(
                "files_list",
                Some(report.token.clone()),
                json!({
                    "context":context,"path":outside
                })
            )),
            Err("file_context_changed")
        );
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
        engine.roots.get_mut(&report.token).unwrap().touched = Instant::now() - ROOT_TTL;
        // Attached editor grants survive idle time; preview leases still expire.
        let current = engine.dispatch(&open).unwrap();
        assert_eq!(current["nativeRevision"], saved["nativeRevision"]);
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
        let current = engine.dispatch(&open).unwrap();
        let file_action = |opened: &Value| {
            json!({
                "path":opened["path"],"expectedMtimeNanos":opened["mtimeNanos"],
                "expectedSize":opened["size"],"expectedContentHash":opened["contentHash"]
            })
        };
        let mut rename_args = file_action(&current);
        rename_args["newName"] = json!("renamed 한글.txt");
        let rename = request(
            "files_rename",
            Some(report.token.clone()),
            json!({
                "context":context,"nativeRevision":current["nativeRevision"],"request":rename_args
            }),
        );
        let renamed = engine.dispatch(&rename).unwrap();
        assert!(!file.exists());
        assert_ne!(renamed["nativeRevision"], current["nativeRevision"]);
        assert_eq!(
            std::fs::read(root.join("renamed 한글.txt")).unwrap(),
            b"saved\r\n"
        );
        assert_eq!(engine.dispatch(&rename), Err("file_selection_required"));
        let mut delete = request(
            "files_delete",
            Some(report.token.clone()),
            json!({
                "context":context,"nativeRevision":current["nativeRevision"],"request":file_action(&renamed)
            }),
        );
        assert_eq!(engine.dispatch(&delete), Err("file_snapshot_changed"));
        assert!(root.join("renamed 한글.txt").exists());
        delete.args["nativeRevision"] = renamed["nativeRevision"].clone();
        engine.dispatch(&delete).unwrap();
        assert!(!root.join("renamed 한글.txt").exists());
        assert_eq!(engine.dispatch(&delete), Err("file_selection_required"));
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

#[cfg(test)]
mod definition_tests {
    use super::*;
    use std::fs;
    fn request(method: &str, token: Option<&str>, args: Value) -> Request {
        Request {
            version: crate::VERSION,
            session_id: uuid::Uuid::new_v4().to_string(),
            request_id: uuid::Uuid::new_v4().to_string(),
            sequence: 1,
            budget_ms: 5000,
            method: method.into(),
            root_token: token.map(str::to_owned),
            args,
        }
    }
    fn context() -> ProjectContext {
        ProjectContext {
            project_id: "project".into(),
            worktree_id: "tree".into(),
            revision: 1,
            target: ExecutionTarget::Wsl {
                distro_id: uuid::Uuid::new_v4().to_string(),
            },
        }
    }
    fn root(engine: &mut Engine, path: &Path) -> String {
        engine
            .dispatch(&request("observe_root", None, json!({"path":path})))
            .unwrap()["token"]
            .as_str()
            .unwrap()
            .into()
    }
    fn fixture() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(".wsl-definitions-fixture-")
            .tempdir_in(env!("CARGO_MANIFEST_DIR"))
            .unwrap()
    }
    #[test]
    fn reviewed_source_changes_invalidate_without_execution_or_reattachment_reset() {
        let directory = fixture();
        fs::create_dir(directory.path().join(".devbox")).unwrap();
        let manifest = br#"{"schemaVersion":1,"tasks":{"dev":{"kind":"package-script","source":"package.json","selector":"dev"}}}"#;
        fs::write(directory.path().join(".devbox/project.json"), manifest).unwrap();
        fs::write(
            directory.path().join("package.json"),
            br#"{"scripts":{"dev":"must never run"}}"#,
        )
        .unwrap();
        let mut engine = Engine::default();
        let token = root(&mut engine, directory.path());
        let context = context();
        let read = |path: &str, optional| {
            request(
                "definitions_read",
                Some(&token),
                json!({"context":context,"path":path,"optional":optional}),
            )
        };
        assert_eq!(
            engine.dispatch(&read("package.json", false)),
            Err("wsl_context_required")
        );
        let attach = request(
            "definitions_attach",
            Some(&token),
            json!({"context":context}),
        );
        engine.dispatch(&attach).unwrap();
        let bytes: Option<Vec<u8>> = serde_json::from_value(
            engine
                .dispatch(&read(".devbox/project.json", true))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(bytes.unwrap(), manifest);
        engine.dispatch(&read("package.json", false)).unwrap();
        let validate = request(
            "definitions_validate",
            Some(&token),
            json!({"context":context}),
        );
        engine.roots.get_mut(&token).unwrap().touched =
            Instant::now() - ROOT_TTL - Duration::from_secs(1);
        engine.dispatch(&validate).unwrap();
        fs::write(
            directory.path().join("package.json"),
            br#"{"scripts":{"dev":"external edit"}}"#,
        )
        .unwrap();
        assert_eq!(
            engine.dispatch(&validate),
            Err("project_definition_changed")
        );
        assert_eq!(engine.dispatch(&attach), Err("project_definition_changed"));
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    }
    #[test]
    fn absent_manifest_parent_and_context_are_part_of_the_snapshot() {
        let directory = fixture();
        let mut engine = Engine::default();
        let token = root(&mut engine, directory.path());
        let context = context();
        engine
            .dispatch(&request(
                "definitions_attach",
                Some(&token),
                json!({"context":context}),
            ))
            .unwrap();
        assert_eq!(
            engine
                .dispatch(&request(
                    "definitions_read",
                    Some(&token),
                    json!({"context":context,"path":".devbox/project.json","optional":true})
                ))
                .unwrap(),
            Value::Null
        );
        let mut stale = context.clone();
        stale.revision += 1;
        for method in ["definitions_attach", "definitions_validate", "files_attach"] {
            assert_eq!(
                engine.dispatch(&request(method, Some(&token), json!({"context":stale}))),
                Err("file_context_changed")
            );
        }
        let validate = request(
            "definitions_validate",
            Some(&token),
            json!({"context":context}),
        );
        engine.dispatch(&validate).unwrap();
        fs::create_dir(directory.path().join(".devbox")).unwrap();
        assert_eq!(
            engine.dispatch(&validate),
            Err("project_definition_changed")
        );
    }
    #[test]
    fn links_escape_unknown_fields_and_cancelled_reads_never_return_source_contents() {
        let directory = fixture();
        let outside = fixture();
        fs::write(
            outside.path().join("source.json"),
            b"outside synthetic data",
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), directory.path().join("linked")).unwrap();
        fs::write(
            directory.path().join("source.json"),
            b"inside synthetic data",
        )
        .unwrap();
        let mut engine = Engine::default();
        let token = root(&mut engine, directory.path());
        let context = context();
        engine
            .dispatch(&request(
                "definitions_attach",
                Some(&token),
                json!({"context":context}),
            ))
            .unwrap();
        for path in [
            "linked/source.json",
            "../source.json",
            "/etc/passwd",
            "C:/file",
            "source.json:stream",
        ] {
            assert!(
                engine
                    .dispatch(&request(
                        "definitions_read",
                        Some(&token),
                        json!({"context":context,"path":path,"optional":true})
                    ))
                    .is_err(),
                "{path}"
            );
        }
        let mut read = request(
            "definitions_read",
            Some(&token),
            json!({"context":context,"path":"source.json","optional":false}),
        );
        assert_eq!(
            engine.dispatch_guarded(&read, &|| Err("wsl_request_cancelled")),
            Err("wsl_request_cancelled")
        );
        read.args["command"] = "forbidden".into();
        assert_eq!(engine.dispatch(&read), Err("wsl_request_invalid"));
        assert_eq!(
            fs::read(directory.path().join("source.json")).unwrap(),
            b"inside synthetic data"
        );
    }
}
