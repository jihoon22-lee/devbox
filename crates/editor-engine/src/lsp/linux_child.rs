//! Linux child retirement shared by LSP sessions and runtime probes. Keep the
//! direct child unreaped until the last signal, so a reused PID/group is never
//! a cleanup target. A reviewed first-party supervisor owns detached descendants.
use std::{
    io,
    process::ExitStatus,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::process::Child;

struct State {
    pid: Option<libc::pid_t>,
    supervised: bool,
}
#[derive(Clone)]
pub(super) struct LinuxChild {
    state: Arc<Mutex<State>>,
}
fn exited(pid: libc::pid_t) -> io::Result<bool> {
    loop {
        let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == 0 {
            return Ok(unsafe { info.si_pid() } == pid);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}
impl LinuxChild {
    pub(super) fn capture(child: &Child, supervised: bool) -> io::Result<Self> {
        let pid = child
            .id()
            .and_then(|pid| i32::try_from(pid).ok())
            .filter(|pid| *pid > 1)
            .ok_or_else(|| io::Error::other("native child unavailable"))?;
        exited(pid)?;
        Ok(Self {
            state: Arc::new(Mutex::new(State {
                pid: Some(pid),
                supervised,
            })),
        })
    }
    pub(super) fn terminate(&self) -> io::Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("native child unavailable"))?;
        let Some(pid) = state.pid else {
            return Ok(());
        };
        // waitid also proves direct-child ownership when it reports no exit.
        if let Err(error) = exited(pid) {
            state.pid = None;
            return Err(error);
        }
        let (target, signal) = if state.supervised {
            (pid, libc::SIGTERM)
        } else {
            (-pid, libc::SIGKILL)
        };
        if unsafe { libc::kill(target, signal) } != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error);
            }
        }
        Ok(())
    }
    fn poll(&self, child: &mut Child) -> io::Result<Option<ExitStatus>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("native child unavailable"))?;
        if let Some(pid) = state.pid {
            match exited(pid) {
                Ok(false) => return Ok(None),
                Ok(true) => {}
                Err(error) => {
                    state.pid = None;
                    return Err(error);
                }
            }
            if !state.supervised {
                // The zombie leader still reserves the group ID here.
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
            }
            // A supervised exit already confirms that every adopted child has
            // retired. Disable all later signals before reaping the owner.
            state.pid = None;
        }
        child.try_wait()
    }
    pub(super) async fn wait(&self, child: &mut Child) -> io::Result<ExitStatus> {
        loop {
            if let Some(status) = self.poll(child)? {
                return Ok(status);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}
impl Drop for LinuxChild {
    fn drop(&mut self) {
        if Arc::strong_count(&self.state) == 1 {
            let _ = self.terminate();
        }
    }
}
