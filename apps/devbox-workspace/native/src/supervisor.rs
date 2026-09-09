//! One reaping owner per native job, including detached descendants on WSL1.
//! This entry point runs before the helper creates any threads. It is private
//! process plumbing, never a method in the renderer-facing pipe protocol.
use std::{
    ffi::OsString,
    io,
    path::Path,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

static CANCELLED: AtomicBool = AtomicBool::new(false);
extern "C" fn cancel(_: libc::c_int) {
    CANCELLED.store(true, Ordering::Release);
}

fn observe(pid: libc::id_t, kind: libc::idtype_t) -> io::Result<libc::siginfo_t> {
    let mut info = unsafe { std::mem::zeroed() };
    let result = unsafe {
        libc::waitid(
            kind,
            pid,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result == 0 {
        Ok(info)
    } else {
        Err(io::Error::last_os_error())
    }
}

fn initialize() -> io::Result<()> {
    let parent = unsafe { libc::getppid() };
    if parent <= 1 {
        return Err(io::Error::other("missing owner"));
    }
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        libc::sigemptyset(&mut action.sa_mask);
        action.sa_sigaction = libc::SIG_DFL;
        // Never auto-reap: an unreaped direct child pins the PID until we have
        // sent its last signal. No other thread or handler calls wait here.
        if libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error());
        }
        action.sa_sigaction = cancel as *const () as usize;
        for signal in [libc::SIGTERM, libc::SIGINT] {
            if libc::sigaction(signal, &action, std::ptr::null_mut()) != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        let mut signals: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut signals);
        for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGCHLD] {
            libc::sigaddset(&mut signals, signal);
        }
        if libc::sigprocmask(libc::SIG_UNBLOCK, &signals, std::ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) != 0
            || libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM, 0, 0, 0) != 0
        {
            return Err(io::Error::last_os_error());
        }
        if libc::getppid() != parent {
            CANCELLED.store(true, Ordering::Release);
        }
    }
    Ok(())
}

/// Signal only positively identified, unreaped direct children. /proc is a
/// discovery source, not authority. waitid also succeeds (si_pid == 0) for a
/// living child; ECHILD rejects every unrelated candidate. Detached descendants
/// are adopted after their parent dies and handled on a subsequent pass.
fn retire_pass() -> io::Result<bool> {
    match observe(0, libc::P_ALL) {
        Err(error) if error.raw_os_error() == Some(libc::ECHILD) => return Ok(true),
        Err(error) if error.kind() != io::ErrorKind::Interrupted => return Err(error),
        _ => {}
    }
    // WSL1 has neither pidfd nor /proc/self/task/<pid>/children. Streaming this
    // directory uses bounded memory, regardless of the machine's process count.
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == 0 || pid > i32::MAX as u32 {
            continue;
        }
        if observe(pid, libc::P_PID).is_err() {
            continue;
        }
        // Between this proof and waitpid, the only reaper is this thread. Even
        // if the child exits meanwhile its zombie reserves this exact PID.
        unsafe {
            libc::kill(pid as i32, libc::SIGKILL);
            libc::waitpid(pid as i32, std::ptr::null_mut(), libc::WNOHANG);
        }
    }
    Ok(false)
}

pub fn run(args: &[OsString]) -> i32 {
    if args.len() < 2
        || args[0] != "--"
        || args.len() > 514
        || !Path::new(&args[1]).is_absolute()
        || args
            .iter()
            .any(|arg| arg.as_encoded_bytes().len() > 256 * 1024)
        || args
            .iter()
            .map(|arg| arg.as_encoded_bytes().len())
            .sum::<usize>()
            > 1024 * 1024
        || initialize().is_err()
    {
        return 125;
    }
    if CANCELLED.load(Ordering::Acquire) {
        return 143;
    }
    let child = match Command::new(&args[1])
        .args(&args[2..])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return 125,
    };
    let root = child.id();
    // Native waitid/waitpid below exclusively own reaping; Child::drop does not
    // signal or wait. Never call try_wait, which would release the PID early.
    drop(child);
    let exit_code = loop {
        if CANCELLED.load(Ordering::Acquire) {
            break 143;
        }
        match observe(root, libc::P_PID) {
            Ok(info) if unsafe { info.si_pid() } != 0 => {
                break if info.si_code == libc::CLD_EXITED {
                    unsafe { info.si_status() }
                } else {
                    128 + unsafe { info.si_status() }
                };
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break 125,
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    loop {
        if matches!(retire_pass(), Ok(true)) {
            return exit_code;
        }
        // Do not abandon an unconfirmed tree or signal unrelated processes to
        // enforce a timeout. The caller retains this owner until confirmed exit.
        std::thread::sleep(Duration::from_millis(5));
    }
}
