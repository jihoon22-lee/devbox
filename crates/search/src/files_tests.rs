use crate::files::{self, FileNames, GitConfig, Listed, ls_files, score};
use crate::fixture::*;
use crate::*;
use std::time::{Duration, Instant};

#[tokio::test]
async fn blank_title_note_is_found_by_file_name() -> T {
    let s = fixture();
    // The notes index gives a blank title the file name (AR-F10); the path is the vault path.
    s.index_note("ideas/idea.md", "idea", "---\ntitle: \"\"\n---\n")
        .await?;
    s.index_note("other.md", "other", "body").await?;
    let chunks = s
        .run(q("idea", &[Scope::Files]))
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(
        found(&chunks, Scope::Files),
        [("vault".to_string(), "ideas/idea.md".to_string())]
    );
    let hit = &chunks[0].items[0];
    assert_eq!(hl(&hit.snippet, hit.ranges_utf16[0]), "idea", "{hit:?}");
    Ok(())
}

#[test]
fn fuzzy_score_prefers_substrings_and_word_starts() {
    let s = |q: &str, p: &str| score(q, p, false).map(|(n, _)| n);
    assert!(s("search", "docs/search-notes.md") > s("search", "assets/s_e_a_r_c_h.png"));
    assert!(s("sn", "docs/search-notes.md") > s("sn", "docs/assign.md"));
    assert!(s("idea", "idea.md") > s("idea", "ideas/long/idea-old.txt"));
    assert_eq!(s("zz", "src/main.rs"), None);
    assert_eq!(
        score("IDEA", "notes/idea.md", false).map(|(_, r)| r),
        Some(vec![(6, 10)])
    );
    assert_eq!(score("IDEA", "notes/idea.md", true), None, "case-sensitive");
}

#[test]
fn rank_keeps_the_best_within_limit() {
    let candidates = (0..10)
        .map(|i| ("vault".to_string(), format!("x{i}/note.md")))
        .chain([("vault".to_string(), "note.md".to_string())])
        .collect();
    let (hits, truncated) = files::rank(
        "note",
        None,
        candidates,
        false,
        3,
        &tokio_util::sync::CancellationToken::new(),
    );
    assert_eq!(hits.len(), 3);
    assert!(truncated);
    assert_eq!(hits[0].path, "note.md");
}

fn repo(base: &std::path::Path) -> std::path::PathBuf {
    let repo = base.join("repo");
    write(&repo.join("a.txt"), "a\n");
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-qm", "init"]);
    repo
}

#[tokio::test]
async fn ls_files_runs_no_repository_fsmonitor_hook() {
    let t = tempfile::tempdir().expect("fixture");
    let repo = repo(t.path());
    let marker = t.path().join("MARK");
    let hook = script(
        &t.path().join("hook.sh"),
        &format!("touch '{}'\nexit 1", marker.display()),
    );
    git(
        &repo,
        &[
            "config",
            "core.fsmonitor",
            hook.to_str().expect("utf-8 path"),
        ],
    );
    let listed = ls_files(&GitConfig::default(), &repo).await;
    assert_eq!(listed, Listed::Files(vec!["a.txt".into()], false));
    assert!(!marker.exists(), "the repository's fsmonitor hook ran");
}

#[tokio::test]
async fn ls_files_is_time_and_size_bounded() {
    let t = tempfile::tempdir().expect("fixture");
    let dir = t.path();
    let slow = GitConfig {
        program: script(&dir.join("slow"), "sleep 5"),
        timeout: Duration::from_millis(200),
        ..GitConfig::default()
    };
    let started = Instant::now();
    assert_eq!(ls_files(&slow, dir).await, Listed::TimedOut);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );

    let many = GitConfig {
        program: script(
            &dir.join("many"),
            "i=0; while [ $i -lt 1000 ]; do printf 'f%d\\0' $i; i=$((i+1)); done",
        ),
        max_files: 10,
        ..GitConfig::default()
    };
    match ls_files(&many, dir).await {
        Listed::Files(names, truncated) => assert_eq!((names.len(), truncated), (10, true)),
        other => panic!("{other:?}"),
    }
    let wide = GitConfig {
        program: script(
            &dir.join("wide"),
            "head -c 300000 /dev/zero | tr '\\0' 'a'; printf '\\0b\\0'",
        ),
        max_output: 1000,
        ..GitConfig::default()
    };
    match ls_files(&wide, dir).await {
        Listed::Files(names, truncated) => {
            assert!(truncated && names.len() <= 1, "{}", names.len())
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn file_names_skip_hidden_internal_excluded_and_private_and_walk_non_git_folders() {
    let t = tempfile::tempdir().expect("fixture");
    let repo = t.path().join("repo");
    for rel in [
        ".env",
        ".github/ci.yml",
        "dist/a.js",
        "data/x.db",
        "src/ok.rs",
    ] {
        write(&repo.join(rel), "x\n");
    }
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "init"]);
    let names = FileNames::new(GitConfig::default());
    let listing = names
        .list(&repo, &["**/dist/**".into()], &[repo.join("data")])
        .await;
    assert_eq!(*listing.names, ["src/ok.rs"]);

    let plain = t.path().join("plain");
    for rel in [".secret", "b/c.txt", ".trash/d.md"] {
        write(&plain.join(rel), "x\n");
    }
    let listing = names.list(&plain, &[], &[]).await;
    assert_eq!(
        *listing.names,
        ["b/c.txt"],
        "a folder without Git is walked"
    );
}

#[tokio::test]
async fn file_name_listing_is_cached_for_the_ttl() {
    let t = tempfile::tempdir().expect("fixture");
    let count = t.path().join("count");
    let git = GitConfig {
        program: script(
            &t.path().join("git"),
            &format!("echo x >> '{}'\nprintf 'a.txt\\0'", count.display()),
        ),
        ..GitConfig::default()
    };
    let calls = || {
        std::fs::read_to_string(&count)
            .map(|s| s.lines().count())
            .unwrap_or(0)
    };
    let names = FileNames::new(git.clone());
    names.list(t.path(), &[], &[]).await;
    let listing = names.list(t.path(), &[], &[]).await;
    assert_eq!(
        (calls(), listing.names.as_slice()),
        (1, ["a.txt".to_string()].as_slice())
    );
    let fresh = FileNames::new(GitConfig {
        ttl: Duration::ZERO,
        ..git
    });
    fresh.list(t.path(), &[], &[]).await;
    fresh.list(t.path(), &[], &[]).await;
    assert_eq!(calls(), 3);
}

#[tokio::test]
async fn folder_excludes_apply_to_git_file_names_like_code_search() {
    let t = tempfile::tempdir().expect("fixture");
    let repo = t.path().join("repo");
    for rel in ["dist/a.js", "build/b.js", "web/dist/c.js", "src/ok.js"] {
        write(&repo.join(rel), "needle\n");
    }
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "init"]);
    let exclude = ["dist/".to_string(), "build".to_string()];
    let names = FileNames::new(GitConfig::default());
    let listing = names.list(&repo, &exclude, &[]).await;
    assert_eq!(*listing.names, ["src/ok.js"]);
    let walked = crate::code::list(&repo, &exclude, &[], 100).0;
    assert_eq!(walked, ["src/ok.js"], "code search excludes the same files");
}

#[test]
fn rank_stops_when_cancelled() {
    let candidates: Vec<(String, String)> = (0..200_000)
        .map(|i| ("vault".to_string(), format!("d{}/note{i}.md", i % 97)))
        .collect();
    let cancel = tokio_util::sync::CancellationToken::new();
    let (hits, _) = files::rank("note", None, candidates.clone(), false, 10, &cancel);
    assert_eq!(hits.len(), 10, "control");
    cancel.cancel();
    let (hits, _) = files::rank("note", None, candidates, false, 10, &cancel);
    assert!(hits.is_empty(), "a cancelled ranking returns nothing");
}

#[tokio::test]
async fn ls_files_timeout_kills_the_whole_process_group() {
    let t = tempfile::tempdir().expect("fixture");
    let pid_file = t.path().join("child.pid");
    // The fake git leaves a background child holding no pipe of ours; only a
    // process-group kill reaches it.
    let git = GitConfig {
        program: script(
            &t.path().join("git"),
            &format!(
                "sleep 30 >/dev/null 2>&1 &\necho $! > '{}'\nsleep 30",
                pid_file.display()
            ),
        ),
        timeout: Duration::from_millis(300),
        ..GitConfig::default()
    };
    assert_eq!(ls_files(&git, t.path()).await, Listed::TimedOut);
    let pid = std::fs::read_to_string(&pid_file).expect("child pid");
    let stat = format!("/proc/{}/stat", pid.trim());
    let deadline = Instant::now() + Duration::from_secs(3);
    let alive = || {
        std::fs::read_to_string(&stat)
            .map(|s| {
                !s.split(") ")
                    .nth(1)
                    .is_some_and(|rest| rest.starts_with('Z'))
            })
            .unwrap_or(false)
    };
    while alive() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!alive(), "the git helper's child survived");
}
