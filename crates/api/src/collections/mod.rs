//! 컬렉션 파일 읽기·쓰기·감시(S5-3). 스키마는 화면 TS 소유이며 저장 bytes는 재작성하지 않는다.
//! S5-6 raw 본문은 쓰기 전에 비밀 값과 잘못된 인코딩을 보수적으로 거부한다.
//! 쓰기는 등록 프로젝트의 `.devbox/api`와 이 채널의 `Paths.config_dir/api` 안에서만 한다.
mod files;
pub mod model;
mod reviews;
mod storage_guard;
mod watch;

use crate::{ApiError, R, store};
use devbox_core::{Db, Hub, Paths};
use devbox_protocol::{Done, Fail, Internal};
use files::{Condition, Failure, Root};
use model::*;
use std::sync::Arc;

pub struct CollectionsService {
    db: Db,
    paths: Paths,
    projects: Arc<devbox_projects::ProjectsService>,
    watches: watch::Watches,
    /// 같은 데몬 안의 쓰기·삭제를 줄 세운다(digest 확인과 rename 사이에 다른 쓰기가 끼지 않게).
    writes: tokio::sync::Mutex<()>,
}

/// 풀린 루트: 파일 시스템 위치와 검토 표 키(개인은 `None`, 검토 불필요).
struct Resolved {
    root: Root,
    review_key: Option<String>,
    watch_key: String,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn failed(failure: Failure) -> Fail<ApiError> {
    match failure {
        Failure::Domain(error) => error.into(),
        Failure::Io(error) => Fail::Internal(format!("collection file: {}", error.kind())),
    }
}

/// 파일 작업은 blocking 스레드에서 한다.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> files::F<T> + Send + 'static) -> R<T> {
    tokio::task::spawn_blocking(f)
        .await
        .internal("collection file task")?
        .map_err(failed)
}

impl CollectionsService {
    pub fn new(
        db: Db,
        hub: Arc<Hub>,
        paths: Paths,
        projects: Arc<devbox_projects::ProjectsService>,
    ) -> Self {
        Self {
            db,
            paths,
            projects,
            watches: watch::Watches::new(hub),
            writes: tokio::sync::Mutex::new(()),
        }
    }

    async fn resolve(&self, root: &CollectionRoot) -> R<Resolved> {
        match root {
            CollectionRoot::Personal => Ok(Resolved {
                root: Root {
                    anchor: self.paths.config_dir.clone(),
                    below: &["api"],
                    private: true,
                },
                review_key: None,
                watch_key: "personal".into(),
            }),
            CollectionRoot::Project { project_id } => {
                let project = self
                    .projects
                    .get(project_id)
                    .await
                    .map_err(|_| Fail::Internal("look up project".into()))?
                    .ok_or(ApiError::ProjectNotFound)?;
                Ok(Resolved {
                    root: Root {
                        anchor: project.path.into(),
                        below: &[".devbox", "api"],
                        private: false,
                    },
                    review_key: Some(project.id.clone()),
                    watch_key: format!("project:{}", project.id),
                })
            }
        }
    }

    fn watch(&self, root: &CollectionRoot, resolved: Resolved) {
        if let Some(dir) = files::existing_dir(&resolved.root) {
            self.watches.ensure(resolved.watch_key, root.clone(), dir);
        }
    }

    async fn reviewed_digests(
        &self,
        key: &Option<String>,
    ) -> R<std::collections::BTreeMap<String, String>> {
        let Some(key) = key.clone() else {
            return Ok(Default::default());
        };
        self.db
            .read(move |tx| store::reviewed_all(tx, &key))
            .await
            .internal("read collection reviews")
    }

    pub async fn list(&self, p: CollectionsQuery) -> R<CollectionList> {
        let resolved = self.resolve(&p.root).await?;
        let reviewed = self.reviewed_digests(&resolved.review_key).await?;
        let personal = resolved.review_key.is_none();
        let dir = resolved.root.clone();
        let (listed, truncated) = blocking(move || files::list(&dir)).await?;
        let items = listed
            .into_iter()
            .map(|item| {
                let ok = personal
                    || !item.risky
                    || (item.digest.is_some() && reviewed.get(&item.path) == item.digest.as_ref());
                CollectionFile {
                    path: item.path,
                    size: item.size,
                    digest: item.digest,
                    reviewed: ok,
                    unreadable: item.unreadable,
                }
            })
            .collect();
        self.watch(&p.root, resolved);
        Ok(CollectionList {
            root: p.root,
            items,
            truncated,
        })
    }

    pub async fn read(&self, p: FileRef) -> R<FileText> {
        let resolved = self.resolve(&p.root).await?;
        let root = resolved.root.clone();
        let path = p.path.clone();
        let bytes = blocking(move || files::read(&root, &path)).await?;
        let digest = files::digest(&bytes);
        let reviewed = match &resolved.review_key {
            None => true,
            Some(_) if !reviews::needs_review(&bytes) => true,
            Some(key) => {
                let (key, path) = (key.clone(), p.path.clone());
                self.db
                    .read(move |tx| store::reviewed_digest(tx, &key, &path))
                    .await
                    .internal("read collection review")?
                    .as_deref()
                    == Some(digest.as_str())
            }
        };
        let text = String::from_utf8(bytes).map_err(|_| ApiError::CollectionPathInvalid)?;
        self.watch(&p.root, resolved);
        Ok(FileText {
            text,
            digest,
            reviewed,
        })
    }

    /// 쓰기는 검토를 기록하지 않는다. 가져오기처럼 사용자가 내용을 보지 않은 쓰기도 같은 경로라서,
    /// 검토는 화면이 내용을 보인 뒤 `review`로 그 digest를 명시해 기록한다(07b 34행).
    pub async fn write(&self, p: WriteFile) -> R<Written> {
        let resolved = self.resolve(&p.root).await?;
        let _guard = self.writes.lock().await;
        let root = resolved.root.clone();
        let (path, text, expect) = (p.path.clone(), p.text, p.expect);
        let digest = blocking(move || {
            let condition = match &expect {
                CollectionExpect::Digest { digest } => Condition::Digest(digest),
                CollectionExpect::Absent => Condition::Absent,
            };
            files::write(&root, &path, text.as_bytes(), condition)
        })
        .await?;
        let rev = self.db.rev();
        self.watch(&p.root, resolved);
        Ok(Written {
            digest,
            rev: rev.to_string(),
        })
    }

    pub async fn delete(&self, p: DeleteFile) -> R<Done> {
        let resolved = self.resolve(&p.root).await?;
        let _guard = self.writes.lock().await;
        let root = resolved.root.clone();
        let (path, expected) = (p.path.clone(), p.digest);
        blocking(move || files::delete(&root, &path, &expected)).await?;
        let rev = match resolved.review_key {
            Some(key) => {
                let path = p.path;
                self.db
                    .write(move |tx| store::forget(tx, &key, &path))
                    .await
                    .internal("forget collection review")?
                    .1
            }
            None => self.db.rev(),
        };
        Ok(Done {
            rev: rev.to_string(),
        })
    }

    /// (루트, 경로, digest)를 이 PC에서 검토했다고 기록한다. 개인 루트는 기록할 것이 없다.
    pub async fn review(&self, p: ReviewFile) -> R<Done> {
        files::segments(&p.path)?;
        let resolved = self.resolve(&p.root).await?;
        let Some(key) = resolved.review_key else {
            return Ok(Done {
                rev: self.db.rev().to_string(),
            });
        };
        if p.digest.len() != 64 || !p.digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ApiError::input("digest", "format").into());
        }
        let (_, rev) = self
            .db
            .write(move |tx| store::record(tx, &key, &p.path, &p.digest, now_ms()))
            .await
            .internal("record collection review")?;
        Ok(Done {
            rev: rev.to_string(),
        })
    }

    /// MCP 연결 출처 확인(S5-5). 한 번 읽은 bytes로 digest와 이 PC의 검토를 확인하고 그 bytes를 돌려준다.
    /// 호출자는 같은 bytes에서 실행할 명령이 요청과 같은지 본다(확인과 실행 사이에 파일을 다시 읽지 않는다).
    pub(crate) async fn verified(
        &self,
        root: &CollectionRoot,
        path: &str,
        digest: &str,
    ) -> R<Vec<u8>> {
        let resolved = self.resolve(root).await?;
        let (dir, file) = (resolved.root.clone(), path.to_owned());
        let bytes = blocking(move || files::read(&dir, &file)).await?;
        let current = files::digest(&bytes);
        if current != digest {
            return Err(ApiError::CollectionConflict {
                digest: Some(current),
            }
            .into());
        }
        if let Some(key) = resolved.review_key
            && reviews::needs_review(&bytes)
        {
            let file = path.to_owned();
            let reviewed = self
                .db
                .read(move |tx| store::reviewed_digest(tx, &key, &file))
                .await
                .internal("read collection review")?;
            if reviewed.as_deref() != Some(digest) {
                return Err(ApiError::ReviewRequired.into());
            }
        }
        Ok(bytes)
    }
}
