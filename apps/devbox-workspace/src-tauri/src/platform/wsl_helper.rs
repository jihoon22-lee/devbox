//! Native-only, single-flight private stdio connection to the packaged helper.
//! Construct and call on a blocking Workspace worker, never a renderer thread.
#[cfg(windows)]
mod native {
    use super::super::wsl_distro::{self, Lease};
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    use std::{
        fs::File,
        io::Read,
        os::windows::fs::{MetadataExt, OpenOptionsExt},
        path::Path,
        process::Stdio,
        time::Duration,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        process::{Child, ChildStdin, ChildStdout},
        runtime::Runtime,
        task::JoinHandle,
    };
    use workspace_wsl::{Request, Response, RootReport, MAX_FRAME_BYTES, VERSION};
    type Result<T> = std::result::Result<T, &'static str>;

    struct Artifact {
        _file: File,
        _directories: Vec<File>,
    }
    impl Artifact {
        fn open(directory: &Path) -> Result<Self> {
            let expected =
                option_env!("DEVBOX_WSL_HELPER_SHA256").ok_or("wsl_helper_unavailable")?;
            let size: u64 = option_env!("DEVBOX_WSL_HELPER_BYTES")
                .ok_or("wsl_helper_unavailable")?
                .parse()
                .map_err(|_| "wsl_helper_unavailable")?;
            Self::pin(directory, expected, size)
        }
        fn pin(directory: &Path, expected: &str, size: u64) -> Result<Self> {
            if !(64..=64 * 1024 * 1024).contains(&size) {
                return Err("wsl_helper_unavailable");
            }
            let path = directory.join("devbox-workspace-wsl");
            super::super::windows_path::admit(&path)?;
            devbox_filesystem::ensure_no_links(&path).map_err(|_| "wsl_helper_changed")?;
            // Pin every ancestor from the volume root down before opening
            // the next component. Denying delete sharing prevents a directory
            // rename from substituting different bytes at the launch path.
            let mut directories = Vec::new();
            for parent in directory.ancestors().collect::<Vec<_>>().into_iter().rev() {
                let handle = std::fs::OpenOptions::new()
                    .read(true)
                    .access_mode(0x80)
                    .share_mode(3)
                    .custom_flags(0x02000000 | 0x00200000)
                    .open(parent)
                    .map_err(|_| "wsl_helper_unavailable")?;
                let metadata = handle.metadata().map_err(|_| "wsl_helper_unavailable")?;
                if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
                    return Err("wsl_helper_changed");
                }
                directories.push(handle);
            }
            // Read sharing only pins the exact installed bytes through launch
            // and every request. No writer or rename can replace this object.
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(1)
                .custom_flags(0x00200000)
                .open(&path)
                .map_err(|_| "wsl_helper_unavailable")?;
            let metadata = file.metadata().map_err(|_| "wsl_helper_unavailable")?;
            if !metadata.is_file()
                || metadata.len() != size
                || metadata.file_attributes() & 0x400 != 0
            {
                return Err("wsl_helper_changed");
            }
            let mut hash = Sha256::new();
            let mut buffer = [0_u8; 65536];
            loop {
                let count = file
                    .read(&mut buffer)
                    .map_err(|_| "wsl_helper_unavailable")?;
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
            }
            let actual = hash
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            if actual != expected {
                return Err("wsl_helper_changed");
            }
            Ok(Self {
                _file: file,
                _directories: directories,
            })
        }
    }
    pub struct Connection {
        lease: Lease,
        _artifact: Artifact,
        runtime: Runtime,
        child: Child,
        input: Option<ChildStdin>,
        output: Option<ChildStdout>,
        stderr: JoinHandle<()>,
        session: String,
        sequence: u64,
        failed: bool,
        retired: bool,
    }
    impl Connection {
        /// `directory` comes from AppHandle's native resource directory. The
        /// renderer supplies neither helper paths nor executable arguments.
        pub fn connect(directory: &Path, distro: &str, allow_start: bool) -> Result<Self> {
            let artifact = Artifact::open(directory)?;
            let lease = Lease::capture(distro, allow_start)?;
            let session = uuid::Uuid::new_v4().to_string();
            let args = lease.helper_args(directory, &session, allow_start)?;
            let executable = wsl_distro::executable()?;
            let system = executable.parent().ok_or("wsl_unavailable")?;
            let root = system.parent().ok_or("wsl_unavailable")?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| "wsl_unavailable")?;
            let mut command = tokio::process::Command::new(&executable);
            command
                .args(args)
                .current_dir(system)
                .env_clear()
                .env("SystemRoot", root)
                .env("PATH", system)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .creation_flags(0x08000000);
            let mut child = {
                let _guard = runtime.enter();
                command.spawn().map_err(|_| "wsl_unavailable")?
            };
            let input = child.stdin.take().ok_or("wsl_unavailable")?;
            let output = child.stdout.take().ok_or("wsl_unavailable")?;
            let mut errors = child.stderr.take().ok_or("wsl_unavailable")?;
            let stderr = runtime.spawn(async move {
                // Discard content, retain no Linux paths or server output in
                // host logs. EOF or excessive stderr retires the connection.
                let mut count = 0;
                let mut bytes = [0_u8; 4096];
                loop {
                    match errors.read(&mut bytes).await {
                        Ok(0) | Err(_) => break,
                        Ok(length) => {
                            count += length;
                            if count > 65536 {
                                break;
                            }
                        }
                    }
                }
            });
            let mut connection = Self {
                lease,
                _artifact: artifact,
                runtime,
                child,
                input: Some(input),
                output: Some(output),
                stderr,
                session,
                sequence: 0,
                failed: false,
                retired: false,
            };
            let hello = connection.request("hello", None, serde_json::json!({}), 5000)?;
            if hello["version"] != VERSION {
                return Err("wsl_protocol_invalid");
            }
            Ok(connection)
        }
        pub fn lease(&self) -> &Lease {
            &self.lease
        }
        pub fn is_open(&self) -> bool {
            !self.failed && !self.retired
        }
        pub fn observe(&mut self, path: &str) -> Result<RootReport> {
            let value = self.request(
                "observe_root",
                None,
                serde_json::json!({"path": path}),
                10000,
            )?;
            let report: RootReport =
                serde_json::from_value(value).map_err(|_| "wsl_protocol_invalid")?;
            report.validate()?;
            Ok(report)
        }
        pub fn validate(&mut self, token: &str) -> Result<()> {
            let value = self.request("validate_root", Some(token), serde_json::json!({}), 5000)?;
            let report: RootReport =
                serde_json::from_value(value).map_err(|_| "wsl_protocol_invalid")?;
            report.validate()?;
            if report.token != token {
                self.retire();
                return Err("wsl_protocol_invalid");
            }
            Ok(())
        }
        pub fn release(&mut self, token: &str) -> Result<()> {
            self.request("release_root", Some(token), serde_json::json!({}), 5000)
                .map(|_| ())
        }
        /// Private file methods require the registered distro context on every
        /// call. No arbitrary helper command or native picker is exposed.
        pub fn file_request(&mut self, method: &str, token: &str, args: Value) -> Result<Value> {
            self.file_request_until(method, token, args, u64::MAX)
        }
        pub fn file_request_until(
            &mut self,
            method: &str,
            token: &str,
            args: Value,
            deadline: u64,
        ) -> Result<Value> {
            if !matches!(
                method,
                "definitions_attach"
                    | "definitions_read"
                    | "definitions_validate"
                    | "definitions_write"
                    | "files_attach"
                    | "files_poll"
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
                return Err("wsl_request_invalid");
            }
            if args["context"]["target"]["kind"] != "wsl"
                || args["context"]["target"]["distroId"] != self.lease.id()
            {
                return Err("wsl_context_invalid");
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| "wsl_timeout")?
                .as_millis();
            let remaining = u128::from(deadline).saturating_sub(now).min(15000) as u32;
            if remaining == 0 {
                return Err("wsl_timeout");
            }
            self.request(method, Some(token), args, remaining)
        }
        fn request(
            &mut self,
            method: &str,
            root: Option<&str>,
            args: Value,
            budget: u32,
        ) -> Result<Value> {
            if self.failed {
                return Err("wsl_connection_closed");
            }
            self.lease.revalidate()?;
            let sequence = self.sequence.checked_add(1).ok_or("wsl_protocol_invalid")?;
            let request = Request {
                version: VERSION,
                session_id: self.session.clone(),
                request_id: uuid::Uuid::new_v4().to_string(),
                sequence,
                budget_ms: budget,
                method: method.into(),
                root_token: root.map(str::to_owned),
                args,
            };
            request.validate(&self.session)?;
            self.sequence = sequence;
            let mut frame = Vec::new();
            workspace_wsl::write_frame(&mut frame, &request).map_err(|_| "wsl_protocol_invalid")?;
            let input = self.input.as_mut().ok_or("wsl_connection_closed")?;
            let output = self.output.as_mut().ok_or("wsl_connection_closed")?;
            let stderr = &mut self.stderr;
            let result = self.runtime.block_on(async { tokio::time::timeout(Duration::from_millis(u64::from(budget) + 1000), async {
                tokio::select! {
                    _ = stderr => Err("wsl_connection_closed"),
                    result = async {
                        input.write_all(&frame).await.map_err(|_| "wsl_connection_closed")?;
                        input.flush().await.map_err(|_| "wsl_connection_closed")?;
                        let length = output.read_u32_le().await.map_err(|_| "wsl_connection_closed")? as usize;
                        if length == 0 || length > MAX_FRAME_BYTES { return Err("wsl_protocol_invalid"); }
                        let mut body = vec![0; length];
                        output.read_exact(&mut body).await.map_err(|_| "wsl_connection_closed")?;
                        serde_json::from_slice::<Response>(&body).map_err(|_| "wsl_protocol_invalid")
                    } => result,
                }
            }).await }).unwrap_or(Err("wsl_timeout"));
            let response = match result {
                Ok(response)
                    if response.version == VERSION
                        && response.session_id == self.session
                        && response.request_id == request.request_id
                        && response.sequence == request.sequence =>
                {
                    response
                }
                other => {
                    self.retire();
                    return Err(other.err().unwrap_or("wsl_protocol_invalid"));
                }
            };
            if let Err(error) = self.lease.revalidate() {
                self.retire();
                return Err(error);
            }
            response.result.map_err(|error| match error.as_str() {
                "wsl_root_expired" => "wsl_root_expired",
                "wsl_root_changed" => "wsl_root_changed",
                "wsl_identity_unavailable" => "wsl_identity_unavailable",
                "project_object_changed" => "wsl_root_changed",
                "wsl_context_invalid" => "wsl_context_invalid",
                "wsl_context_required" => "wsl_context_required",
                "wsl_native_filesystem_required" => "wsl_native_filesystem_required",
                "wsl_filesystem_unavailable" => "wsl_filesystem_unavailable",
                "project_definition_changed" => "project_definition_changed",
                "project_definition_unavailable" => "project_definition_unavailable",
                "project_definition_limit" => "project_definition_limit",
                "unsafe_project_definition" => "unsafe_project_definition",
                "definition_preview_stale" => "definition_preview_stale",
                "invalid_manifest" => "invalid_manifest",
                "manifest_limit" => "manifest_limit",
                "unsupported_manifest_version" => "unsupported_manifest_version",
                "file_context_changed" => "file_context_changed",
                "file_snapshot_changed" => "file_snapshot_changed",
                "file_save_conflict" => "file_save_conflict",
                "file_rename_conflict" => "file_rename_conflict",
                "file_rename_unconfirmed" => "file_rename_unconfirmed",
                "file_delete_conflict" => "file_delete_conflict",
                "wsl_request_cancelled" => "wsl_request_cancelled",
                "wsl_timeout" => "wsl_timeout",
                "file_selection_required" => "file_selection_required",
                "unsafe_file_path" => "unsafe_file_path",
                "file_target_unavailable" => "file_target_unavailable",
                "file_path_limit" => "file_path_limit",
                "file_limit" => "file_limit",
                "file_owner_path" => "file_owner_path",
                "file_read_failed" => "file_read_failed",
                "file_listing_unavailable" => "file_listing_unavailable",
                "file_preview_unavailable" => "file_preview_unavailable",
                "file_unavailable" => "file_unavailable",
                "file_changed" => "file_changed",
                _ => "wsl_operation_failed",
            })
        }
        fn retire(&mut self) {
            let _ = self.shutdown();
        }
        pub fn shutdown(&mut self) -> Result<()> {
            if self.retired {
                return Ok(());
            }
            self.failed = true;
            // EOF cancels at the helper's precommit boundary. Drain an abandoned
            // response without retaining its contents, so a full stdout pipe
            // cannot block the Windows launcher during confirmed retirement.
            self.input.take();
            let output = self.output.take();
            let child = &mut self.child;
            let stopped = self.runtime.block_on(async {
                let complete = async {
                    let drain = async move {
                        if let Some(mut output) = output {
                            let mut bytes = [0u8; 16384];
                            let mut total = 0usize;
                            loop {
                                match output.read(&mut bytes).await {
                                    Ok(0) | Err(_) => break,
                                    Ok(length) => {
                                        total += length;
                                        if total > MAX_FRAME_BYTES + 4 {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    };
                    let (status, _) = tokio::join!(child.wait(), drain);
                    status.map(|_| ()).map_err(|_| "wsl_shutdown_unconfirmed")
                };
                // The helper has five seconds to retire blocked atomic/read IO.
                match tokio::time::timeout(Duration::from_secs(8), complete).await {
                    Ok(Ok(())) => Ok(()),
                    _ => {
                        let killed = child.kill().await;
                        if killed.is_ok() || child.try_wait().ok().flatten().is_some() {
                            Ok(())
                        } else {
                            Err("wsl_shutdown_unconfirmed")
                        }
                    }
                }
            });
            self.stderr.abort();
            self.retired = stopped.is_ok();
            stopped
        }
    }
    impl Drop for Connection {
        fn drop(&mut self) {
            self.retire();
        }
    }
    #[cfg(test)]
    mod artifact_tests {
        use super::*;
        #[test]
        fn artifact_lease_blocks_file_and_ancestor_substitution() {
            let temporary = tempfile::tempdir().unwrap();
            let directory = temporary.path().join("resource parent").join("wsl");
            std::fs::create_dir_all(&directory).unwrap();
            let binary = directory.join("devbox-workspace-wsl");
            let bytes = [42_u8; 128];
            std::fs::write(&binary, bytes).unwrap();
            let expected = Sha256::digest(bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            let lease = Artifact::pin(&directory, &expected, bytes.len() as u64).unwrap();
            assert!(std::fs::write(&binary, b"replacement").is_err());
            assert!(std::fs::rename(&binary, directory.join("renamed")).is_err());
            let parent = directory.parent().unwrap();
            let moved = temporary.path().join("moved");
            assert!(std::fs::rename(parent, &moved).is_err());
            drop(lease);
            std::fs::rename(parent, &moved).unwrap();
            assert!(Artifact::pin(&directory, &expected, bytes.len() as u64).is_err());
            assert_eq!(
                std::fs::read(moved.join("wsl/devbox-workspace-wsl")).unwrap(),
                bytes
            );
        }
    }
}
#[cfg(windows)]
pub use native::Connection;
