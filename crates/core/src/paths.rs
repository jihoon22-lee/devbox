use crate::CoreError;
use devbox_protocol::channel::AppChannel;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance(String);

impl Instance {
    pub fn parse(s: &str) -> Result<Self, CoreError> {
        let mut chars = s.chars();
        let ok = s != "bin"
            && s.len() <= 32
            && chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if ok {
            Ok(Self(s.to_owned()))
        } else {
            Err(CoreError::InvalidInstance(s.to_owned()))
        }
    }
    pub fn from_env(env: &dyn Env) -> Result<Self, CoreError> {
        Self::parse(env.var("DEVBOX_INSTANCE").as_deref().unwrap_or("prod"))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn channel(&self) -> AppChannel {
        AppChannel::for_instance(self.as_str())
    }
    pub fn is_test(&self) -> bool {
        self.0.starts_with("test-")
    }
}

pub trait Env {
    fn var(&self, key: &str) -> Option<String>;
    fn uid(&self) -> u32;
}

pub struct OsEnv;

impl Env for OsEnv {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
    fn uid(&self) -> u32 {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata("/proc/self")
            .expect("Linux /proc/self must be available to resolve the current uid")
            .uid()
    }
}

#[derive(Debug, Clone)]
pub struct Paths {
    pub instance: Instance,
    pub home: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub xdg_config_home: PathBuf,
    pub config_dir: PathBuf,
    pub instance_config_dir: PathBuf,
    pub socket: PathBuf,
    pub bin_dir: PathBuf,
    pub log_dir: PathBuf,
    pub db_path: PathBuf,
    pub index_db_path: PathBuf,
    pub backups_dir: PathBuf,
}

impl Paths {
    pub fn resolve(instance: Instance, env: &dyn Env) -> Result<Self, CoreError> {
        let home = absolute_env_path(env, "HOME").ok_or(CoreError::NoHome)?;
        let xdg =
            |key: &str, rel: &str| absolute_env_path(env, key).unwrap_or_else(|| home.join(rel));
        let data_home = xdg("XDG_DATA_HOME", ".local/share");
        let channel = instance.channel().config();
        let data_root = data_home.join("devbox");
        let data_dir = data_root.join(instance.as_str());
        let state_dir = xdg("XDG_STATE_HOME", ".local/state")
            .join("devbox")
            .join(instance.as_str());
        let xdg_config_home = xdg("XDG_CONFIG_HOME", ".config");
        let config_dir = xdg_config_home.join(channel.storage_name);
        let runtime = absolute_env_path(env, "XDG_RUNTIME_DIR")
            .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", env.uid())));
        Ok(Self {
            socket: runtime.join(format!("devbox-{}.sock", instance.as_str())),
            instance_config_dir: config_dir.join(instance.as_str()),
            bin_dir: data_home.join(channel.storage_name).join("bin"),
            log_dir: state_dir.join("logs"),
            db_path: data_dir.join("devbox.db"),
            index_db_path: data_dir.join("index.db"),
            backups_dir: data_dir.join("backups"),
            instance,
            home,
            data_dir,
            state_dir,
            xdg_config_home,
            config_dir,
        })
    }

    pub fn ensure_dirs(&self) -> Result<(), CoreError> {
        for d in [
            &self.data_dir,
            &self.state_dir,
            &self.log_dir,
            &self.backups_dir,
            &self.instance_config_dir,
        ] {
            std::fs::create_dir_all(d).map_err(crate::io_err(format!("create {}", d.display())))?;
        }
        Ok(())
    }
}
fn absolute_env_path(env: &dyn Env, key: &str) -> Option<PathBuf> {
    env.var(key)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct FakeEnv(HashMap<String, String>);
    impl Env for FakeEnv {
        fn var(&self, k: &str) -> Option<String> {
            self.0.get(k).map(|v| v.to_string())
        }
        fn uid(&self) -> u32 {
            1000
        }
    }
    fn env(pairs: &[(&str, &str)]) -> FakeEnv {
        FakeEnv(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    #[test]
    fn instance_names_are_validated() {
        assert!(Instance::parse("prod").is_ok());
        assert!(
            Instance::parse("test-ab12")
                .expect("parse fixture instance")
                .is_test()
        );
        for bad in ["", "Prod", "1dev", "a/b", "x".repeat(33).as_str()] {
            assert!(Instance::parse(bad).is_err(), "{bad} should be rejected");
        }
        assert_eq!(
            Instance::from_env(&env(&[]))
                .expect("resolve fixture instance")
                .as_str(),
            "prod"
        );
        assert_eq!(
            Instance::from_env(&env(&[("DEVBOX_INSTANCE", "dev")]))
                .expect("resolve fixture instance")
                .as_str(),
            "dev"
        );
    }

    #[test]
    fn instance_bin_is_reserved_for_shared_binaries() {
        assert!(matches!(
            Instance::parse("bin"),
            Err(CoreError::InvalidInstance(name)) if name == "bin"
        ));
        assert!(Instance::parse("bin-tools").is_ok());
    }

    #[test]
    fn defaults_follow_xdg_layout() {
        let p = Paths::resolve(
            Instance::parse("dev").expect("parse fixture instance"),
            &env(&[("HOME", "/home/u")]),
        )
        .expect("parse fixture instance");
        assert_eq!(p.data_dir, PathBuf::from("/home/u/.local/share/devbox/dev"));
        assert_eq!(
            p.state_dir,
            PathBuf::from("/home/u/.local/state/devbox/dev")
        );
        assert_eq!(p.config_dir, PathBuf::from("/home/u/.config/devbox-dev"));
        assert_eq!(
            p.instance_config_dir,
            PathBuf::from("/home/u/.config/devbox-dev/dev")
        );
        assert_eq!(p.socket, PathBuf::from("/run/user/1000/devbox-dev.sock"));
        assert_eq!(
            p.bin_dir,
            PathBuf::from("/home/u/.local/share/devbox-dev/bin")
        );
        assert_eq!(
            p.db_path,
            PathBuf::from("/home/u/.local/share/devbox/dev/devbox.db")
        );
        assert_eq!(
            p.index_db_path,
            PathBuf::from("/home/u/.local/share/devbox/dev/index.db")
        );
    }

    #[test]
    fn xdg_variables_override_defaults() {
        let p = Paths::resolve(
            Instance::parse("prod").expect("parse fixture instance"),
            &env(&[
                ("HOME", "/h"),
                ("XDG_DATA_HOME", "/d"),
                ("XDG_STATE_HOME", "/s"),
                ("XDG_CONFIG_HOME", "/c"),
                ("XDG_RUNTIME_DIR", "/r"),
            ]),
        )
        .expect("parse fixture instance");
        assert_eq!(p.data_dir, PathBuf::from("/d/devbox/prod"));
        assert_eq!(p.state_dir, PathBuf::from("/s/devbox/prod"));
        assert_eq!(p.config_dir, PathBuf::from("/c/devbox"));
        assert_eq!(p.socket, PathBuf::from("/r/devbox-prod.sock"));
    }

    #[test]
    fn missing_home_is_an_error() {
        assert!(
            Paths::resolve(
                Instance::parse("prod").expect("parse fixture instance"),
                &env(&[])
            )
            .is_err()
        );
    }

    #[test]
    fn empty_instance_is_invalid_in_every_environment() {
        assert!(matches!(
            Instance::from_env(&env(&[("DEVBOX_INSTANCE", "")])),
            Err(CoreError::InvalidInstance(_))
        ));
        assert!(
            !Instance::parse("test")
                .expect("parse fixture instance")
                .is_test()
        );
        assert!(Instance::parse(&"a".repeat(32)).is_ok());
        for bad in ["a_b", "a.b", "a b", "é", "a\n"] {
            assert!(Instance::parse(bad).is_err());
        }
    }

    #[test]
    fn home_must_be_nonempty_and_absolute() {
        for home in ["", "relative"] {
            assert!(matches!(
                Paths::resolve(
                    Instance::parse("prod").expect("parse fixture instance"),
                    &env(&[("HOME", home)])
                ),
                Err(CoreError::NoHome)
            ));
        }
    }

    #[test]
    fn empty_or_relative_xdg_values_use_defaults() {
        for invalid in ["", "relative"] {
            let p = Paths::resolve(
                Instance::parse("dev").expect("parse fixture instance"),
                &env(&[
                    ("HOME", "/home/u"),
                    ("XDG_DATA_HOME", invalid),
                    ("XDG_STATE_HOME", invalid),
                    ("XDG_CONFIG_HOME", invalid),
                    ("XDG_RUNTIME_DIR", invalid),
                ]),
            )
            .expect("parse fixture instance");
            assert_eq!(p.data_dir, PathBuf::from("/home/u/.local/share/devbox/dev"));
            assert_eq!(
                p.state_dir,
                PathBuf::from("/home/u/.local/state/devbox/dev")
            );
            assert_eq!(p.config_dir, PathBuf::from("/home/u/.config/devbox-dev"));
            assert_eq!(p.socket, PathBuf::from("/run/user/1000/devbox-dev.sock"));
        }
    }

    #[test]
    fn instances_isolate_state_and_share_config_and_binaries() {
        let e = env(&[("HOME", "/home/u")]);
        let a = Paths::resolve(Instance::parse("prod").expect("parse fixture instance"), &e)
            .expect("parse fixture instance");
        let b = Paths::resolve(
            Instance::parse("test-ab").expect("parse fixture instance"),
            &e,
        )
        .expect("parse fixture instance");
        assert_ne!(a.data_dir, b.data_dir);
        assert_ne!(a.state_dir, b.state_dir);
        assert_ne!(a.log_dir, b.log_dir);
        assert_ne!(a.backups_dir, b.backups_dir);
        assert_ne!(a.db_path, b.db_path);
        assert_ne!(a.index_db_path, b.index_db_path);
        assert_ne!(a.instance_config_dir, b.instance_config_dir);
        assert_ne!(a.socket, b.socket);
        assert_eq!(a.config_dir, b.config_dir);
        assert_eq!(a.bin_dir, b.bin_dir);
    }

    #[test]
    fn ensure_dirs_creates_only_instance_directories_and_is_idempotent() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let home = temp.path().to_str().expect("fixture path is UTF-8");
        let p = Paths::resolve(
            Instance::parse("test-dirs").expect("parse fixture instance"),
            &env(&[("HOME", home), ("XDG_RUNTIME_DIR", home)]),
        )
        .expect("parse fixture instance");
        p.ensure_dirs().expect("create fixture directories");
        p.ensure_dirs().expect("create fixture directories");
        for dir in [
            &p.data_dir,
            &p.state_dir,
            &p.log_dir,
            &p.backups_dir,
            &p.instance_config_dir,
        ] {
            assert!(dir.is_dir(), "{}", dir.display());
        }
        for path in [&p.bin_dir, &p.socket, &p.db_path, &p.index_db_path] {
            assert!(!path.exists(), "{}", path.display());
        }
        assert!(!p.config_dir.join("prod").exists());
    }

    #[test]
    fn ensure_dirs_reports_io_context_and_source() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let home = temp.path().to_str().expect("fixture path is UTF-8");
        let p = Paths::resolve(
            Instance::parse("test-error").expect("parse fixture instance"),
            &env(&[("HOME", home)]),
        )
        .expect("parse fixture instance");
        std::fs::create_dir_all(p.data_dir.parent().expect("fixture path has parent"))
            .expect("fixture path has parent");
        std::fs::write(&p.data_dir, "blocked").expect("write fixture file");
        match p.ensure_dirs().expect_err("fixture operation must fail") {
            CoreError::Io { what, source } => {
                assert_eq!(what, format!("create {}", p.data_dir.display()));
                assert_eq!(source.kind(), std::io::ErrorKind::AlreadyExists);
            }
            error => panic!("unexpected error: {error}"),
        }
    }
    #[test]
    fn channels_isolate_binaries_config_and_instance_state_under_xdg() {
        let e = env(&[
            ("HOME", "/owned"),
            ("XDG_CONFIG_HOME", "/custom/config"),
            ("XDG_DATA_HOME", "/custom/data"),
        ]);
        for (instance, storage) in [
            ("prod", "devbox"),
            ("accept", "devbox-accept"),
            ("dev", "devbox-dev"),
            ("test-owned", "devbox"),
        ] {
            let p = Paths::resolve(Instance::parse(instance).unwrap(), &e).unwrap();
            assert_eq!(p.xdg_config_home, PathBuf::from("/custom/config"));
            assert_eq!(p.config_dir, PathBuf::from("/custom/config").join(storage));
            assert_eq!(p.instance_config_dir, p.config_dir.join(instance));
            assert_eq!(
                p.bin_dir,
                PathBuf::from("/custom/data").join(storage).join("bin")
            );
            assert_eq!(
                p.data_dir,
                PathBuf::from("/custom/data/devbox").join(instance)
            );
            assert_eq!(
                p.state_dir,
                PathBuf::from("/owned/.local/state/devbox").join(instance)
            );
            assert_eq!(
                crate::systemd::path_environment(&p)[1],
                ("XDG_CONFIG_HOME", PathBuf::from("/custom/config").as_path())
            );
        }
    }

    #[test]
    fn accept_config_does_not_load_prod_roots_or_secret_names() {
        let temp = tempfile::tempdir().unwrap();
        let e = env(&[("HOME", temp.path().to_str().unwrap())]);
        let prod = Paths::resolve(Instance::parse("prod").unwrap(), &e).unwrap();
        let accept = Paths::resolve(Instance::parse("accept").unwrap(), &e).unwrap();
        prod.ensure_dirs().unwrap();
        std::fs::write(
            prod.config_dir.join("config.toml"),
            "[secrets]\nnames=['PROD_SENTINEL']\n",
        )
        .unwrap();
        std::fs::write(
            prod.config_dir.join("config.local.toml"),
            "[windows]\nroot='C:\\prod-fixture'\n",
        )
        .unwrap();
        let config = crate::Config::load(&accept).unwrap();
        assert!(config.secrets.names.is_empty());
        assert_eq!(config.windows.root, None);
        accept.ensure_dirs().unwrap();
        std::fs::write(
            accept.config_dir.join("config.local.toml"),
            "[windows]\nroot='C:\\accept-fixture'\n[secrets]\nnames=['ACCEPT_SENTINEL']\n",
        )
        .unwrap();
        let config = crate::Config::load(&accept).unwrap();
        assert_eq!(config.windows.root.as_deref(), Some(r"C:\accept-fixture"));
        assert_eq!(config.secrets.names, ["ACCEPT_SENTINEL"]);
        let prod_config = crate::Config::load(&prod).unwrap();
        assert_eq!(
            prod_config.windows.root.as_deref(),
            Some(r"C:\prod-fixture")
        );
        assert_eq!(prod_config.secrets.names, ["PROD_SENTINEL"]);
    }
}
