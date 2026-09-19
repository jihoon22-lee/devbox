/// Synchronous WSL leases own their own runtime. Terminal's retained blocking
/// worker also drives an async PTY future, so lease calls and final destruction
/// must leave that runtime context. Join before returning: no detached worker or
/// request/cleanup permit is abandoned when the renderer goes away.
#[cfg(any(windows, test))]
pub(crate) fn outside_runtime<T: Send>(call: impl FnOnce() -> T + Send) -> Result<T, &'static str> {
    if tokio::runtime::Handle::try_current().is_err() {
        return Ok(call());
    }
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("workspace-native-call".into())
            .spawn_scoped(scope, call)
            .map_err(|_| "native_worker_unavailable")?
            .join()
            .map_err(|_| "native_worker_unavailable")
    })
}
