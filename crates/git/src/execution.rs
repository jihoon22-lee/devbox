//! Native product execution scopes. A scope is installed for each async poll
//! and explicitly inherited by blocking workers; it never leaks to another task.
use crate::GitTarget;
use std::{
    cell::RefCell,
    ffi::OsString,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct NativeRepository {
    pub worktree: PathBuf,
    pub git_dir: PathBuf,
    pub common_dir: PathBuf,
}

type Admission = dyn Fn(&GitTarget) -> Result<NativeRepository, String> + Send + Sync;
pub struct ExecutionPolicy {
    pub(crate) program: PathBuf,
    pub(crate) environment: Vec<(OsString, OsString)>,
    deadline: Instant,
    cancelled: AtomicBool,
    admit: Box<Admission>,
}
thread_local! {
    static CURRENT: RefCell<Option<Arc<ExecutionPolicy>>> = const { RefCell::new(None) };
}
pub fn current() -> Option<Arc<ExecutionPolicy>> {
    CURRENT.with(|slot| slot.borrow().clone())
}
struct Restore(Option<Arc<ExecutionPolicy>>);
impl Drop for Restore {
    fn drop(&mut self) {
        CURRENT.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}
pub struct CancelOnDrop(Arc<ExecutionPolicy>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
impl ExecutionPolicy {
    /// Only native code constructs this capability. The admission closure owns
    /// filesystem/config/tool evidence and any activity/worker permits.
    pub fn new(
        program: PathBuf,
        environment: Vec<(OsString, OsString)>,
        deadline: Instant,
        admit: impl Fn(&GitTarget) -> Result<NativeRepository, String> + Send + Sync + 'static,
    ) -> Result<Arc<Self>, String> {
        let bytes = environment.iter().try_fold(0usize, |sum, (key, value)| {
            sum.checked_add(key.as_encoded_bytes().len())?
                .checked_add(value.as_encoded_bytes().len())
        });
        if !program.is_absolute()
            || environment.len() > 2048
            || bytes.is_none_or(|bytes| bytes > 4 * 1024 * 1024)
            || environment.iter().any(|(key, value)| {
                key.is_empty()
                    || key
                        .as_encoded_bytes()
                        .iter()
                        .any(|byte| matches!(byte, 0 | b'='))
                    || value.as_encoded_bytes().contains(&0)
            })
            || Instant::now() >= deadline
        {
            return Err("git_execution_invalid".into());
        }
        Ok(Arc::new(Self {
            program,
            environment,
            deadline,
            cancelled: AtomicBool::new(false),
            admit: Box::new(admit),
        }))
    }
    pub fn scope<T>(self: &Arc<Self>, action: impl FnOnce() -> T) -> T {
        let previous = CURRENT.with(|slot| slot.borrow_mut().replace(self.clone()));
        let _restore = Restore(previous);
        action()
    }
    pub async fn scope_future<F: std::future::Future>(self: &Arc<Self>, future: F) -> F::Output {
        let mut future = std::pin::pin!(future);
        std::future::poll_fn(|context| self.scope(|| future.as_mut().poll(context))).await
    }
    pub fn cancel_on_drop(self: &Arc<Self>) -> CancelOnDrop {
        CancelOnDrop(self.clone())
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn boundary(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Acquire) {
            Err("git_cancelled".into())
        } else if Instant::now() >= self.deadline {
            Err("git_timeout".into())
        } else {
            Ok(())
        }
    }
    pub fn remaining(&self, requested: Duration) -> Result<Duration, String> {
        self.boundary()?;
        Ok(requested.min(self.deadline.saturating_duration_since(Instant::now())))
    }
    pub fn admit(&self, target: &GitTarget) -> Result<NativeRepository, String> {
        self.boundary()?;
        // WSL execution requires its own validated distro/executable transport.
        if target.is_wsl() {
            return Err("git_execution_target_unavailable".into());
        }
        let repository = (self.admit)(target)?;
        if !repository.worktree.is_absolute()
            || !repository.git_dir.is_absolute()
            || !repository.common_dir.is_absolute()
        {
            return Err("git_execution_invalid".into());
        }
        self.boundary()?;
        Ok(repository)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy() -> Arc<ExecutionPolicy> {
        ExecutionPolicy::new(
            std::env::current_exe().unwrap(),
            vec![],
            Instant::now() + Duration::from_secs(5),
            |_| Err("not_admitted".into()),
        )
        .unwrap()
    }
    #[test]
    fn nested_scopes_restore_even_when_the_inner_action_panics() {
        let outer = policy();
        let inner = policy();
        assert!(current().is_none());
        outer.scope(|| {
            assert!(Arc::ptr_eq(&current().unwrap(), &outer));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                inner.scope(|| panic!("fixture"))
            }));
            assert!(result.is_err());
            assert!(Arc::ptr_eq(&current().unwrap(), &outer));
        });
        assert!(current().is_none());
    }
    #[test]
    fn caller_drop_cancels_retained_worker_and_scope_is_not_implicitly_inherited() {
        let policy = policy();
        let guard = policy.cancel_on_drop();
        let retained = policy.clone();
        policy.scope(|| {
            std::thread::spawn(|| assert!(current().is_none()))
                .join()
                .unwrap()
        });
        assert!(retained.boundary().is_ok());
        drop(guard);
        assert_eq!(retained.boundary().unwrap_err(), "git_cancelled");
        assert!(retained.admit(&GitTarget::native("unused")).is_err());
    }

    #[test]
    fn native_command_fixes_program_repository_and_environment_without_redirects() {
        let root = std::env::temp_dir().join("native-root");
        let expected = root.clone();
        let program = std::env::current_exe().unwrap();
        let policy = ExecutionPolicy::new(
            program.clone(),
            vec![
                ("SSH_ASKPASS".into(), "synthetic-auth-helper".into()),
                ("GIT_DIR".into(), "unrelated-repository".into()),
            ],
            Instant::now() + Duration::from_secs(5),
            move |target| {
                if target.cwd() != expected.to_string_lossy() {
                    return Err("context_changed".into());
                }
                Ok(NativeRepository {
                    worktree: expected.clone(),
                    git_dir: expected.join(".git"),
                    common_dir: expected.join(".git"),
                })
            },
        )
        .unwrap();
        policy.scope(|| {
            let command = crate::command_for_target(
                &GitTarget::native(root.to_string_lossy()),
                &["status"],
                Duration::from_secs(5),
            )
            .unwrap();
            assert_eq!(command.get_program(), program);
            let args = command
                .get_args()
                .map(|value| value.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            assert_eq!(
                args[0],
                format!("--git-dir={}", root.join(".git").display())
            );
            assert_eq!(args[1], format!("--work-tree={}", root.display()));
            let env = command
                .get_envs()
                .collect::<std::collections::BTreeMap<_, _>>();
            assert_eq!(
                env[std::ffi::OsStr::new("SSH_ASKPASS")],
                Some(std::ffi::OsStr::new("synthetic-auth-helper"))
            );
            assert_eq!(
                env.get(std::ffi::OsStr::new("GIT_DIR")).copied().flatten(),
                None
            );
            assert!(crate::command_for_target(
                &GitTarget::native("unregistered"),
                &["status"],
                Duration::from_secs(5)
            )
            .is_err());
        });
        assert!(current().is_none());
    }

    #[test]
    fn interleaved_async_polls_do_not_borrow_another_requests_policy() {
        use std::{
            future::Future,
            task::{Context, Poll, Waker},
        };
        let left = policy();
        let right = policy();
        let future = |expected: Arc<ExecutionPolicy>| {
            let mut waiting = true;
            std::future::poll_fn(move |_| {
                assert!(Arc::ptr_eq(&current().unwrap(), &expected));
                if std::mem::take(&mut waiting) {
                    Poll::Pending
                } else {
                    Poll::Ready(())
                }
            })
        };
        let mut a = std::pin::pin!(left.scope_future(future(left.clone())));
        let mut b = std::pin::pin!(right.scope_future(future(right.clone())));
        let mut context = Context::from_waker(Waker::noop());
        assert!(a.as_mut().poll(&mut context).is_pending());
        assert!(current().is_none());
        assert!(b.as_mut().poll(&mut context).is_pending());
        assert!(current().is_none());
        assert!(a.as_mut().poll(&mut context).is_ready());
        assert!(b.as_mut().poll(&mut context).is_ready());
        assert!(current().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn caller_cancellation_terminates_the_owned_child_before_its_legacy_timeout() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("devbox-git-policy-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let program = root.join("owned-git");
        std::fs::write(&program, "#!/bin/sh\n/bin/sleep 5\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let owned_root = root.clone();
        let policy = ExecutionPolicy::new(
            program,
            vec![],
            Instant::now() + Duration::from_secs(5),
            move |_| {
                Ok(NativeRepository {
                    worktree: owned_root.clone(),
                    git_dir: owned_root.join(".git"),
                    common_dir: owned_root.join(".git"),
                })
            },
        )
        .unwrap();
        let caller = policy.cancel_on_drop();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            drop(caller);
        });
        let started = Instant::now();
        let result = policy.scope(|| {
            crate::run_bounded(
                &["status"],
                &root.to_string_lossy(),
                Duration::from_secs(5),
                1024,
            )
        });
        thread.join().unwrap();
        assert_eq!(result.unwrap_err(), "git_cancelled");
        assert!(started.elapsed() < Duration::from_secs(2));
        std::fs::remove_dir_all(root).unwrap();
    }
}
