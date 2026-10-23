//! S5-3 3b: 컬렉션 파일 읽기·쓰기. 데몬은 텍스트만 다루고 경로는 등록 프로젝트·개인 폴더 안만 쓴다.
#![cfg(unix)]
use collection_fixture::*;
use devbox_api::*;

mod collection_fixture;

#[tokio::test]
async fn write_rejects_stale_digest_and_preserves_bytes() {
    let f = Fixture::new().await;
    let (root, project) = f.project("app").await;
    let first = f
        .write(
            &root,
            "users/me.request.json",
            REQUEST,
            CollectionExpect::Absent,
        )
        .await
        .expect("create file");
    let read = f.read(&root, "users/me.request.json").await.expect("read");
    assert_eq!(read.text, REQUEST);
    assert_eq!(read.digest, first.digest);
    assert_eq!(read.digest, sha(REQUEST.as_bytes()));
    let file = project.join(".devbox/api/users/me.request.json");
    let outside = "{\"version\":1,\"name\":\"edited outside\"}";
    std::fs::write(&file, outside).expect("external edit");
    let stale = f
        .write(
            &root,
            "users/me.request.json",
            "{\"mine\":true}",
            CollectionExpect::Digest {
                digest: read.digest.clone(),
            },
        )
        .await;
    assert_eq!(
        domain(stale),
        ApiError::CollectionConflict {
            digest: Some(sha(outside.as_bytes()))
        }
    );
    assert_eq!(std::fs::read(&file).expect("bytes"), outside.as_bytes());
    let fresh = f
        .write(
            &root,
            "users/me.request.json",
            "{\"mine\":true}",
            CollectionExpect::Digest {
                digest: sha(outside.as_bytes()),
            },
        )
        .await
        .expect("write with current digest");
    assert_eq!(fresh.digest, sha(b"{\"mine\":true}"));
    assert_eq!(
        std::fs::read_to_string(&file).expect("text"),
        "{\"mine\":true}"
    );
    let leftovers: Vec<_> = std::fs::read_dir(file.parent().expect("parent"))
        .expect("dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert_eq!(leftovers, vec![std::ffi::OsString::from("me.request.json")]);
}

#[tokio::test]
async fn save_as_new_never_overwrites() {
    let f = Fixture::new().await;
    let (root, project) = f.project("app").await;
    f.write(&root, "a.request.json", REQUEST, CollectionExpect::Absent)
        .await
        .expect("create");
    let again = f
        .write(
            &root,
            "a.request.json",
            "{\"other\":1}",
            CollectionExpect::Absent,
        )
        .await;
    assert_eq!(
        domain(again),
        ApiError::CollectionConflict {
            digest: Some(sha(REQUEST.as_bytes()))
        }
    );
    assert_eq!(
        std::fs::read_to_string(project.join(".devbox/api/a.request.json")).expect("text"),
        REQUEST
    );
    let missing = f
        .write(
            &root,
            "gone.request.json",
            "{}",
            CollectionExpect::Digest { digest: sha(b"{}") },
        )
        .await;
    assert_eq!(
        domain(missing),
        ApiError::CollectionConflict { digest: None }
    );
    assert!(!project.join(".devbox/api/gone.request.json").exists());
}

#[tokio::test]
async fn case_collisions_are_rejected_for_new_files() {
    let f = Fixture::new().await;
    let (root, project) = f.project("app").await;
    f.write(
        &root,
        "Users/Me.request.json",
        REQUEST,
        CollectionExpect::Absent,
    )
    .await
    .expect("create");
    let collide = f
        .write(
            &root,
            "Users/me.REQUEST.json",
            "{}",
            CollectionExpect::Absent,
        )
        .await;
    assert_eq!(
        domain(collide),
        ApiError::CollectionConflict {
            digest: Some(sha(REQUEST.as_bytes()))
        }
    );
    let folder = f
        .write(
            &root,
            "users/other.request.json",
            "{}",
            CollectionExpect::Absent,
        )
        .await;
    assert_eq!(
        domain(folder),
        ApiError::CollectionConflict { digest: None }
    );
    let names: Vec<_> = std::fs::read_dir(project.join(".devbox/api"))
        .expect("dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("Users")]);
}

#[tokio::test]
async fn paths_outside_the_root_and_symlinks_are_rejected() {
    let f = Fixture::new().await;
    let (root, project) = f.project("app").await;
    let outside = f.dir.path().join("outside");
    std::fs::create_dir_all(&outside).expect("outside dir");
    for bad in [
        "../x.json",
        "a/../../x.json",
        "/etc/x.json",
        "a//b.json",
        "./a.json",
        "a.txt",
        "a\\b.json",
        "a\0b.json",
        "",
        ".devbox-tmp-x.json",
    ] {
        assert_eq!(
            domain(f.write(&root, bad, "{}", CollectionExpect::Absent).await),
            ApiError::CollectionPathInvalid,
            "{bad:?}"
        );
        assert_eq!(
            domain(f.read(&root, bad).await),
            ApiError::CollectionPathInvalid,
            "{bad:?}"
        );
    }
    f.write(&root, "ok.request.json", "{}", CollectionExpect::Absent)
        .await
        .expect("normal write");
    let api = project.join(".devbox/api");
    std::os::unix::fs::symlink(&outside, api.join("link")).expect("dir symlink");
    std::fs::write(outside.join("secret.json"), "{\"outside\":true}").expect("outside file");
    std::os::unix::fs::symlink(outside.join("secret.json"), api.join("file.json"))
        .expect("file symlink");
    assert_eq!(
        domain(
            f.write(&root, "link/new.json", "{}", CollectionExpect::Absent)
                .await
        ),
        ApiError::CollectionPathInvalid
    );
    assert_eq!(
        domain(f.read(&root, "link/secret.json").await),
        ApiError::CollectionPathInvalid
    );
    assert_eq!(
        domain(f.read(&root, "file.json").await),
        ApiError::CollectionPathInvalid
    );
    assert_eq!(
        domain(
            f.write(
                &root,
                "file.json",
                "{}",
                CollectionExpect::Digest {
                    digest: sha(b"{\"outside\":true}")
                }
            )
            .await
        ),
        ApiError::CollectionPathInvalid
    );
    assert!(!outside.join("new.json").exists());
    assert_eq!(
        std::fs::read_to_string(outside.join("secret.json")).expect("outside"),
        "{\"outside\":true}"
    );
    let listed: Vec<_> = f
        .list(&root)
        .await
        .items
        .into_iter()
        .map(|i| i.path)
        .collect();
    assert_eq!(listed, vec!["ok.request.json".to_owned()]);

    // `.devbox` 자체가 밖을 가리키면 그 아래 아무것도 만들지 않는다.
    let (other, other_path) = f.project("other").await;
    let target = f.dir.path().join("elsewhere");
    std::fs::create_dir_all(&target).expect("target");
    std::os::unix::fs::symlink(&target, other_path.join(".devbox")).expect("devbox symlink");
    assert_eq!(
        domain(
            f.write(&other, "a.json", "{}", CollectionExpect::Absent)
                .await
        ),
        ApiError::CollectionPathInvalid
    );
    assert_eq!(std::fs::read_dir(&target).expect("target").count(), 0);
}

#[tokio::test]
async fn personal_root_uses_the_channel_config_dir() {
    let dir = tempfile::tempdir().expect("owned fixture");
    let paths = instance_paths(dir.path(), "accept");
    assert!(paths.config_dir.ends_with("devbox-accept"));
    let f = Fixture::open(dir, paths).await;
    let prod = f.dir.path().join(".config/devbox");
    assert!(!prod.exists());
    let personal = CollectionRoot::Personal;
    assert!(f.list(&personal).await.items.is_empty());
    assert!(
        !f.paths.config_dir.join("api").exists(),
        "list must not create the folder"
    );
    f.write(
        &personal,
        "mine/a.request.json",
        REQUEST,
        CollectionExpect::Absent,
    )
    .await
    .expect("personal write");
    let api = f.paths.config_dir.join("api");
    assert_eq!(
        std::fs::read_to_string(api.join("mine/a.request.json")).expect("text"),
        REQUEST
    );
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&api).expect("meta").permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    assert!(!prod.exists(), "prod config folder must not be created");
    let list = f.list(&personal).await;
    assert_eq!(list.items.len(), 1);
    assert!(list.items[0].reviewed);
    assert_eq!(f.project_missing().await, ApiError::ProjectNotFound);
}

#[tokio::test]
async fn review_is_tied_to_the_file_digest() {
    let f = Fixture::new().await;
    let (root, project) = f.project("app").await;
    let plain = "{\"version\":1,\"request\":{\"url\":\"https://x.test\"}}";
    let secret = "{\"version\":1,\"request\":{\"headers\":[{\"name\":\"A\",\"value\":\"{{secret:TOKEN}}\"}]}}";
    let file = "{\"version\":1,\"request\":{\"body\":{\"parts\":[{\"kind\":\"file\",\"file_path\":\"/home/u/x\"}]}}}";
    let stdio = "{\"version\":1,\"server\":{\"transport\":\"stdio\",\"command\":\"node\"}}";
    let escaped = "{\"v\":\"\\u007b\\u007bsecret:T}}\"}";
    let binary = "{\"version\":1,\"request\":{\"body\":{\"kind\":\"binary\",\"path\":\"/home/u/.ssh/id_ed25519\"}}}";
    let tls = "{\"version\":1,\"request\":{\"tls\":{\"clientKeyPath\":\"/home/u/k.pem\"}}}";
    let api = project.join(".devbox/api");
    std::fs::create_dir_all(&api).expect("api dir");
    for (name, text) in [
        ("plain.json", plain),
        ("secret.json", secret),
        ("file.json", file),
        ("stdio.json", stdio),
        ("escaped.json", escaped),
        ("body.json", binary),
        ("tls.json", tls),
        ("broken.json", "{not json {{secret"),
    ] {
        std::fs::write(api.join(name), text).expect("fixture file");
    }
    std::fs::write(api.join("binary.json"), [0xff, 0xfe, 0x00]).expect("binary");
    let reviewed = |list: &CollectionList, path: &str| {
        list.items
            .iter()
            .find(|i| i.path == path)
            .unwrap_or_else(|| panic!("{path} listed"))
            .reviewed
    };
    let list = f.list(&root).await;
    assert!(reviewed(&list, "plain.json"));
    for path in [
        "secret.json",
        "file.json",
        "stdio.json",
        "escaped.json",
        "body.json",
        "tls.json",
        "broken.json",
        "binary.json",
    ] {
        assert!(!reviewed(&list, path), "{path} needs review");
    }
    f.service
        .review(ReviewFile {
            root: root.clone(),
            path: "secret.json".into(),
            digest: sha(secret.as_bytes()),
        })
        .await
        .expect("review");
    assert!(reviewed(&f.list(&root).await, "secret.json"));
    assert!(f.read(&root, "secret.json").await.expect("read").reviewed);
    // 밖에서 바뀌면(git pull) 다시 검토가 필요하다.
    let pulled = secret.replace("TOKEN", "OTHER");
    std::fs::write(api.join("secret.json"), &pulled).expect("pull");
    assert!(!reviewed(&f.list(&root).await, "secret.json"));
    // 쓰기는 검토를 기록하지 않는다. 화면 저장은 사용자가 본 내용이라 쓴 digest로 검토를 따로 기록한다.
    // (검토한 적 없는 새 내용으로 저장한다. 처음 내용은 위에서 검토한 digest라 그대로 검토된 것으로 보인다.)
    let edited = secret.replace("TOKEN", "SAVED");
    let saved = f
        .write(
            &root,
            "secret.json",
            &edited,
            CollectionExpect::Digest {
                digest: sha(pulled.as_bytes()),
            },
        )
        .await
        .expect("save");
    assert_eq!(saved.digest, sha(edited.as_bytes()));
    assert!(!reviewed(&f.list(&root).await, "secret.json"));
    f.service
        .review(ReviewFile {
            root: root.clone(),
            path: "secret.json".into(),
            digest: saved.digest.clone(),
        })
        .await
        .expect("review saved digest");
    assert!(reviewed(&f.list(&root).await, "secret.json"));
    // 개인 루트는 항상 검토된 것으로 본다.
    f.write(
        &CollectionRoot::Personal,
        "s.json",
        secret,
        CollectionExpect::Absent,
    )
    .await
    .expect("personal");
    assert!(reviewed(&f.list(&CollectionRoot::Personal).await, "s.json"));
}

/// 가져오기처럼 사용자가 내용을 보지 않은 쓰기가 검토를 대신하지 않는다(07b 34행).
/// 검토는 `api.collection_review`로 명시한 digest만 기록한다.
#[tokio::test]
async fn writing_a_project_file_does_not_record_a_review() {
    let f = Fixture::new().await;
    let (root, _) = f.project("app").await;
    let request = "{\"version\":1,\"request\":{\"url\":\"https://evil.test/\",\"headers\":[{\"name\":\"X-Key\",\"value\":\"{{secret:GH}}\"}]}}";
    let env = "{\"version\":1,\"name\":\"dev\",\"variables\":[{\"name\":\"x\",\"value\":\"{{secret:GH}}\"}]}";
    for (path, text) in [("imported.request.json", request), ("env/dev.json", env)] {
        let written = f
            .write(&root, path, text, CollectionExpect::Absent)
            .await
            .expect("import write");
        let read = f.read(&root, path).await.expect("read");
        assert!(!read.reviewed, "{path}: a write must not count as review");
        let listed = f.list(&root).await;
        let item = listed
            .items
            .iter()
            .find(|i| i.path == path)
            .expect("listed");
        assert!(!item.reviewed, "{path}: listed as unreviewed");
        f.service
            .review(ReviewFile {
                root: root.clone(),
                path: path.into(),
                digest: written.digest.clone(),
            })
            .await
            .expect("explicit review");
        assert!(f.read(&root, path).await.expect("read").reviewed, "{path}");
    }
}

#[tokio::test]
async fn collection_files_round_trip_without_losing_unknown_fields() {
    let f = Fixture::new().await;
    let (root, _) = f.project("app").await;
    // 데몬은 스키마를 해석하지 않고 텍스트를 그대로 둔다(모르는 필드·순서·공백·BOM 없는 UTF-8).
    let text = "{\n  \"x-team\": {\"owner\": \"한글\"},\n  \"version\": 1,\n  \"request\": {\"x-extra\": [1, 2],   \"url\": \"https://x.test\"}\n}";
    let written = f
        .write(&root, "t.request.json", text, CollectionExpect::Absent)
        .await
        .expect("write");
    let read = f.read(&root, "t.request.json").await.expect("read");
    assert_eq!(read.text, text);
    assert_eq!(read.digest, written.digest);
    let list = f.list(&root).await;
    assert_eq!(
        list.items[0].digest.as_deref(),
        Some(written.digest.as_str())
    );
    assert_eq!(list.items[0].size, text.len() as i64);
}

#[tokio::test]
async fn oversized_missing_and_deleted_files_fail_closed() {
    let f = Fixture::new().await;
    let (root, project) = f.project("app").await;
    let big = "x".repeat(1024 * 1024 + 1);
    assert_eq!(
        domain(
            f.write(&root, "big.json", &big, CollectionExpect::Absent)
                .await
        ),
        ApiError::CollectionTooLarge { limit: 1024 * 1024 }
    );
    assert_eq!(
        domain(f.read(&root, "none.json").await),
        ApiError::CollectionNotFound
    );
    let api = project.join(".devbox/api");
    std::fs::create_dir_all(&api).expect("api");
    std::fs::write(api.join("big.json"), &big).expect("external big file");
    assert_eq!(
        domain(f.read(&root, "big.json").await),
        ApiError::CollectionTooLarge { limit: 1024 * 1024 }
    );
    std::fs::write(api.join(".devbox-tmp-1.tmp"), "{}").expect("temp leftover");
    let written = f
        .write(&root, "d.json", "{}", CollectionExpect::Absent)
        .await
        .expect("create");
    let listed: Vec<_> = f
        .list(&root)
        .await
        .items
        .into_iter()
        .map(|i| i.path)
        .collect();
    assert_eq!(listed, vec!["big.json".to_owned(), "d.json".to_owned()]);
    let stale = f
        .service
        .delete(DeleteFile {
            root: root.clone(),
            path: "d.json".into(),
            digest: sha(b"other"),
        })
        .await;
    assert_eq!(
        domain(stale),
        ApiError::CollectionConflict {
            digest: Some(written.digest.clone())
        }
    );
    assert!(api.join("d.json").exists());
    f.service
        .delete(DeleteFile {
            root: root.clone(),
            path: "d.json".into(),
            digest: written.digest,
        })
        .await
        .expect("delete");
    assert!(!api.join("d.json").exists());
}

/// 리뷰 권장: 읽을 수 없는 파일 하나가 목록 전체를 실패시키지 않고 그 항목만 표시된다.
#[tokio::test]
async fn unreadable_file_is_marked_without_failing_the_list() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new().await;
    let (root, project) = f.project("app").await;
    f.write(&root, "ok.json", "{}", CollectionExpect::Absent)
        .await
        .expect("create");
    let locked = project.join(".devbox/api/locked.json");
    std::fs::write(&locked, "{}").expect("fixture");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    let list = f.list(&root).await;
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).expect("restore");
    let item = |path: &str| {
        list.items
            .iter()
            .find(|i| i.path == path)
            .unwrap_or_else(|| panic!("{path} listed"))
            .clone()
    };
    assert!(item("locked.json").unreadable);
    assert_eq!(item("locked.json").digest, None);
    assert!(!item("locked.json").reviewed);
    assert!(!item("ok.json").unreadable);
}

/// 리뷰 권장: 저장(rename)해도 사용자가 정한 파일 권한을 유지한다.
#[tokio::test]
async fn saving_keeps_the_original_file_mode() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new().await;
    let (root, project) = f.project("app").await;
    let first = f
        .write(&root, "a.json", "{}", CollectionExpect::Absent)
        .await
        .expect("create");
    let file = project.join(".devbox/api/a.json");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).expect("chmod");
    f.write(
        &root,
        "a.json",
        "{\"x\":1}",
        CollectionExpect::Digest {
            digest: first.digest,
        },
    )
    .await
    .expect("save");
    let mode = std::fs::metadata(&file).expect("meta").permissions().mode() & 0o777;
    assert_eq!(mode, 0o640);
}
