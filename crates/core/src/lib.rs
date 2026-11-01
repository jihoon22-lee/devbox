//! 데몬 공용 기반: 인스턴스·경로·설정·DB·이벤트 허브·로그.
pub mod config;
pub mod hub;
pub mod input;
pub mod log;
pub mod notify;
pub mod paths;
#[cfg(unix)]
pub mod systemd;
pub use hub::{Hub, HubMessage, HubReceiver};

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("{program} timed out")]
    CommandTimedOut { program: String },
    #[error("{program} could not run")]
    CommandUnavailable { program: String },
    #[error("{program} exited unsuccessfully")]
    CommandFailed { program: String },
    #[error("invalid systemd response")]
    SystemdResponse,
    #[error("{0}")]
    UnitPath(&'static str),
    #[error("invalid instance name {0:?}")]
    InvalidInstance(String),
    #[error("HOME is not set to an absolute path")]
    NoHome,
    #[error("{what}: {source}")]
    Io {
        what: String,
        #[source]
        source: std::io::Error,
    },
    #[error("config {path}: {message}")]
    Config { path: String, message: String },
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("invalid database input: {0}")]
    DbInput(String),
    #[error("database worker stopped")]
    DbClosed,
}

pub fn io_err(what: impl Into<String>) -> impl FnOnce(std::io::Error) -> CoreError {
    let what = what.into();
    move |source| CoreError::Io { what, source }
}
pub use config::{Config, expand_home};
pub use paths::{Env, Instance, OsEnv, Paths};

pub mod db;
pub use db::{Db, Migration, current_rev};
pub mod index_db;
pub use index_db::IndexDb;

/// Quote one shell argument, including literal quotes, whitespace and newlines.
pub fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Whether `tmux -V` output (`tmux 3.4`, `tmux 3.5a`, `tmux next-3.6`) names `want` or newer.
/// Anything else, such as `tmux master`, does not. Shared by the environment check and the
/// terminal, so both read a version the same way.
pub fn tmux_at_least(v: &str, want: (u32, u32)) -> bool {
    let Some(v) = v.strip_prefix("tmux ") else {
        return false;
    };
    let v = v.strip_prefix("next-").unwrap_or(v);
    let Some((major, rest)) = v.split_once('.') else {
        return false;
    };
    let len = rest.bytes().take_while(u8::is_ascii_digit).count();
    let (minor, suffix) = rest.split_at(len);
    if !suffix.bytes().all(|b| b.is_ascii_alphabetic()) {
        return false;
    }
    match (major.parse::<u32>(), minor.parse::<u32>()) {
        (Ok(a), Ok(b)) => (a, b) >= want,
        _ => false,
    }
}

#[cfg(test)]
mod shell_tests {
    #[test]
    fn sh_quote_preserves_literal_shell_characters() {
        assert_eq!(super::sh_quote("a b"), "'a b'");
        assert_eq!(super::sh_quote("it's"), "'it'\\''s'");
        assert_eq!(super::sh_quote("$HOME\nnext"), "'$HOME\nnext'");
    }
}

mod sql_enum;

pub mod run_log;
