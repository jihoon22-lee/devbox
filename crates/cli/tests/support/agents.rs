use super::TestDaemon;
use devbox_cli::client::Client;
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};
impl TestDaemon {
    pub fn start_with_env(env: &[(&str, &str)]) -> Self {
        let mut daemon = Self::fixture();
        daemon.extra_env = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        daemon.launch();
        daemon
    }
    /// Configure a fixture before `launch`; production config is loaded once at startup.
    pub fn write_config(&self, text: &str) {
        assert!(self.child.is_none(), "write config before launch");
        std::fs::create_dir_all(&self.paths.config_dir).expect("create owned config");
        std::fs::write(self.paths.config_dir.join("config.toml"), text)
            .expect("write owned config");
    }
    pub fn git_repo(&self, relative: &str) -> String {
        let path = self.home.path().join(relative);
        assert!(path.starts_with(self.home.path()));
        assert!(!Path::new(relative).components().any(|c| matches!(
            c,
            std::path::Component::ParentDir | std::path::Component::RootDir
        )));
        std::fs::create_dir_all(&path).expect("create owned repository");
        self.git(&path, &["init", "-q", "-b", "main"]);
        std::fs::write(path.join("initial.txt"), "initial\n").expect("write initial file");
        self.git(&path, &["add", "initial.txt"]);
        self.git(&path, &["commit", "-qm", "initial"]);
        path.to_str().expect("fixture UTF-8").into()
    }
    pub fn git(&self, path: &Path, args: &[&str]) {
        let output = self
            .environment("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .expect("fixture git");
        assert!(
            output.status.success(),
            "fixture git: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    pub async fn add_project(&self, client: &Client, path: &str) -> String {
        let body = client
            .raw_call("projects.add", json!({"path":path}), None)
            .await
            .expect("add fixture project");
        serde_json::from_str::<Value>(&body).expect("project")["id"]
            .as_str()
            .expect("id")
            .into()
    }
    pub async fn create_agent(
        &self,
        client: &Client,
        project: &str,
        profile: &str,
        worktree: bool,
    ) -> String {
        let body = client.raw_call("agents.create", json!({"projectId":project,"title":"Fix login","profile":profile,"worktree":worktree,"prompt":"fix it"}), None).await.expect("create fixture agent");
        serde_json::from_str::<Value>(&body).expect("task")["id"]
            .as_str()
            .expect("id")
            .into()
    }
    pub async fn wait_for(
        &self,
        client: &Client,
        id: &str,
        state: &str,
        timeout: Duration,
    ) -> Value {
        let deadline = Instant::now() + timeout;
        loop {
            let body = client
                .raw_call("agents.get", json!({"id":id}), None)
                .await
                .expect("get fixture agent");
            let task: Value = serde_json::from_str(&body).expect("task");
            if task["state"] == state {
                return task;
            }
            assert!(
                Instant::now() < deadline,
                "expected {state}, got {task}; {}",
                std::fs::read_to_string(self.home.path().join("stderr.log")).unwrap_or_default()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    pub fn kill_tmux_server(&self) {
        let _ = self
            .environment("tmux")
            .args(["-L", &format!("devbox-{}", self.instance), "kill-server"])
            .status();
    }
}
