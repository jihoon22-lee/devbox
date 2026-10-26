use super::native;
use crate::core::{
    channel::{AppChannel, CHANNEL},
    self_check,
    update::runner::{
        Gate, ProcessIdentity, Result, RunnerError,
        files::{self, EXE_LIMIT, Manifest, Outcome, RECORD_LIMIT},
    },
};
use std::{
    ffi::OsString,
    fs::File,
    os::windows::{io::OwnedHandle, process::CommandExt},
    process::{Command, Stdio},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use windows::Win32::System::Threading::CREATE_NO_WINDOW;

const AUTHORITY_TIME: Duration = Duration::from_secs(60);
const INSTALL_TIME: Duration = Duration::from_secs(20 * 60);

pub fn run(arguments: &[OsString]) -> Result<()> {
    let [parent, installer, payload, commit, disarm, armed]: [OwnedHandle; 6] =
        native::receive_handles(arguments)?
            .try_into()
            .map_err(|_| RunnerError::Identity)?;
    let installer = File::from(installer);
    let mut payload = File::from(payload);
    let manifest = files::read_manifest(&mut payload)?;
    let current = native::current_process()?;
    let runner = native::identity(native::raw(&current))?;
    validate(&manifest, &installer, &payload, &parent, runner)?;
    let mut gate = Gate::new(manifest.binding.clone(), manifest.parent)?;
    gate.arm(&manifest.binding, native::identity(native::raw(&parent))?)?;
    files::record(&manifest, runner, Outcome::Armed, None, None)?;
    native::signal(&armed)?;
    let result = (|| {
        match native::wait_any(
            &[
                native::raw(&disarm),
                native::raw(&commit),
                native::raw(&parent),
            ],
            AUTHORITY_TIME,
        )? {
            Some(1) => gate.commit()?,
            Some(_) => {
                gate.disarm();
                return Err(RunnerError::Cancelled);
            }
            None => {
                gate.disarm();
                return Err(RunnerError::Timeout);
            }
        }
        files::record(&manifest, runner, Outcome::Committed, None, None)?;
        match native::wait_any(
            &[native::raw(&disarm), native::raw(&parent)],
            AUTHORITY_TIME,
        )? {
            Some(1) => gate.exited(manifest.parent)?,
            Some(_) => {
                gate.disarm();
                return Err(RunnerError::Cancelled);
            }
            None => {
                gate.disarm();
                return Err(RunnerError::Timeout);
            }
        }
        // The retained process object, validated before arming, has actually exited. PID lookup is not used.
        files::verify(
            &installer,
            &manifest.installer_proof,
            &CancellationToken::new(),
        )?;
        files::verify(
            &files::locked(&manifest.installer)?,
            &manifest.installer_proof,
            &CancellationToken::new(),
        )?;
        gate.take_install()?;
        install(&manifest, runner)?;
        gate.finish()?;
        Ok(())
    })();
    if let Err(error) = result {
        // install() preserves its precise Running/Unknown/result record. Preparation errors have no installer.
        if matches!(
            error,
            RunnerError::Cancelled
                | RunnerError::Timeout
                | RunnerError::Identity
                | RunnerError::Storage
                | RunnerError::InvalidBinding
        ) {
            let outcome = if error == RunnerError::Cancelled {
                Outcome::Cancelled
            } else {
                Outcome::Failed
            };
            let _ = files::record(&manifest, runner, outcome, None, Some(error));
        }
        return Err(error);
    }
    Ok(())
}
fn validate(
    manifest: &Manifest,
    installer: &File,
    payload: &File,
    parent: &OwnedHandle,
    runner: ProcessIdentity,
) -> Result<()> {
    let actual = std::fs::canonicalize(std::env::current_exe().map_err(|_| RunnerError::Identity)?)
        .map_err(|_| RunnerError::Identity)?;
    manifest.validate(CHANNEL, &actual)?;
    let expected_root =
        std::fs::canonicalize(native::local_root()?).map_err(|_| RunnerError::Identity)?;
    if actual.parent().and_then(|p| p.parent()) != Some(expected_root.as_path())
        || native::parent_pid()? != manifest.parent.pid
        || runner.session != manifest.parent.session
        || native::identity(native::raw(parent))? != manifest.parent
        || native::exited(native::raw(parent))?
        || native::image(native::raw(parent))? != manifest.parent_exe
        || !native::outside_job(native::raw(&native::current_process()?))?
    {
        return Err(RunnerError::Identity);
    }
    let token = CancellationToken::new();
    let owner_path = actual
        .parent()
        .ok_or(RunnerError::Identity)?
        .join("owner.json");
    let payload_proof = files::proof(payload, RECORD_LIMIT, &token)?;
    files::verify(&files::locked(&owner_path)?, &payload_proof, &token)?;
    files::verify(installer, &manifest.installer_proof, &token)?;
    files::verify(&files::locked(&actual)?, &manifest.runner_proof, &token)?;
    if manifest.runner_proof.digest != manifest.source_digest
        || files::proof(&files::locked(&manifest.parent_exe)?, EXE_LIMIT, &token)?.digest
            != manifest.source_digest
    {
        return Err(RunnerError::Identity);
    }
    match CHANNEL {
        AppChannel::Prod => {
            let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))
                .map_err(|_| RunnerError::Identity)?;
            let target = semver::Version::parse(&manifest.binding.target_version)
                .map_err(|_| RunnerError::Identity)?;
            if !current.pre.is_empty()
                || !current.build.is_empty()
                || target <= current
                || manifest.binding.fixture_install
            {
                return Err(RunnerError::Identity);
            }
        }
        #[cfg(devbox_update_fixture)]
        AppChannel::Accept if manifest.binding.fixture_install => {}
        _ => return Err(RunnerError::Identity),
    }
    Ok(())
}
fn expected_version(manifest: &Manifest) -> &str {
    #[cfg(devbox_update_fixture)]
    if CHANNEL == AppChannel::Accept && manifest.binding.fixture_install {
        return env!("CARGO_PKG_VERSION");
    }
    &manifest.binding.target_version
}
fn install(manifest: &Manifest, runner: ProcessIdentity) -> Result<()> {
    // The path is an internally-created frozen copy; no shell and no caller-controlled arguments.
    let mut child = match Command::new(&manifest.installer)
        .arg("/S")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
    {
        Ok(child) => child,
        Err(_) => {
            files::record(
                manifest,
                runner,
                Outcome::Failed,
                None,
                Some(RunnerError::Installer),
            )?;
            return Err(RunnerError::Installer);
        }
    };
    let installer = native::identity(native::raw(&child));
    // An installer has started. Any subsequent recording/query failure is unknown, never safe to retry.
    let installer_id = installer.ok();
    if files::record(manifest, runner, Outcome::Running, installer_id, None).is_err() {
        let _ = native::bounded_child(&mut child, INSTALL_TIME);
        return Err(RunnerError::Unknown);
    }
    let status = match native::bounded_child(&mut child, INSTALL_TIME) {
        Ok(Some(status)) => status,
        _ => {
            let _ = files::record(
                manifest,
                runner,
                Outcome::Unknown,
                installer_id,
                Some(RunnerError::Unknown),
            );
            // Never kill or relaunch around a possibly-running installer; retain its PID/start and files.
            return Err(RunnerError::Unknown);
        }
    };
    if !status.success() {
        files::record(
            manifest,
            runner,
            Outcome::Failed,
            installer_id,
            Some(RunnerError::Installer),
        )?;
        return Err(RunnerError::Installer);
    }
    let _frozen = match verify_relaunch(manifest) {
        Ok(frozen) => frozen,
        Err(error) => {
            let outcome = if error == RunnerError::Unknown {
                Outcome::Unknown
            } else {
                Outcome::Failed
            };
            files::record(manifest, runner, outcome, installer_id, Some(error))?;
            return Err(error);
        }
    };
    // Publish completed installation before normal startup's active-runner guard is consulted.
    files::record(manifest, runner, Outcome::Installed, installer_id, None)?;
    if Command::new(&manifest.parent_exe)
        .arg("--minimized")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
        .is_err()
    {
        files::record(
            manifest,
            runner,
            Outcome::Failed,
            installer_id,
            Some(RunnerError::Relaunch),
        )?;
        return Err(RunnerError::Relaunch);
    }
    Ok(())
}
fn verify_relaunch(manifest: &Manifest) -> Result<File> {
    let parent = manifest.parent_exe.parent().ok_or(RunnerError::Relaunch)?;
    let frozen = files::locked(&manifest.parent_exe).map_err(|_| RunnerError::Relaunch)?;
    let proof = files::proof(&frozen, EXE_LIMIT, &CancellationToken::new())
        .map_err(|_| RunnerError::Relaunch)?;
    if proof.size == 0 {
        return Err(RunnerError::Relaunch);
    }
    self_check::validate_resources(parent, expected_version(manifest))
        .map_err(|_| RunnerError::Relaunch)?;
    let mut check = Command::new(&manifest.parent_exe)
        .args(["--self-check", "--channel", manifest.channel.as_str()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
        .map_err(|_| RunnerError::Relaunch)?;
    match native::bounded_child(&mut check, Duration::from_secs(15)) {
        Ok(Some(status)) if status.success() => Ok(frozen),
        _ => {
            // This is only our read-only self-check child. No ordinary app or installer is terminated.
            let _ = check.kill();
            if matches!(
                native::bounded_child(&mut check, Duration::from_secs(2)),
                Ok(Some(_))
            ) {
                Err(RunnerError::Relaunch)
            } else {
                Err(RunnerError::Unknown)
            }
        }
    }
}
