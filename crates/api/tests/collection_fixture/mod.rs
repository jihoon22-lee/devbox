//! 컬렉션 시험 공용 fixture: 소유 임시 HOME의 Paths·DB·프로젝트 등록과 `CollectionsService`.
#![allow(dead_code)]
use devbox_api::*;
use devbox_core::{Db, Env, Hub, Instance, Paths};
use devbox_protocol::Fail;
use std::path::Path;
use std::sync::Arc;

pub struct Home(String);
impl Env for Home {
    fn var(&self, k: &str) -> Option<String> {
        (k == "HOME").then(|| self.0.clone())
    }
    fn uid(&self) -> u32 {
        1000
    }
}

pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub paths: Paths,
    pub hub: Arc<Hub>,
    pub projects: Arc<devbox_projects::ProjectsService>,
    pub service: CollectionsService,
}

pub fn instance_paths(home: &Path, instance: &str) -> Paths {
    let paths = Paths::resolve(
        Instance::parse(instance).expect("instance"),
        &Home(home.display().to_string()),
    )
    .expect("paths");
    paths.ensure_dirs().expect("dirs");
    paths
}

impl Fixture {
    pub async fn new() -> Self {
        let dir = tempfile::tempdir().expect("owned fixture");
        let suffix = &uuid::Uuid::new_v4().simple().to_string()[..8];
        let paths = instance_paths(dir.path(), &format!("test-{suffix}"));
        Self::open(dir, paths).await
    }
    pub async fn open(dir: tempfile::TempDir, paths: Paths) -> Self {
        let migrations = [devbox_projects::MIGRATIONS, devbox_api::MIGRATIONS].concat();
        let db = Db::open(&paths.db_path, &paths.backups_dir, "test", &migrations).expect("db");
        let hub = Arc::new(Hub::new());
        let projects = Arc::new(devbox_projects::ProjectsService::new(
            db.clone(),
            hub.clone(),
            dir.path().into(),
            vec![],
        ));
        let service = CollectionsService::new(db, hub.clone(), paths.clone(), projects.clone());
        Self {
            dir,
            paths,
            hub,
            projects,
            service,
        }
    }
    /// 소유 임시 폴더 안의 프로젝트를 등록하고 (루트, 프로젝트 폴더)를 돌려준다.
    pub async fn project(&self, name: &str) -> (CollectionRoot, std::path::PathBuf) {
        let path = self.dir.path().join(name);
        std::fs::create_dir_all(&path).expect("project dir");
        let project = self
            .projects
            .add(devbox_projects::AddProject {
                path: path.display().to_string(),
            })
            .await
            .expect("register project");
        (
            CollectionRoot::Project {
                project_id: project.id,
            },
            std::path::PathBuf::from(project.path),
        )
    }
    pub async fn write(
        &self,
        root: &CollectionRoot,
        path: &str,
        text: &str,
        expect: CollectionExpect,
    ) -> Result<Written, Fail<ApiError>> {
        self.service
            .write(WriteFile {
                root: root.clone(),
                path: path.into(),
                text: text.into(),
                expect,
            })
            .await
    }
    pub async fn read(
        &self,
        root: &CollectionRoot,
        path: &str,
    ) -> Result<FileText, Fail<ApiError>> {
        self.service
            .read(FileRef {
                root: root.clone(),
                path: path.into(),
            })
            .await
    }
    pub async fn list(&self, root: &CollectionRoot) -> CollectionList {
        self.service
            .list(CollectionsQuery { root: root.clone() })
            .await
            .expect("list collection")
    }
}

pub fn domain(result: Result<impl std::fmt::Debug, Fail<ApiError>>) -> ApiError {
    match result {
        Err(Fail::Domain(error)) => error,
        other => panic!("expected a domain error, got {other:?}"),
    }
}

pub fn sha(text: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    data_encoding::HEXLOWER.encode(&Sha256::digest(text))
}

pub const REQUEST: &str = "{\n  \"version\": 1,\n  \"name\": \"me\"\n}\n";

impl Fixture {
    pub async fn project_missing(&self) -> ApiError {
        let root = CollectionRoot::Project {
            project_id: "missing".into(),
        };
        domain(self.service.list(CollectionsQuery { root }).await)
    }
}
