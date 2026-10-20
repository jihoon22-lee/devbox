use devbox_core::{CoreError, IndexDb};
use rusqlite::OptionalExtension;
use std::sync::{Arc, Barrier};

const API: &str = "CREATE TABLE api_y (value TEXT);";
const NOTES: &str = "CREATE TABLE notes_x (value TEXT);";

fn fixture() -> (tempfile::TempDir, IndexDb) {
    let dir = tempfile::tempdir().expect("fixture directory");
    let db = IndexDb::open(&dir.path().join("index.db")).expect("open index");
    (dir, db)
}

#[tokio::test]
async fn ensure_domain_recreates_only_that_domain() {
    let (_dir, db) = fixture();
    assert!(db.ensure_domain("notes", 1, NOTES).unwrap());
    assert!(db.ensure_domain("api", 1, API).unwrap());
    db.write(|tx| tx.execute_batch("INSERT INTO notes_x VALUES ('keep'); INSERT INTO api_y VALUES ('old'); CREATE TABLE apix_y (value); INSERT INTO apix_y VALUES ('near'); CREATE TABLE apiX (value); INSERT INTO apiX VALUES ('boundary'); CREATE TABLE \"api_quoted\"\"name\" (value); CREATE VIEW api_view AS SELECT value FROM notes_x;"))
        .await.unwrap();
    assert!(db.ensure_domain("api", 2, API).unwrap());
    assert!(!db.ensure_domain("notes", 1, NOTES).unwrap());
    db.read(|tx| {
        assert_eq!(
            tx.query_row("SELECT value FROM notes_x", [], |r| r.get::<_, String>(0))?,
            "keep"
        );
        assert_eq!(
            tx.query_row("SELECT count(*) FROM api_y", [], |r| r.get::<_, i64>(0))?,
            0
        );
        assert_eq!(
            tx.query_row("SELECT value FROM apix_y", [], |r| r.get::<_, String>(0))?,
            "near"
        );
        assert_eq!(
            tx.query_row("SELECT value FROM apiX", [], |r| r.get::<_, String>(0))?,
            "boundary"
        );
        assert_eq!(
            tx.query_row("SELECT value FROM api_view", [], |r| r.get::<_, String>(0))?,
            "keep"
        );
        assert_eq!(
            tx.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name='api_quoted\"name'",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            0
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn ensure_domain_is_idempotent_and_persists_versions() {
    let (dir, db) = fixture();
    assert!(db.ensure_domain("api", 0, API).unwrap());
    db.write(|tx| tx.execute("INSERT INTO api_y VALUES ('keep')", []))
        .await
        .unwrap();
    assert!(
        !db.ensure_domain("api", 0, "invalid SQL must not execute")
            .unwrap()
    );
    let reopened = IndexDb::open(&dir.path().join("index.db")).unwrap();
    assert!(!reopened.ensure_domain("api", 0, API).unwrap());
    assert_eq!(
        reopened
            .read(|tx| tx.query_row("SELECT value FROM api_y", [], |r| r.get::<_, String>(0)))
            .await
            .unwrap(),
        "keep"
    );
    assert!(reopened.ensure_domain("api", u32::MAX, API).unwrap());
    assert!(reopened.ensure_domain("api", 1, API).unwrap());
    assert!(!reopened.ensure_domain("api", 1, API).unwrap());
}

#[tokio::test]
async fn ensure_domain_failed_schema_rolls_back_tables_data_and_version() {
    let (_dir, db) = fixture();
    db.ensure_domain("api", 1, API).unwrap();
    db.ensure_domain("notes", 1, NOTES).unwrap();
    db.write(|tx| {
        tx.execute_batch("INSERT INTO api_y VALUES ('old'); INSERT INTO notes_x VALUES ('keep');")
    })
    .await
    .unwrap();
    assert!(
        db.ensure_domain(
            "api",
            2,
            "CREATE TABLE api_new (value); INSERT INTO missing VALUES (1);"
        )
        .is_err()
    );
    db.read(|tx| {
        assert_eq!(
            tx.query_row("SELECT value FROM api_y", [], |r| r.get::<_, String>(0))?,
            "old"
        );
        assert_eq!(
            tx.query_row(
                "SELECT version FROM index_schema WHERE domain='api'",
                [],
                |r| r.get::<_, u32>(0)
            )?,
            1
        );
        assert_eq!(
            tx.query_row("SELECT value FROM notes_x", [], |r| r.get::<_, String>(0))?,
            "keep"
        );
        assert_eq!(
            tx.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name='api_new'",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            0
        );
        Ok(())
    })
    .await
    .unwrap();
    assert!(db.ensure_domain("api", 2, API).unwrap());
}

#[tokio::test]
async fn ensure_domain_failed_initialization_leaves_no_schema_or_version() {
    let (_dir, db) = fixture();
    assert!(
        db.ensure_domain("search", 1, "CREATE TABLE search_x(value); INVALID SQL;")
            .is_err()
    );
    db.read(|tx| {
        assert!(
            tx.query_row(
                "SELECT version FROM index_schema WHERE domain='search'",
                [],
                |r| r.get::<_, u32>(0)
            )
            .optional()?
            .is_none()
        );
        assert_eq!(
            tx.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name='search_x'",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            0
        );
        Ok(())
    })
    .await
    .unwrap();
    assert!(
        db.ensure_domain("search", 1, "CREATE TABLE search_x(value);")
            .unwrap()
    );
}

#[test]
fn ensure_domain_rejects_ambiguous_or_reserved_names_before_sql() {
    let (_dir, db) = fixture();
    // Underscores create overlapping ownership (api_ vs api_history_);
    // index owns index_schema and sqlite owns SQLite internal tables.
    for domain in [
        "",
        "api_",
        "api_history",
        "API",
        "1api",
        "api%",
        "api*",
        "api-",
        "api'",
        "api\0x",
        " api",
        "노트",
        "index",
        "sqlite",
    ] {
        assert!(
            matches!(
                db.ensure_domain(domain, 1, "INVALID SQL"),
                Err(CoreError::DbInput(_))
            ),
            "{domain:?}"
        );
    }
    assert!(
        db.ensure_domain("api2", 1, "CREATE TABLE api2_x(value);")
            .unwrap()
    );
    assert!(!db.ensure_domain("api2", 1, "INVALID SQL").unwrap());
}

#[tokio::test]
async fn ensure_domain_recreates_fts_without_dropping_shadow_tables_twice() {
    let (_dir, db) = fixture();
    let schema = "CREATE VIRTUAL TABLE notes_fts USING fts5(body); CREATE TABLE notes_meta(value);";
    db.ensure_domain("notes", 1, schema).unwrap();
    db.ensure_domain("api", 1, API).unwrap();
    db.write(|tx| {
        tx.execute_batch(
            "INSERT INTO notes_fts VALUES ('old searchable'); INSERT INTO api_y VALUES ('keep');",
        )
    })
    .await
    .unwrap();
    assert!(db.ensure_domain("notes", 2, schema).unwrap());
    db.write(|tx| tx.execute("INSERT INTO notes_fts VALUES ('fresh searchable')", []))
        .await
        .unwrap();
    db.read(|tx| {
        assert_eq!(
            tx.query_row(
                "SELECT count(*) FROM notes_fts WHERE notes_fts MATCH 'old'",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            0
        );
        assert_eq!(
            tx.query_row(
                "SELECT body FROM notes_fts WHERE notes_fts MATCH 'fresh'",
                [],
                |r| r.get::<_, String>(0)
            )?,
            "fresh searchable"
        );
        assert_eq!(
            tx.query_row("SELECT value FROM api_y", [], |r| r.get::<_, String>(0))?,
            "keep"
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[test]
fn ensure_domain_concurrent_same_version_rebuilds_once() {
    let (_dir, db) = fixture();
    let barrier = Arc::new(Barrier::new(9));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let db = db.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                db.ensure_domain("api", 1, API).unwrap()
            })
        })
        .collect();
    barrier.wait();
    assert_eq!(
        workers
            .into_iter()
            .map(|worker| usize::from(worker.join().unwrap()))
            .sum::<usize>(),
        1
    );
}

#[tokio::test]
async fn ensure_domain_concurrent_domains_and_writers_preserve_each_other() {
    let (dir, db) = fixture();
    db.ensure_domain("notes", 1, NOTES).unwrap();
    db.ensure_domain("api", 1, API).unwrap();
    let independent = IndexDb::open(&dir.path().join("index.db")).unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let workers: Vec<_> = [
        (db.clone(), "api", API),
        (independent, "search", "CREATE TABLE search_x(value);"),
    ]
    .into_iter()
    .map(|(db, domain, schema)| {
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            for version in 2..18 {
                assert!(db.ensure_domain(domain, version, schema).unwrap());
            }
        })
    })
    .collect();
    barrier.wait();
    for _ in 0..32 {
        db.write(|tx| tx.execute("INSERT INTO notes_x VALUES ('keep')", []))
            .await
            .unwrap();
    }
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(
        db.read(|tx| tx.query_row("SELECT count(*) FROM notes_x", [], |r| r.get::<_, i64>(0)))
            .await
            .unwrap(),
        32
    );
    assert!(!db.ensure_domain("api", 17, API).unwrap());
    assert!(!db.ensure_domain("search", 17, "INVALID SQL").unwrap());
}

#[tokio::test]
async fn index_db_wal_read_only_pool_and_write_rollback() {
    let (dir, db) = fixture();
    db.ensure_domain("api", 1, API).unwrap();
    assert_eq!(
        db.read(|tx| tx.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0)))
            .await
            .unwrap(),
        "wal"
    );
    assert!(
        db.read(|tx| tx.execute("INSERT INTO api_y VALUES ('forbidden')", []))
            .await
            .is_err()
    );
    assert!(
        db.write(|tx| {
            tx.execute("INSERT INTO api_y VALUES ('rollback')", [])?;
            tx.execute("INSERT INTO absent VALUES (1)", [])
        })
        .await
        .is_err()
    );
    db.write(|tx| tx.execute("INSERT INTO api_y VALUES ('committed')", []))
        .await
        .unwrap();
    assert_eq!(
        db.read(|tx| tx.query_row("SELECT value FROM api_y", [], |r| r.get::<_, String>(0)))
            .await
            .unwrap(),
        "committed"
    );
    assert!(!dir.path().join("backups").exists());
    assert_eq!(
        db.read(|tx| tx.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name IN ('schema_versions','core_meta')",
            [],
            |r| r.get::<_, i64>(0)
        ))
        .await
        .unwrap(),
        0
    );
}

#[tokio::test]
async fn ensure_domain_version_write_failure_rolls_back_rebuild() {
    let (_dir, db) = fixture();
    db.ensure_domain("api", 1, API).unwrap();
    db.write(|tx| tx.execute_batch("INSERT INTO api_y VALUES ('keep'); CREATE TRIGGER reject_version BEFORE UPDATE ON index_schema BEGIN SELECT RAISE(ABORT, 'owned version failure'); END;")).await.unwrap();
    assert!(db.ensure_domain("api", 2, API).is_err());
    db.read(|tx| {
        assert_eq!(
            tx.query_row("SELECT value FROM api_y", [], |r| r.get::<_, String>(0))?,
            "keep"
        );
        assert_eq!(
            tx.query_row(
                "SELECT version FROM index_schema WHERE domain='api'",
                [],
                |r| r.get::<_, u32>(0)
            )?,
            1
        );
        Ok(())
    })
    .await
    .unwrap();
    db.write(|tx| tx.execute_batch("DROP TRIGGER reject_version;"))
        .await
        .unwrap();
    assert!(db.ensure_domain("api", 2, API).unwrap());
}

#[tokio::test]
async fn index_db_wal_reader_keeps_snapshot_while_writer_commits() {
    let (_dir, db) = fixture();
    db.ensure_domain("api", 1, API).unwrap();
    db.write(|tx| tx.execute("INSERT INTO api_y VALUES ('before')", []))
        .await
        .unwrap();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (finish, wait) = std::sync::mpsc::channel();
    let reader_db = db.clone();
    let reader = tokio::spawn(async move {
        reader_db
            .read(move |tx| {
                let before: String = tx.query_row("SELECT value FROM api_y", [], |r| r.get(0))?;
                started.send(()).unwrap();
                wait.recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                let after: String = tx.query_row("SELECT value FROM api_y", [], |r| r.get(0))?;
                Ok((before, after))
            })
            .await
            .unwrap()
    });
    ready.await.unwrap();
    db.write(|tx| tx.execute("UPDATE api_y SET value='after'", []))
        .await
        .unwrap();
    finish.send(()).unwrap();
    assert_eq!(reader.await.unwrap(), ("before".into(), "before".into()));
    assert_eq!(
        db.read(|tx| tx.query_row("SELECT value FROM api_y", [], |r| r.get::<_, String>(0)))
            .await
            .unwrap(),
        "after"
    );
}

#[tokio::test]
async fn ensure_domain_rebuilds_intradomain_foreign_keys_in_any_drop_order() {
    let mut failures = Vec::new();
    for action in ["NO ACTION", "RESTRICT"] {
        let (_dir, db) = fixture();
        // pragma_table_list returns the newer parent first: exercise its implicit
        // DELETE while an older child table still refers to the parent's rows.
        let schema = format!(
            "CREATE TABLE api_child (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES api_parent(id) ON DELETE {action}); CREATE TABLE api_parent (id INTEGER PRIMARY KEY);"
        );
        db.ensure_domain("api", 1, &schema).unwrap();
        db.ensure_domain("notes", 1, NOTES).unwrap();
        db.write(|tx| tx.execute_batch("INSERT INTO api_parent VALUES (1); INSERT INTO api_child VALUES (1,1); INSERT INTO notes_x VALUES ('keep');"))
            .await.unwrap();
        for invalid in ["INVALID SQL;", "INSERT INTO api_child VALUES (2,999);"] {
            assert!(
                db.ensure_domain("api", 2, &format!("{schema} {invalid}"))
                    .is_err()
            );
            db.read(|tx| {
                assert_eq!(
                    tx.query_row("SELECT parent_id FROM api_child", [], |r| r
                        .get::<_, i64>(0))?,
                    1
                );
                assert_eq!(
                    tx.query_row("SELECT id FROM api_parent", [], |r| r.get::<_, i64>(0))?,
                    1
                );
                assert_eq!(
                    tx.query_row(
                        "SELECT version FROM index_schema WHERE domain='api'",
                        [],
                        |r| r.get::<_, u32>(0)
                    )?,
                    1
                );
                Ok(())
            })
            .await
            .unwrap();
            assert_eq!(db.write(|tx| tx.query_row("PRAGMA defer_foreign_keys", [], |r| r.get::<_, i64>(0))).await.unwrap(), 0);
        }
        let result = db.ensure_domain("api", 2, &schema);
        if !matches!(result, Ok(true)) {
            failures.push(format!("{action}: {result:?}"));
            continue;
        }
        db.read(|tx| {
            for table in ["api_parent", "api_child"] {
                assert_eq!(
                    tx.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                        .get::<_, i64>(0))?,
                    0
                );
            }
            assert_eq!(
                tx.query_row("SELECT value FROM notes_x", [], |r| r.get::<_, String>(0))?,
                "keep"
            );
            Ok(())
        })
        .await
        .unwrap();
        assert!(!db.ensure_domain("notes", 1, "INVALID SQL").unwrap());
        db.write(|tx| {
            assert_eq!(
                tx.query_row("PRAGMA defer_foreign_keys", [], |r| r.get::<_, i64>(0))?,
                0
            );
            tx.execute_batch(
                "INSERT INTO api_parent VALUES (2); INSERT INTO api_child VALUES (2,2);",
            )
        })
        .await
        .unwrap();
        assert!(!db.ensure_domain("api", 2, "INVALID SQL").unwrap());
        assert_eq!(
            db.read(
                |tx| tx.query_row("SELECT parent_id FROM api_child", [], |r| r
                    .get::<_, i64>(0))
            )
            .await
            .unwrap(),
            2
        );
        assert!(
            db.write(|tx| tx.execute("INSERT INTO api_child VALUES (3,999)", []))
                .await
                .is_err()
        );
    }
    assert!(
        failures.is_empty(),
        "same-domain rebuild failures: {failures:?}"
    );
}
