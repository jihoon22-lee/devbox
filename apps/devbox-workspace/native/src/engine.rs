//! Linux filesystem observations. Root inspection executes no Git, hook, server
//! or package manager; only the Windows owner can turn the report into Registry state.
use crate::{ObjectStamp, Request, RootReport, MAX_ROOTS};
use devbox_filesystem::{ensure_no_links, project::ProjectObservation};
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
    pub fn dispatch(&mut self, request: &Request) -> Result<Value> {
        self.roots
            .retain(|_, root| root.touched.elapsed() < ROOT_TTL);
        match request.method.as_str() {
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
