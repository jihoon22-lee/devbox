//! 데몬 공용 기반: 인스턴스·경로·설정·DB·이벤트 허브·로그.
pub mod paths;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
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
    #[error("database worker stopped")]
    DbClosed,
}

pub fn io_err(what: impl Into<String>) -> impl FnOnce(std::io::Error) -> CoreError {
    let what = what.into();
    move |source| CoreError::Io { what, source }
}
pub use paths::{Env, Instance, OsEnv, Paths};
