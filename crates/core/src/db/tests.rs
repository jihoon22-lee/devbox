use super::*;
const V1: Migration = Migration {
    domain: "t",
    version: 1,
    sql: "CREATE TABLE t_items (name TEXT);",
};
const V2: Migration = Migration {
    domain: "t",
    version: 2,
    sql: "ALTER TABLE t_items ADD COLUMN extra TEXT;",
};
fn open(
    d: &tempfile::TempDir,
    version: &str,
    migrations: &[Migration],
) -> Result<Db, crate::CoreError> {
    Db::open(
        &d.path().join("devbox.db"),
        &d.path().join("backups"),
        version,
        migrations,
    )
}
#[tokio::test]
async fn commits_rollback_and_read_only() {
    let d = tempfile::tempdir().expect("create fixture directory");
    let db = open(&d, "1", &[V1]).expect("open fixture database");
    assert_eq!(
        db.write(|tx| tx.execute("INSERT INTO t_items VALUES ('a')", []))
            .await
            .expect("execute fixture SQL")
            .1,
        1
    );
    assert!(
        db.write(|tx| {
            tx.execute("INSERT INTO t_items VALUES ('b')", [])?;
            tx.execute("INSERT INTO absent VALUES (1)", [])
        })
        .await
        .is_err()
    );
    assert_eq!(db.rev(), 1);
    assert_eq!(
        db.read(|tx| Ok((
            tx.query_row("SELECT count(*) FROM t_items", [], |r| r.get::<_, i64>(0))?,
            current_rev(tx)?
        )))
        .await
        .expect("query fixture row"),
        (1, 1)
    );
    assert!(
        db.read(|tx| tx.execute("DELETE FROM t_items", []))
            .await
            .is_err()
    );
    assert!(
        db.read(|tx| {
            tx.execute_batch("PRAGMA query_only=OFF")?;
            tx.execute("DELETE FROM t_items", [])
        })
        .await
        .is_err()
    );
}
#[tokio::test]
async fn panic_recovers_writer_and_reader() {
    let d = tempfile::tempdir().expect("create fixture directory");
    let db = open(&d, "1", &[V1]).expect("open fixture database");
    assert!(
        db.write::<(), _>(|tx| {
            tx.execute("INSERT INTO t_items VALUES ('a')", [])?;
            panic!("owned test panic")
        })
        .await
        .is_err()
    );
    assert_eq!(db.rev(), 0);
    assert!(
        db.read::<(), _>(|_| panic!("owned reader panic"))
            .await
            .is_err()
    );
    assert_eq!(
        db.write(|tx| tx.execute("INSERT INTO t_items VALUES ('b')", []))
            .await
            .expect("execute fixture SQL")
            .1,
        1
    );
}
#[test]
fn validates_versions_and_preserves_original_backup_on_retry() {
    let d = tempfile::tempdir().expect("create fixture directory");
    drop(open(&d, "1", &[V1]).expect("open fixture database"));
    assert!(open(&d, "../escape", &[V1, V2]).is_err());
    assert!(open(&d, "2", &[V2, V1]).is_err());
    assert!(open(&d, "2", &[V1, V1]).is_err());
    let bad = Migration {
        domain: "other",
        version: 1,
        sql: "INVALID SQL",
    };
    let foreign = d.path().join("backups/devbox-pre-foreign.txt");
    std::fs::create_dir_all(foreign.parent().expect("fixture path has parent"))
        .expect("fixture path has parent");
    std::fs::write(&foreign, "keep").expect("write fixture file");
    assert!(open(&d, "2", &[V1, V2, bad]).is_err());
    let backup = d.path().join("backups/devbox-pre-2.db");
    let before = std::fs::read(&backup).expect("read fixture file");
    assert!(open(&d, "2", &[V1, V2, bad]).is_err());
    assert_eq!(std::fs::read(&backup).expect("read fixture file"), before);
    assert!(foreign.exists());
    drop(open(&d, "2", &[V1, V2]).expect("open fixture database"));
    let c = rusqlite::Connection::open(backup).expect("open fixture database");
    assert!(c.prepare("SELECT extra FROM t_items").is_err());
    assert!(!d.path().join("backups/devbox-pre-3.db").exists());
    drop(open(&d, "3", &[V1, V2]).expect("open fixture database"));
}
#[test]
fn negative_revision_is_rejected() {
    let d = tempfile::tempdir().expect("create fixture directory");
    drop(open(&d, "1", &[V1]).expect("open fixture database"));
    let c = rusqlite::Connection::open(d.path().join("devbox.db")).expect("open fixture database");
    c.execute("UPDATE core_meta SET value=-1", [])
        .expect("execute fixture SQL");
    assert!(open(&d, "1", &[V1]).is_err());
}

#[tokio::test]
async fn reader_snapshot_stays_consistent_across_write() {
    let d = tempfile::tempdir().expect("create fixture directory");
    let db = open(&d, "1", &[V1]).expect("open fixture database");
    let (started, ready) = tokio::sync::oneshot::channel();
    let (resume, proceed) = std::sync::mpsc::channel();
    let reader = db.clone();
    let task = tokio::spawn(async move {
        reader
            .read(move |tx| {
                let before = current_rev(tx)?;
                let _ = started.send(());
                proceed
                    .recv_timeout(std::time::Duration::from_secs(3))
                    .expect("receive fixture signal before deadline");
                let count: i64 = tx.query_row("SELECT count(*) FROM t_items", [], |r| r.get(0))?;
                Ok((before, current_rev(tx)?, count))
            })
            .await
    });
    ready.await.expect("complete fixture snapshot task");
    db.write(|tx| tx.execute("INSERT INTO t_items VALUES ('a')", []))
        .await
        .expect("execute fixture SQL");
    resume.send(()).expect("send fixture message");
    assert_eq!(
        task.await
            .expect("complete fixture snapshot task")
            .expect("complete fixture snapshot task"),
        (0, 0, 0)
    );
}

#[tokio::test]
async fn cancelled_waiter_does_not_stop_writer() {
    let d = tempfile::tempdir().expect("create fixture directory");
    let db = open(&d, "1", &[V1]).expect("open fixture database");
    let (started, ready) = tokio::sync::oneshot::channel();
    let (resume, proceed) = std::sync::mpsc::channel();
    let writer = db.clone();
    let task = tokio::spawn(async move {
        writer
            .write(move |tx| {
                let _ = started.send(());
                proceed
                    .recv_timeout(std::time::Duration::from_secs(3))
                    .expect("receive fixture signal before deadline");
                tx.execute("INSERT INTO t_items VALUES ('a')", [])
            })
            .await
    });
    ready
        .await
        .expect("fixture writer starts before cancellation");
    task.abort();
    resume.send(()).expect("send fixture message");
    assert_eq!(
        db.write(|tx| tx.execute("INSERT INTO t_items VALUES ('b')", []))
            .await
            .expect("execute fixture SQL")
            .1,
        2
    );
}

#[test]
fn failed_backup_keeps_previous_and_retention_only_removes_owned_file() {
    let d = tempfile::tempdir().expect("create fixture directory");
    drop(open(&d, "1", &[V1]).expect("open fixture database"));
    drop(open(&d, "2", &[V1, V2]).expect("open fixture database"));
    let previous = d.path().join("backups/devbox-pre-2.db");
    let bytes = std::fs::read(&previous).expect("read fixture file");
    let foreign = d.path().join("backups/devbox-pre-foreign.db");
    std::fs::write(&foreign, b"foreign").expect("write fixture file");
    let blocked = d.path().join("backups/devbox-pre-3.db");
    std::fs::create_dir(&blocked).expect("create fixture directory");
    let v3 = Migration {
        domain: "t",
        version: 3,
        sql: "CREATE TABLE t_more (id INTEGER);",
    };
    assert!(open(&d, "3", &[V1, V2, v3]).is_err());
    assert_eq!(std::fs::read(&previous).expect("read fixture file"), bytes);
    std::fs::remove_dir(blocked).expect("remove fixture backup obstruction");
    drop(open(&d, "3", &[V1, V2, v3]).expect("open fixture database"));
    assert!(!previous.exists());
    assert!(foreign.exists());
}

#[tokio::test]
async fn negative_revision_in_transaction_is_rejected() {
    let d = tempfile::tempdir().expect("create fixture directory");
    let db = open(&d, "1", &[V1]).expect("open fixture database");
    assert!(
        db.write(|tx| {
            tx.execute("UPDATE core_meta SET value=-2", [])?;
            current_rev(tx)
        })
        .await
        .is_err()
    );
    assert_eq!(
        db.read(current_rev).await.expect("read fixture snapshot"),
        0
    );
}

#[test]
fn semver_build_metadata_is_safe() {
    let d = tempfile::tempdir().expect("create fixture directory");
    drop(open(&d, "1.0.0+build.1", &[V1]).expect("open fixture database"));
    drop(open(&d, "1.0.1+build.2", &[V1, V2]).expect("open fixture database"));
    assert!(
        d.path()
            .join("backups/devbox-pre-1.0.1+build.2.db")
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn symlink_receipt_and_backup_fail_without_touching_foreign_data() {
    let d = tempfile::tempdir().expect("create fixture directory");
    drop(open(&d, "1", &[V1]).expect("open fixture database"));
    let backups = d.path().join("backups");
    std::fs::create_dir(&backups).expect("create fixture directory");
    let foreign = d.path().join("foreign");
    std::fs::write(&foreign, "owned by someone else").expect("write fixture file");
    let receipt = backups.join(".devbox-backup");
    std::os::unix::fs::symlink(&foreign, &receipt).expect("create fixture symlink");
    assert!(open(&d, "2", &[V1, V2]).is_err());
    assert_eq!(
        std::fs::read_to_string(&foreign).expect("read fixture text"),
        "owned by someone else"
    );
    std::fs::remove_file(receipt).expect("remove fixture file");
    std::os::unix::fs::symlink(&foreign, backups.join("devbox-pre-2.db"))
        .expect("create fixture symlink");
    assert!(open(&d, "2", &[V1, V2]).is_err());
    assert_eq!(
        std::fs::read_to_string(&foreign).expect("read fixture text"),
        "owned by someone else"
    );
}

#[tokio::test]
async fn new_batch_same_app_version_refreshes_backup() {
    let d = tempfile::tempdir().expect("create fixture directory");
    drop(open(&d, "1.0.0-dev", &[V1]).expect("open fixture database"));
    let db = open(&d, "1.0.0-dev", &[V1, V2]).expect("open fixture database");
    db.write(|tx| {
        tx.execute(
            "INSERT INTO t_items(name,extra) VALUES ('user','saved')",
            [],
        )
    })
    .await
    .expect("execute fixture SQL");
    drop(db);
    let v3 = Migration {
        domain: "t",
        version: 3,
        sql: "CREATE TABLE t_more(id INTEGER);",
    };
    drop(open(&d, "1.0.0-dev", &[V1, V2, v3]).expect("open fixture database"));
    let backup = rusqlite::Connection::open(d.path().join("backups/devbox-pre-1.0.0-dev.db"))
        .expect("open fixture database");
    assert_eq!(
        backup
            .query_row("SELECT extra FROM t_items WHERE name='user'", [], |r| {
                r.get::<_, String>(0)
            })
            .expect("query fixture row"),
        "saved"
    );
    assert!(backup.prepare("SELECT * FROM t_more").is_err());
}

#[test]
fn receipt_stage_collision_preserves_unowned_file() {
    let d = tempfile::tempdir().expect("create fixture directory");
    let db = d.path().join("devbox.db");
    drop(open(&d, "1", &[V1]).expect("open fixture database"));
    let conn = rusqlite::Connection::open(db).expect("open fixture database");
    let dir = d.path().join("backups");
    std::fs::create_dir(&dir).expect("create fixture directory");
    let temporary = dir.join(".collision.tmp");
    let receipt_stage = temporary.with_extension("receipt");
    std::fs::write(&receipt_stage, b"unowned stage").expect("write fixture file");
    assert!(backup_at(&conn, &dir, "2", &[V1, V2], &temporary).is_err());
    assert_eq!(
        std::fs::read(&receipt_stage).expect("read fixture file"),
        b"unowned stage"
    );
    drop(open(&d, "2", &[V1, V2]).expect("open fixture database"));
    let target = dir.join("devbox-pre-2.db");
    let original = std::fs::read(&target).expect("read fixture file");
    let receipt = std::fs::read(dir.join(".devbox-backup")).expect("read fixture file");
    let v3 = Migration {
        domain: "t",
        version: 3,
        sql: "CREATE TABLE t_more(id INTEGER);",
    };
    assert!(backup_at(&conn, &dir, "2", &[V1, V2, v3], &temporary).is_err());
    assert_eq!(
        std::fs::read(receipt_stage).expect("read fixture file"),
        b"unowned stage"
    );
    assert_eq!(std::fs::read(target).expect("read fixture file"), original);
    assert_eq!(
        std::fs::read(dir.join(".devbox-backup")).expect("read fixture file"),
        receipt
    );
}

#[tokio::test]
async fn rejected_write_rolls_back_without_revision() {
    let d = tempfile::tempdir().expect("create fixture directory");
    let db = open(&d, "1", &[V1]).expect("open fixture database");
    let rejected = db
        .write_checked::<(), _, _>(|tx| {
            tx.execute("INSERT INTO t_items VALUES ('discard')", [])?;
            Ok(Err("rejected"))
        })
        .await
        .expect("complete rejected fixture transaction");
    assert_eq!(rejected, Err("rejected"));
    assert_eq!(db.rev(), 0);
    assert_eq!(
        db.read(|tx| tx.query_row("SELECT COUNT(*) FROM t_items", [], |r| r.get::<_, u64>(0)))
            .await
            .expect("query fixture row"),
        0
    );
    assert_eq!(
        db.write_checked::<_, (), _>(|tx| Ok(Ok(
            tx.execute("INSERT INTO t_items VALUES ('kept')", [])?
        )))
        .await
        .expect("execute fixture SQL")
        .expect("execute fixture SQL"),
        (1, 1)
    );
}

#[tokio::test]
async fn commit_callback_runs_only_after_successful_commit() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let d = tempfile::tempdir().expect("create fixture directory");
    let db = open(&d, "1", &[V1]).expect("open fixture database");
    let calls = Arc::new(AtomicUsize::new(0));
    let callback = |calls: Arc<AtomicUsize>, db: Db| {
        move |_: &(), rev| {
            assert_eq!(db.rev(), rev);
            calls.fetch_add(1, Ordering::SeqCst);
        }
    };
    let rejected = db
        .write_checked_with_commit::<(), _, _, _>(
            |tx| {
                tx.execute("INSERT INTO t_items VALUES ('rejected')", [])?;
                Ok(Err("rejected"))
            },
            callback(calls.clone(), db.clone()),
        )
        .await
        .expect("complete fixture commit callback");
    assert_eq!(rejected, Err("rejected"));
    let failure = db
        .write_checked_with_commit::<(), (), _, _>(
            |tx| {
                tx.execute("INSERT INTO absent VALUES (1)", [])?;
                Ok(Ok(()))
            },
            callback(calls.clone(), db.clone()),
        )
        .await;
    assert!(failure.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(db.rev(), 0);
    db.write_checked_with_commit::<(), (), _, _>(
        |tx| {
            tx.execute("INSERT INTO t_items VALUES ('committed')", [])?;
            Ok(Ok(()))
        },
        callback(calls.clone(), db.clone()),
    )
    .await
    .expect("complete fixture commit callback")
    .expect("complete fixture commit callback");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(db.rev(), 1);
}
