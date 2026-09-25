//! Shared Windows job ownership; LSP retains its completion-port and 2 s policy.
pub(crate) use process_tree::windows_job::WindowsJob as WindowsJobObject;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "spawned only by the owned suspended-child test"]
    fn suspended_child_fixture() {
        let Some(marker) = std::env::var_os("DEVBOX_SUSPENDED_CHILD_MARKER") else {
            return;
        };
        std::fs::write(marker, b"executed").unwrap();
    }

    #[tokio::test]
    async fn suspended_child_cannot_execute_before_job_assignment() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("owned-marker");
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "lsp::process::windows_job::tests::suspended_child_fixture",
            ])
            .env("DEVBOX_SUSPENDED_CHILD_MARKER", &marker)
            .kill_on_drop(true);
        process_tree::ProcessTree::prepare_tokio(&mut command);
        let mut child = command.spawn().unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(75)).await;
        assert!(!marker.exists());
        let job = WindowsJobObject::assign_to(&child).unwrap();
        let status = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(status.success());
        assert_eq!(std::fs::read(marker).unwrap(), b"executed");
        job.terminate_and_wait().await.unwrap();
        assert!(job.is_empty().unwrap());
    }
}
