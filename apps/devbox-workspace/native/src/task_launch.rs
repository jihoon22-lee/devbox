//! Verify a reviewed task source and pin its actual Linux cwd before exec.
//! Called before the helper creates any threads; no task is exposed through Files IPC.
use crate::{
    engine,
    task_contract::{validate, TaskLaunch},
};
use devbox_filesystem::{filesystem_identity, open_filesystem_object, project::ProjectObservation};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    io::Read,
    os::{
        fd::AsRawFd,
        unix::{fs::OpenOptionsExt, process::CommandExt},
    },
    path::Path,
    process::Command,
};

pub fn run(args: &[OsString]) -> i32 {
    // JSON metadata, "--", then the already constructed Runtime supervisor argv.
    if args.len() < 4
        || args.len() > 264
        || args[1] != "--"
        || args.iter().map(|arg| arg.len()).sum::<usize>() > 256 * 1024
    {
        return 2;
    }
    let Some(encoded) = args[0].to_str() else {
        return 2;
    };
    let input: TaskLaunch = match serde_json::from_str(encoded) {
        Ok(value) => value,
        Err(_) => return 2,
    };
    if validate(&input).is_err() {
        return 2;
    }
    let prepared = (|| -> Result<_, &'static str> {
        let root = Path::new(&input.root);
        let observation = ProjectObservation::capture(root, engine::admit)?;
        if engine::stamp(observation.root_handle())? != input.root_object {
            return Err("task_root_changed");
        }
        let source = root.join(".vscode/tasks.json");
        devbox_filesystem::ensure_no_links(&source).map_err(|_| "task_source_changed")?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&source)
            .map_err(|_| "task_source_changed")?;
        if !file
            .metadata()
            .map_err(|_| "task_source_changed")?
            .is_file()
        {
            return Err("task_source_changed");
        }
        let identity = devbox_filesystem::opened_filesystem_identity(&file, false)
            .map_err(|_| "task_source_changed")?;
        let mut bytes = Vec::new();
        file.take(512 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "task_source_changed")?;
        if bytes.len() > 512 * 1024 {
            return Err("task_source_changed");
        }
        let digest = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        if digest != input.source_digest {
            return Err("task_source_changed");
        }
        let cwd = Path::new(&input.cwd);
        engine::admit(cwd)?;
        devbox_filesystem::ensure_no_links(cwd).map_err(|_| "task_cwd_changed")?;
        let canonical = cwd.canonicalize().map_err(|_| "task_cwd_changed")?;
        if !canonical.starts_with(observation.root()) {
            return Err("task_cwd_changed");
        }
        let (directory, cwd_identity) =
            open_filesystem_object(&canonical, true).map_err(|_| "task_cwd_changed")?;
        observation.revalidate()?;
        if filesystem_identity(&source, false).map_err(|_| "task_source_changed")? != identity
            || filesystem_identity(&canonical, true).map_err(|_| "task_cwd_changed")?
                != cwd_identity
        {
            return Err("task_source_changed");
        }
        // The retained descriptor selects the reviewed directory even if its old
        // pathname is replaced after this check. No second path-based chdir occurs.
        if unsafe { libc::fchdir(directory.as_raw_fd()) } != 0 {
            return Err("task_cwd_changed");
        }
        observation.revalidate()?;
        Ok((observation, directory))
    })();
    let Ok(_retained) = prepared else {
        return 73;
    };
    let _error = Command::new(&args[2]).args(&args[3..]).exec();
    74
}
