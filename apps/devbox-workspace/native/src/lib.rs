//! Private, bounded pipe protocol between Workspace and its packaged Linux helper.
//! This is not a listener or an API available to other renderer/product sessions.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, Read, Write};

pub const VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_ROOTS: usize = 16;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub session_id: String,
    pub request_id: String,
    pub sequence: u64,
    pub budget_ms: u32,
    pub method: String,
    pub root_token: Option<String>,
    pub args: Value,
}
impl Request {
    pub fn validate(&self, session: &str) -> Result<(), &'static str> {
        if self.sequence == 0
            || self.version != VERSION
            || self.session_id != session
            || !token(&self.session_id)
            || !token(&self.request_id)
            || self.budget_ms == 0
            || self.budget_ms > 30_000
            || self.method.is_empty()
            || self.method.len() > 80
            || !self
                .method
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c == b'_')
            || self.root_token.as_ref().is_some_and(|value| !token(value))
        {
            return Err("wsl_protocol_invalid");
        }
        Ok(())
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Response {
    pub version: u32,
    pub session_id: String,
    pub request_id: String,
    pub sequence: u64,
    pub result: Result<Value, String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObjectStamp {
    pub scope: String,
    pub object: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RootReport {
    pub token: String,
    pub root: String,
    pub root_object: ObjectStamp,
    pub repository_object: Option<ObjectStamp>,
    pub git_directory: Option<String>,
    pub common_directory: Option<String>,
}
impl RootReport {
    pub fn validate(&self) -> Result<(), &'static str> {
        let path = |value: &str| {
            value.starts_with('/')
                && value != "/"
                && value.len() <= 32768
                && !value.chars().any(char::is_control)
                && value
                    .split('/')
                    .skip(1)
                    .all(|part| !matches!(part, "" | "." | ".."))
        };
        let stamp = |value: &ObjectStamp| {
            [&value.scope, &value.object].into_iter().all(|part| {
                part.len() == 32
                    && part
                        .bytes()
                        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            })
        };
        if !token(&self.token)
            || !path(&self.root)
            || !stamp(&self.root_object)
            || self
                .repository_object
                .as_ref()
                .is_some_and(|value| !stamp(value))
            || self
                .git_directory
                .as_ref()
                .is_some_and(|value| !path(value))
            || self
                .common_directory
                .as_ref()
                .is_some_and(|value| !path(value))
            || self.repository_object.is_some() != self.git_directory.is_some()
            || self.repository_object.is_some() != self.common_directory.is_some()
        {
            return Err("wsl_protocol_invalid");
        }
        Ok(())
    }
}
pub fn token(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok() && value.len() == 36
}

/// A partial prefix/body is an error, including EOF during a frame. Length is
/// checked before allocation. No shell strings or line-oriented escaping exist.
pub fn read_frame<R: Read, T: serde::de::DeserializeOwned>(input: &mut R) -> io::Result<Option<T>> {
    let mut cursor = control::FrameCursor::default();
    loop {
        let count = input.read(cursor.buffer())?;
        if count == 0 {
            return if cursor.is_empty() {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "partial frame",
                ))
            };
        }
        if let Some(body) = cursor.advance(count)? {
            return serde_json::from_slice(&body)
                .map(Some)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid protocol JSON"));
        }
    }
}
pub fn write_frame<W: Write, T: Serialize>(output: &mut W, value: &T) -> io::Result<()> {
    struct Bounded(Vec<u8>);
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > MAX_FRAME_BYTES.saturating_sub(self.0.len()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid frame length",
                ));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut body = Bounded(Vec::new());
    serde_json::to_writer(&mut body, value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid protocol JSON"))?;
    let body = body.0;
    if body.is_empty() || body.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid frame length",
        ));
    }
    output.write_all(&(body.len() as u32).to_le_bytes())?;
    output.write_all(&body)?;
    output.flush()
}

#[cfg(all(feature = "helper", target_os = "linux"))]
pub mod engine;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn root_reports_require_native_posix_objects_and_complete_repository_evidence() {
        let mut report = RootReport {
            token: uuid::Uuid::new_v4().to_string(),
            root: "/home/test/한글 project".into(),
            root_object: ObjectStamp {
                scope: "a".repeat(32),
                object: "b".repeat(32),
            },
            repository_object: None,
            git_directory: None,
            common_directory: None,
        };
        report.validate().unwrap();
        report.git_directory = Some("/home/test/.git".into());
        assert!(report.validate().is_err());
        report.common_directory = report.git_directory.clone();
        report.repository_object = Some(report.root_object.clone());
        report.validate().unwrap();
        report.root = "C:/different-target".into();
        assert!(report.validate().is_err());
        report.root = "/home/test/../escape".into();
        assert!(report.validate().is_err());
        report.root = "/home/test".into();
        report.root_object.scope = "unverified".into();
        assert!(report.validate().is_err());
    }
    #[test]
    fn truncated_unknown_and_oversized_frames_fail_before_dispatch() {
        for bytes in [
            vec![1, 0],
            vec![4, 0, 0, 0, b'{'],
            0_u32.to_le_bytes().to_vec(),
            ((MAX_FRAME_BYTES + 1) as u32).to_le_bytes().to_vec(),
        ] {
            assert!(read_frame::<_, Request>(&mut bytes.as_slice()).is_err());
        }
        assert!(read_frame::<_, Request>(&mut [].as_slice())
            .unwrap()
            .is_none());
        let session = uuid::Uuid::new_v4().to_string();
        let mut request = Request {
            version: VERSION,
            session_id: session.clone(),
            request_id: uuid::Uuid::new_v4().to_string(),
            sequence: 1,
            budget_ms: 5000,
            method: "observe_root".into(),
            root_token: None,
            args: serde_json::json!({"path":"/한글 project"}),
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &request).unwrap();
        let roundtrip: Request = read_frame(&mut bytes.as_slice()).unwrap().unwrap();
        assert!(roundtrip.validate(&session).is_ok());
        assert_eq!(roundtrip.args, request.args);
        assert!(request.validate(&uuid::Uuid::new_v4().to_string()).is_err());
        request.budget_ms = 30001;
        assert!(request.validate(&session).is_err());
        let mut unknown = serde_json::to_value(roundtrip).unwrap();
        unknown["command"] = serde_json::json!("ignored shell");
        assert!(serde_json::from_value::<Request>(unknown).is_err());
    }
}

#[cfg(feature = "files")]
pub mod files;
#[cfg(feature = "files")]
pub mod storage_paths;
#[cfg(feature = "files")]
pub mod windows_path;

#[cfg(all(feature = "helper", target_os = "linux"))]
pub mod linux_files;

#[cfg(feature = "files")]
pub mod manifest;
#[cfg(feature = "files")]
pub mod project_files;

#[cfg(all(feature = "helper", target_os = "linux"))]
mod definitions;

#[cfg(all(feature = "helper", target_os = "linux"))]
mod definition_write;

#[cfg(feature = "git")]
pub mod git_config;
#[cfg(feature = "git")]
pub mod git_files;
#[cfg(feature = "git")]
pub mod git_trust;

#[cfg(all(feature = "helper", target_os = "linux"))]
mod git_environment;

#[cfg(all(feature = "helper", target_os = "linux"))]
mod git_review;

pub mod control;
