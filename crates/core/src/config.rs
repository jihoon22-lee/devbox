use crate::{CoreError, Paths};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub projects: ProjectsConfig,
    pub pc: PcConfig,
    pub terminal: TerminalConfig,
    pub agents: AgentsConfig,
    pub mcp: McpConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectsConfig {
    pub roots: Vec<String>,
}

impl Default for ProjectsConfig {
    fn default() -> Self {
        Self {
            roots: vec!["~/projects".into()],
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PcConfig {
    pub name: Option<String>,
    pub windows_root: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    pub profiles: Vec<TerminalProfile>,
}
#[derive(Debug, Clone, serde::Serialize, Deserialize, PartialEq, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct TerminalProfile {
    pub name: String,
    pub command: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct AgentsConfig {
    pub profiles: Vec<toml::Value>,
    pub cleanup_after_close: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct McpConfig {
    pub write: bool,
}

fn config_error(path: &Path, message: impl ToString) -> CoreError {
    CoreError::Config {
        path: path.display().to_string(),
        message: message.to_string(),
    }
}

fn read_table(path: &Path) -> Result<toml::Table, CoreError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(toml::Table::new()),
        Err(source) => {
            return Err(CoreError::Io {
                what: format!("read {}", path.display()),
                source,
            });
        }
    };
    let table = text
        .parse::<toml::Table>()
        .map_err(|error| config_error(path, error))?;
    // Serde structs also accept positional sequences; TOML sections must be tables.
    for section in ["projects", "pc", "terminal", "agents", "mcp"] {
        if table.get(section).is_some_and(|value| !value.is_table()) {
            return Err(config_error(path, format!("{section} must be a table")));
        }
    }
    for section in ["terminal", "agents"] {
        if let Some(profiles) = table
            .get(section)
            .and_then(|value| value.get("profiles"))
            .and_then(toml::Value::as_array)
            && profiles.iter().any(|value| !value.is_table())
        {
            return Err(config_error(
                path,
                format!("{section} profiles must be tables"),
            ));
        }
    }
    // Validate each source before merging so an override cannot hide invalid input.
    let _: Config = toml::Value::Table(table.clone())
        .try_into()
        .map_err(|error: toml::de::Error| config_error(path, error))?;
    Ok(table)
}

fn merge(base: &mut toml::Table, over: toml::Table) {
    for (key, value) in over {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(base)), toml::Value::Table(over)) => merge(base, over),
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

impl Config {
    pub fn load(paths: &Paths) -> Result<Self, CoreError> {
        let shared = paths.config_dir.join("config.toml");
        let local = paths.config_dir.join("config.local.toml");
        let mut table = read_table(&shared)?;
        merge(&mut table, read_table(&local)?);
        toml::Value::Table(table)
            .try_into()
            .map_err(|error: toml::de::Error| CoreError::Config {
                path: format!("{} + {}", shared.display(), local.display()),
                message: error.to_string(),
            })
    }
}

pub fn expand_home(s: &str, home: &Path) -> PathBuf {
    match s.strip_prefix("~/") {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None if s == "~" => home.to_path_buf(),
        None => PathBuf::from(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CoreError, Env, Instance, Paths};
    use std::path::{Path, PathBuf};

    fn paths_in(dir: &Path) -> Paths {
        struct E(String);
        impl Env for E {
            fn var(&self, key: &str) -> Option<String> {
                (key == "HOME" || key == "XDG_RUNTIME_DIR").then(|| self.0.clone())
            }
            fn uid(&self) -> u32 {
                1000
            }
        }
        Paths::resolve(
            Instance::parse("test-config").expect("parse fixture instance"),
            &E(dir.display().to_string()),
        )
        .expect("parse fixture instance")
    }

    fn write(p: &Paths, file: &str, text: &str) {
        std::fs::create_dir_all(&p.config_dir).expect("create fixture directory");
        std::fs::write(p.config_dir.join(file), text).expect("write fixture file");
    }

    #[test]
    fn defaults_when_files_are_missing_or_empty() {
        let t = tempfile::tempdir().expect("create fixture directory");
        let p = paths_in(t.path());
        let c = Config::load(&p).expect("load fixture config");
        assert_eq!(c.projects.roots, ["~/projects"]);
        assert_eq!(c.pc.name, None);
        assert_eq!(c.pc.windows_root, None);
        assert!(!p.config_dir.exists());
        write(&p, "config.toml", "[projects]\n[pc]\n");
        write(&p, "config.local.toml", "");
        assert_eq!(Config::load(&p).expect("load fixture config"), c);
    }

    #[test]
    fn local_overrides_keys_and_preserves_sibling_defaults() {
        let t = tempfile::tempdir().expect("create fixture directory");
        let p = paths_in(t.path());
        write(
            &p,
            "config.toml",
            "[projects]\nroots = ['~/a', '~/b']\n[pc]\nname = 'shared'\nwindows_root = 'C:\\dev'\n",
        );
        write(&p, "config.local.toml", "[pc]\nname = 'desk'\n");
        let c = Config::load(&p).expect("load fixture config");
        assert_eq!(c.projects.roots, ["~/a", "~/b"]);
        assert_eq!(c.pc.name.as_deref(), Some("desk"));
        assert_eq!(c.pc.windows_root.as_deref(), Some("C:\\dev"));
    }

    #[test]
    fn arrays_are_replaced_including_empty_arrays() {
        let t = tempfile::tempdir().expect("create fixture directory");
        let p = paths_in(t.path());
        write(&p, "config.toml", "[projects]\nroots = ['a', 'b']\n");
        for (input, expected) in [("['c']", vec!["c"]), ("[]", vec![])] {
            write(
                &p,
                "config.local.toml",
                &format!("[projects]\nroots = {input}\n"),
            );
            assert_eq!(
                Config::load(&p)
                    .expect("load fixture config")
                    .projects
                    .roots,
                expected
            );
        }
    }

    #[test]
    fn local_alone_is_supported() {
        let t = tempfile::tempdir().expect("create fixture directory");
        let p = paths_in(t.path());
        write(&p, "config.local.toml", "[pc]\nwindows_root = 'D:\\work'\n");
        let c = Config::load(&p).expect("load fixture config");
        assert_eq!(c.projects.roots, ["~/projects"]);
        assert_eq!(c.pc.name, None);
        assert_eq!(c.pc.windows_root.as_deref(), Some("D:\\work"));
    }

    #[test]
    fn invalid_inputs_report_the_exact_source_file_even_when_overridden() {
        let invalid = [
            "unexpected = 1",
            "[projects]\nroot = []",
            "[pc]\nnmae = 'typo'",
            "projects = 1",
            "[projects]\nroots = 'text'",
            "[projects]\nroots = [1]",
            "pc = []",
            "[pc]\nname = 123",
            "[pc]\nwindows_root = false",
            "[pc",
        ];
        for file in ["config.toml", "config.local.toml"] {
            for input in invalid {
                let t = tempfile::tempdir().expect("create fixture directory");
                let p = paths_in(t.path());
                write(
                    &p,
                    "config.local.toml",
                    "[projects]\nroots = []\n[pc]\nname = 'valid'\nwindows_root = 'valid'\n",
                );
                write(&p, file, input);
                match Config::load(&p).expect_err(input) {
                    CoreError::Config { path, message } => {
                        assert_eq!(path, p.config_dir.join(file).display().to_string());
                        assert!(!message.is_empty(), "{input}");
                    }
                    e => panic!("unexpected error for {input}: {e}"),
                }
            }
        }
    }

    #[test]
    fn read_errors_retain_path_and_io_source() {
        for file in ["config.toml", "config.local.toml"] {
            let t = tempfile::tempdir().expect("create fixture directory");
            let p = paths_in(t.path());
            std::fs::create_dir_all(p.config_dir.join(file)).expect("create fixture directory");
            match Config::load(&p).expect_err("fixture operation must fail") {
                CoreError::Io { what, source } => {
                    assert_eq!(what, format!("read {}", p.config_dir.join(file).display()));
                    assert_eq!(source.kind(), std::io::ErrorKind::IsADirectory);
                }
                e => panic!("unexpected error: {e}"),
            }
        }
    }

    #[test]
    fn invalid_utf8_is_a_read_error_with_source_path() {
        let t = tempfile::tempdir().expect("create fixture directory");
        let p = paths_in(t.path());
        std::fs::create_dir_all(&p.config_dir).expect("create fixture directory");
        let path = p.config_dir.join("config.toml");
        std::fs::write(&path, [0xff]).expect("write fixture file");
        match Config::load(&p).expect_err("fixture operation must fail") {
            CoreError::Io { what, source } => {
                assert_eq!(what, format!("read {}", path.display()));
                assert_eq!(source.kind(), std::io::ErrorKind::InvalidData);
            }
            e => panic!("unexpected error: {e}"),
        }
    }

    #[test]
    fn home_expansion_only_changes_bare_or_slash_prefixed_tilde() {
        let t = tempfile::tempdir().expect("create fixture directory");
        let home = t.path();
        assert_eq!(expand_home("~", home), home);
        assert_eq!(expand_home("~/projects", home), home.join("projects"));
        assert_eq!(expand_home("~/", home), home);
        assert_eq!(expand_home("~//projects", home), home.join("projects"));
        assert_eq!(expand_home("~///projects", home), home.join("projects"));
        assert_eq!(expand_home("~//", home), home);
        for input in [
            "",
            "/absolute",
            "relative",
            "~other/a",
            "a/~/b",
            "~\\projects",
        ] {
            assert_eq!(expand_home(input, home), PathBuf::from(input));
        }
    }
    #[test]
    fn terminal_config_accepts_profiles_and_rejects_non_table_sections() {
        let config =
            toml::from_str::<Config>("[[terminal.profiles]]\nname = 'Nu'\ncommand = 'nu'\n");
        assert!(
            config.is_ok(),
            "terminal profile config must deserialize: {config:?}"
        );
        assert!(toml::from_str::<Config>("terminal = ['bad']").is_err());
    }
    #[test]
    fn terminal_profiles_override_as_a_list_and_reject_positional_tables() {
        let temp = tempfile::tempdir().expect("fixture home");
        let paths = paths_in(temp.path());
        write(
            &paths,
            "config.toml",
            "[[terminal.profiles]]\nname='one'\ncommand='bash -l'\n",
        );
        write(
            &paths,
            "config.local.toml",
            "[[terminal.profiles]]\nname='two'\ncommand='nu'\n",
        );
        let config = Config::load(&paths).expect("profiles load");
        assert_eq!(
            config.terminal.profiles,
            vec![TerminalProfile {
                name: "two".into(),
                command: "nu".into()
            }]
        );
        write(
            &paths,
            "config.local.toml",
            "[terminal]\nprofiles=[['name','command']]\n",
        );
        assert!(Config::load(&paths).is_err());
    }
    #[test]
    fn mcp_write_defaults_false_and_local_can_disable_shared_permission() {
        let temp = tempfile::tempdir().expect("owned home");
        let paths = paths_in(temp.path());
        assert!(!Config::load(&paths).unwrap().mcp.write);
        write(&paths, "config.toml", "[mcp]\nwrite=true\n");
        assert!(Config::load(&paths).unwrap().mcp.write);
        write(&paths, "config.local.toml", "[mcp]\nwrite=false\n");
        assert!(!Config::load(&paths).unwrap().mcp.write);
    }

    #[test]
    fn invalid_mcp_sections_are_rejected_before_local_overrides() {
        for file in ["config.toml", "config.local.toml"] {
            for input in [
                "mcp=[]",
                "mcp=[true]",
                "mcp=true",
                "[mcp]\nwrite='true'",
                "[mcp]\nunknown=true",
            ] {
                let temp = tempfile::tempdir().expect("owned home");
                let paths = paths_in(temp.path());
                write(&paths, "config.local.toml", "[mcp]\nwrite=false\n");
                write(&paths, file, input);
                assert!(
                    matches!(Config::load(&paths), Err(CoreError::Config { path, .. }) if path == paths.config_dir.join(file).display().to_string()),
                    "{input}"
                );
            }
        }
    }

    #[test]
    fn agents_sections_are_strict_and_profiles_override_as_a_list() {
        let temp = tempfile::tempdir().expect("owned home");
        let paths = paths_in(temp.path());
        write(
            &paths,
            "config.toml",
            "[agents]\ncleanup_after_close=true\n[[agents.profiles]]\nid='one'\n",
        );
        write(
            &paths,
            "config.local.toml",
            "[[agents.profiles]]\nid='two'\n",
        );
        let config = Config::load(&paths).expect("agents config");
        assert_eq!(config.agents.cleanup_after_close, Some(true));
        assert_eq!(config.agents.profiles.len(), 1);
        assert_eq!(
            config.agents.profiles[0]
                .get("id")
                .and_then(toml::Value::as_str),
            Some("two")
        );
        for input in [
            "agents=[]",
            "[agents]\nprofiles=[['id']]",
            "[agents]\nprofiles=[1]",
            "[agents]\nprofiles=1",
            "[agents]\ncleanup_after_close='yes'",
            "[agents]\nunknown=true",
        ] {
            write(&paths, "config.toml", input);
            assert!(
                matches!(Config::load(&paths),Err(CoreError::Config { path, .. }) if path == paths.config_dir.join("config.toml").display().to_string()),
                "{input}"
            );
        }
    }
}
