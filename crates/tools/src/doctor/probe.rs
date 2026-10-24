pub trait Probe {
    fn pid1(&self) -> String;
    fn user_manager_available(&self) -> bool;
    fn linger(&self) -> String;
    fn tmux_version(&self) -> Option<String>;
    fn inotify_instances(&self) -> String;
    fn env_path_valid(&self) -> bool;
    fn which(&self, tool: &str, path: &str) -> bool;
    fn daemon_path(&self) -> String;
    fn hook_executable(&self) -> bool;
}
