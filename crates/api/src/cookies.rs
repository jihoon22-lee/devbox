//! 컬렉션 키별 쿠키 jar. `<data>/api-cookies/<sha256(key)>.json`(0600)에 둔다.
//! 도메인·경로·Secure 규칙은 `cookie_store`가 맡는다.
use crate::{ApiError, CookieInfo, CookieList};
use cookie_store::CookieStore;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub(crate) struct Jars {
    dir: PathBuf,
    open: Mutex<HashMap<String, Arc<Jar>>>,
}

pub(crate) struct Jar {
    path: PathBuf,
    store: Mutex<CookieStore>,
}

fn key_name(key: &str) -> Result<String, ApiError> {
    if key.is_empty() || key.len() > 1024 || key.contains('\0') {
        return Err(ApiError::input("cookieJar", "invalid"));
    }
    Ok(data_encoding::HEXLOWER.encode(&Sha256::digest(key.as_bytes())))
}

impl Jars {
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            open: Mutex::default(),
        }
    }

    pub(crate) fn jar(&self, key: &str) -> Result<Arc<Jar>, ApiError> {
        let name = key_name(key)?;
        let mut open = self
            .open
            .lock()
            .map_err(|_| ApiError::input("cookieJar", "busy"))?;
        if let Some(jar) = open.get(&name) {
            return Ok(jar.clone());
        }
        let path = self.dir.join(format!("{name}.json"));
        let store = std::fs::File::open(&path)
            .ok()
            .and_then(|f| cookie_store::serde::json::load_all(std::io::BufReader::new(f)).ok())
            .unwrap_or_default();
        let jar = Arc::new(Jar {
            path,
            store: Mutex::new(store),
        });
        open.insert(name, jar.clone());
        Ok(jar)
    }

    pub(crate) fn list(&self, key: &str) -> Result<CookieList, ApiError> {
        let jar = self.jar(key)?;
        let store = jar.lock();
        let cookies = store
            .iter_unexpired()
            .map(|c| CookieInfo {
                name: c.name().to_owned(),
                value: c.value().to_owned(),
                domain: c
                    .domain
                    .as_cow()
                    .map(|d| d.into_owned())
                    .unwrap_or_default(),
                path: c.path.as_ref().to_owned(),
                secure: c.secure().unwrap_or(false),
                http_only: c.http_only().unwrap_or(false),
                expires_ms: match &c.expires {
                    cookie_store::CookieExpiration::AtUtc(t) => {
                        Some((t.unix_timestamp_nanos() / 1_000_000) as i64)
                    }
                    cookie_store::CookieExpiration::SessionEnd => None,
                },
            })
            .collect();
        Ok(CookieList { cookies })
    }

    pub(crate) fn clear(&self, key: &str) -> Result<(), ApiError> {
        let jar = self.jar(key)?;
        jar.lock().clear();
        match std::fs::remove_file(&jar.path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(ApiError::input("cookieJar", "unavailable"))
            }
            _ => Ok(()),
        }
    }
}

impl Jar {
    fn lock(&self) -> std::sync::MutexGuard<'_, CookieStore> {
        self.store.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// 이 URL에 보낼 jar 쿠키(도메인·경로·Secure 규칙 적용).
    pub(crate) fn values(&self, url: &url::Url) -> Vec<(String, String)> {
        self.lock()
            .get_request_values(url)
            .map(|(n, v)| (n.to_owned(), v.to_owned()))
            .collect()
    }

    pub(crate) fn store(&self, url: &url::Url, headers: &reqwest::header::HeaderMap) {
        let mut store = self.lock();
        for value in headers.get_all(reqwest::header::SET_COOKIE) {
            if let Ok(text) = value.to_str() {
                let _ = store.parse(text, url);
            }
        }
    }

    /// 원자적 교체(0600). 실패는 송신 결과를 바꾸지 않는다.
    pub(crate) fn save(&self) -> std::io::Result<()> {
        let mut bytes = Vec::new();
        cookie_store::serde::json::save_incl_expired_and_nonpersistent(&self.lock(), &mut bytes)
            .map_err(|_| std::io::Error::other("serialize cookie jar"))?;
        let dir = self
            .path
            .parent()
            .ok_or_else(|| std::io::Error::other("jar path"))?;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        let temp = self
            .path
            .with_extension(format!("tmp-{}", uuid::Uuid::new_v4().simple()));
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(&temp, &self.path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result
    }
}

/// 요청 Cookie 행을 한 헤더로 순서대로 합친다. jar와 이름이 겹치면 행이 우선한다.
pub(crate) fn header(rows: &[(String, String)], jar: &[(String, String)]) -> Option<String> {
    let mut parts: Vec<String> = jar
        .iter()
        .filter(|(name, _)| !rows.iter().any(|(row, _)| row == name))
        .map(|(n, v)| format!("{n}={v}"))
        .collect();
    parts.extend(rows.iter().map(|(n, v)| format!("{n}={v}")));
    (!parts.is_empty()).then(|| parts.join("; "))
}
