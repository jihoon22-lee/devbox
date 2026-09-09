//! Desktop calls keep Tauri's existing scheduler. Native library consumers
//! supply their own Tokio runtime and never initialize a GUI application.
pub(crate) async fn spawn_blocking<T, F>(action: F) -> Result<T, ()>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    #[cfg(feature = "desktop")]
    {
        tauri::async_runtime::spawn_blocking(action)
            .await
            .map_err(|_| ())
    }
    #[cfg(not(feature = "desktop"))]
    {
        tokio::task::spawn_blocking(action).await.map_err(|_| ())
    }
}

#[cfg(test)]
pub(crate) fn block_on<F: std::future::Future>(future: F) -> F::Output {
    #[cfg(feature = "desktop")]
    {
        tauri::async_runtime::block_on(future)
    }
    #[cfg(not(feature = "desktop"))]
    {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("native fixture runtime")
            .block_on(future)
    }
}
