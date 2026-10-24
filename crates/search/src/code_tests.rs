use crate::code::{self, CodeRoot, CodeSearch, MAX_FILE_BYTES};
use crate::fixture::*;
use crate::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// Every hit below `root` for `needle`, with the chunk sizes and `truncated`.
fn grep(
    root: &Path,
    exclude: &[String],
    private: &[PathBuf],
    limit: usize,
) -> (Vec<SearchHit>, Vec<usize>, bool) {
    let matcher = compile("needle", false, false).expect("literal matcher");
    let roots = [CodeRoot {
        root: "project:p".into(),
        path: root.into(),
    }];
    let s = CodeSearch {
        matcher: &matcher,
        exclude,
        private,
        limit,
    };
    let (mut hits, mut sizes) = (Vec::new(), Vec::new());
    let truncated = code::search(
        &roots,
        &s,
        &CancellationToken::new(),
        &AtomicU64::new(0),
        &mut |chunk| {
            sizes.push(chunk.len());
            hits.extend(chunk);
            true
        },
    );
    (hits, sizes, truncated)
}
fn paths(hits: &[SearchHit]) -> Vec<&str> {
    let mut out: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
    out.sort();
    out
}

#[test]
fn code_search_respects_gitignore_and_user_excludes() {
    let t = tempfile::tempdir().expect("fixture");
    let root = t.path();
    write(&root.join(".gitignore"), "build/\n");
    write(&root.join("src/a.rs"), "fn main() {}\n// needle here\n");
    write(&root.join("build/b.rs"), "needle\n");
    write(&root.join("dist/c.rs"), "needle\n");
    write(&root.join("web/dist/d.js"), "needle\n");
    let (hits, _, _) = grep(root, &["**/dist/**".into()], &[], 100);
    assert_eq!(paths(&hits), ["src/a.rs"]);
    assert_eq!(hits[0].root, "project:p");
    assert_eq!(hits[0].line, Some(2));
    assert_eq!(hl(&hits[0].snippet, hits[0].ranges_utf16[0]), "needle");
}

#[test]
fn hidden_internal_and_private_files_are_not_searched() {
    let t = tempfile::tempdir().expect("fixture");
    let root = t.path();
    for rel in [
        ".env",
        ".config/x.toml",
        ".trash/a.md",
        "x/.devbox-tmp-1/y",
        "data/devbox.db",
        ".git/config",
        "ok.txt",
    ] {
        write(&root.join(rel), "needle\n");
    }
    let (hits, _, _) = grep(root, &[], &[root.join("data")], 100);
    assert_eq!(paths(&hits), ["ok.txt"]);
}

#[test]
fn symlinks_out_of_the_project_are_not_followed() {
    let t = tempfile::tempdir().expect("fixture");
    let (root, outside) = (t.path().join("p"), t.path().join("outside"));
    write(&outside.join("secret.txt"), "needle\n");
    write(&root.join("ok.txt"), "needle\n");
    std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("link.txt"))
        .expect("file link");
    std::os::unix::fs::symlink(&outside, root.join("dir")).expect("dir link");
    let (hits, _, _) = grep(&root, &[], &[], 100);
    assert_eq!(paths(&hits), ["ok.txt"]);
}

#[test]
fn large_and_binary_files_are_skipped() {
    let t = tempfile::tempdir().expect("fixture");
    let root = t.path();
    let mut big = b"needle\n".to_vec();
    big.resize(MAX_FILE_BYTES as usize + 1, b'x');
    write(&root.join("big.txt"), &big);
    big.truncate(MAX_FILE_BYTES as usize);
    write(&root.join("edge.txt"), &big);
    write(&root.join("bin.dat"), b"needle\n\x00\x01\x02");
    write(&root.join("small.txt"), "needle\n");
    let (hits, _, _) = grep(root, &[], &[], 100);
    assert_eq!(paths(&hits), ["edge.txt", "small.txt"]);
}

#[test]
fn long_lines_are_cut_around_the_match() {
    let t = tempfile::tempdir().expect("fixture");
    let root = t.path();
    write(
        &root.join("long.txt"),
        format!("{}needle{}\n", "가".repeat(5000), "y".repeat(3000)),
    );
    let (hits, _, _) = grep(root, &[], &[], 100);
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert!(
        hit.snippet.chars().count() <= code::MAX_LINE_CHARS,
        "{}",
        hit.snippet.chars().count()
    );
    assert_eq!(hit.ranges_utf16.len(), 1);
    assert_eq!(hl(&hit.snippet, hit.ranges_utf16[0]), "needle");
}

#[test]
fn results_stop_at_limit_in_chunks_of_fifty() {
    let t = tempfile::tempdir().expect("fixture");
    let root = t.path();
    for i in 0..120 {
        write(&root.join(format!("f{i:03}.txt")), "needle\n");
    }
    let (hits, sizes, truncated) = grep(root, &[], &[], 100);
    assert_eq!((hits.len(), sizes, truncated), (100, vec![50, 50], true));
    let (hits, sizes, truncated) = grep(root, &[], &[], 500);
    assert_eq!(
        (hits.len(), sizes, truncated),
        (120, vec![50, 50, 20], false)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_stops_the_code_walk() {
    let t = tempfile::tempdir().expect("fixture");
    let root = t.path().to_path_buf();
    const FILES: u64 = 20_000;
    for i in 0..FILES {
        let text = if i % 100 == 0 { "needle\n" } else { "hay\n" };
        write(&root.join(format!("d{:02}/f{i:05}.txt", i / 1000)), text);
    }
    let cancel = CancellationToken::new();
    let visited = std::sync::Arc::new(AtomicU64::new(0));
    let (first, at) = (
        std::sync::Arc::new(AtomicU64::new(0)),
        std::sync::Arc::new(std::sync::Mutex::new(None)),
    );
    let walk = {
        let (cancel, visited, first, at) =
            (cancel.clone(), visited.clone(), first.clone(), at.clone());
        tokio::task::spawn_blocking(move || {
            let matcher = compile("needle", false, false).expect("matcher");
            let roots = [CodeRoot {
                root: "project:p".into(),
                path: root,
            }];
            let s = CodeSearch {
                matcher: &matcher,
                exclude: &[],
                private: &[],
                limit: 500,
            };
            code::search(&roots, &s, &cancel.clone(), &visited, &mut |chunk| {
                if first.load(Ordering::SeqCst) == 0 {
                    first.store(chunk.len() as u64, Ordering::SeqCst);
                    // The stream closed after the first chunk; the receiver stays alive.
                    cancel.cancel();
                    *at.lock().expect("fixture") = Some(Instant::now());
                }
                true
            })
        })
    };
    tokio::time::timeout(Duration::from_secs(30), walk)
        .await
        .expect("walk ends")
        .expect("walk task");
    let ended = Instant::now();
    assert_eq!(first.load(Ordering::SeqCst), 50, "the first chunk is full");
    let visited = visited.load(Ordering::SeqCst);
    assert!(visited < FILES, "walk stopped early: {visited}");
    let cancelled = at.lock().expect("fixture").expect("cancelled");
    assert!(ended - cancelled < Duration::from_secs(1));
}
