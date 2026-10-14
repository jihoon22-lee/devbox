#![cfg(target_os = "linux")]
mod support;

use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use support::TestDaemon;

const PROCESS: &str = r#"import pathlib, sys
pathlib.Path('filter-marker').write_text('process-ran')
source, target = sys.stdin.buffer, sys.stdout.buffer
def read_list():
    result = []
    while True:
        header = source.read(4)
        if not header:
            return None
        size = int(header, 16)
        if size == 0:
            return result
        result.append(source.read(size - 4))
def write_list(items):
    for item in items:
        target.write(('%04x' % (len(item) + 4)).encode() + item)
    target.write(b'0000')
    target.flush()
assert read_list() == [b'git-filter-client\n', b'version=2\n']
write_list([b'git-filter-server\n', b'version=2\n'])
assert read_list() is not None
write_list([b'capability=clean\n', b'capability=smudge\n'])
while True:
    request = read_list()
    if request is None:
        break
    contents = read_list()
    write_list([b'status=success\n'])
    write_list(contents)
    write_list([])
"#;

fn fixture(
    kind: &str,
    driver: &str,
    required: bool,
    worktree: bool,
) -> (
    TestDaemon,
    String,
    std::path::PathBuf,
    String,
    std::path::PathBuf,
) {
    let mut daemon = TestDaemon::fixture();
    daemon.write_config(
        "[[agents.profiles]]\nid='fixture'\nlabel='Fixture'\ncommand='true'\nhooks='none'\n",
    );
    let global = daemon.home.path().join(".gitconfig");
    daemon.extra_env.extend([
        (
            "GIT_CONFIG_GLOBAL".into(),
            global.to_str().expect("owned config").into(),
        ),
        ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
        ("GIT_CONFIG_COUNT".into(), "0".into()),
    ]);
    let repo = daemon.git_repo("untrusted-filter-source");
    let source = Path::new(&repo);
    std::fs::create_dir(source.join(".filters")).expect("owned scripts");
    std::fs::write(
        source.join(".filters/smudge"),
        "printf smudge-ran > filter-marker\ncat\n",
    )
    .expect("owned script");
    std::fs::write(source.join(".filters/process.py"), PROCESS).expect("owned script");
    std::fs::write(
        source.join(".gitattributes"),
        format!("zz-payload filter={driver}\n"),
    )
    .expect("owned attributes");
    std::fs::write(source.join("zz-payload"), "raw-blob-data\n").expect("owned blob");
    daemon.git(source, &["add", ".filters", ".gitattributes", "zz-payload"]);
    daemon.git(source, &["commit", "-qm", "track filter and payload"]);
    let target = daemon.home.path().join("cloned-filter");
    let include = daemon.home.path().join("filters.gitconfig");
    let command = if kind == "smudge" {
        "sh .filters/smudge"
    } else {
        "python3 .filters/process.py"
    };
    std::fs::write(&include, format!("[filter {}]\n{kind} = {command}\nrequired = {required}\n[filter \"never-selected\"]\nrequired = true\n",serde_json::to_string(driver).expect("owned subsection"))).expect("owned include");
    let mut config = std::fs::read_to_string(&global).expect("owned global");
    let condition = if worktree {
        "onbranch:agent/**".to_owned()
    } else {
        format!("gitdir:{}/.git", target.display())
    };
    config.push_str(&format!(
        "\n[checkout]\nworkers = 1\n[includeIf {}]\npath = {}\n",
        serde_json::to_string(&condition).expect("owned condition"),
        serde_json::to_string(include.to_str().expect("owned path")).expect("owned include path")
    ));
    std::fs::write(&global, &config).expect("owned global");
    (daemon, repo, target, config, include)
}

async fn clone_case(kind: &str, driver: &str, required: bool) {
    let (mut daemon, repo, target, global_before, include) = fixture(kind, driver, required, false);
    let include_before = std::fs::read(&include).expect("owned include");
    daemon.launch();
    let client = daemon.client().await;
    let result = client
        .raw_call("projects.clone", json!({"url":repo,"dir":target}), None)
        .await;
    if required {
        let error =
            result.expect_err("selected required filter must fail safely before registration");
        assert!(
            format!("{error:?}").contains("projects.clone_failed"),
            "{error:?}"
        );
        let list = client
            .raw_call("projects.list", json!({}), None)
            .await
            .expect("owned list");
        let list: Value = serde_json::from_str(&list).expect("list");
        assert!(list["items"].as_array().expect("project rows").is_empty());
        assert!(
            target.join(".git").is_dir(),
            "preserve the downloaded owned repository on failed population"
        );
    } else {
        let project: Value =
            serde_json::from_str(&result.expect("optional driver may pass through"))
                .expect("project");
        assert_eq!(project["path"], target.to_str().expect("owned target"));
        assert_eq!(
            std::fs::read_to_string(target.join("zz-payload")).expect("raw payload"),
            "raw-blob-data\n"
        );
    }
    assert!(
        !target.join("filter-marker").exists(),
        "implicit clone executed {kind} repository code before trust"
    );
    assert_eq!(
        std::fs::read_to_string(daemon.home.path().join(".gitconfig")).expect("global preserved"),
        global_before
    );
    assert_eq!(
        std::fs::read(&include).expect("include preserved"),
        include_before
    );
    if !required {
        std::fs::remove_file(target.join("zz-payload")).expect("owned payload");
        daemon.git(&target, &["checkout", "--", "zz-payload"]);
        assert_eq!(
            std::fs::read_to_string(target.join("filter-marker"))
                .expect("ordinary external Git retains filters"),
            format!("{kind}-ran")
        );
    }
}

async fn worktree_case(kind: &str, driver: &str, required: bool) {
    let (mut daemon, repo, _, global_before, include) = fixture(kind, driver, required, true);
    let include_before = std::fs::read(&include).expect("owned include");
    daemon.launch();
    let client = daemon.client().await;
    let project = daemon.add_project(&client, &repo).await;
    assert!(!Path::new(&repo).join(".devbox/devbox.toml").exists());
    let response = client.raw_call("agents.create",json!({"projectId":project,"title":"Owned filter fixture","profile":"fixture","worktree":true,"prompt":""}),None).await.expect("create without setup approval");
    let task: Value = serde_json::from_str(&response).expect("task");
    let task = daemon
        .wait_for(
            &client,
            task["id"].as_str().expect("ID"),
            if required { "failed" } else { "review" },
            Duration::from_secs(10),
        )
        .await;
    let wt = Path::new(task["worktreePath"].as_str().expect("owned worktree"));
    assert!(
        wt.join(".git").is_file(),
        "retain registered partial worktree resources"
    );
    if required {
        assert_eq!(task["reason"], "worktree_add_partial");
    } else {
        assert_eq!(
            std::fs::read_to_string(wt.join("zz-payload")).expect("raw payload"),
            "raw-blob-data\n"
        );
    }
    assert!(
        !wt.join("filter-marker").exists(),
        "implicit agent worktree executed {kind} repository code before trust"
    );
    assert_eq!(
        std::fs::read_to_string(daemon.home.path().join(".gitconfig")).expect("global preserved"),
        global_before
    );
    assert_eq!(
        std::fs::read(&include).expect("include preserved"),
        include_before
    );
    if !required {
        std::fs::remove_file(wt.join("zz-payload")).expect("owned payload");
        daemon.git(wt, &["checkout", "--", "zz-payload"]);
        assert_eq!(
            std::fs::read_to_string(wt.join("filter-marker"))
                .expect("ordinary Git retains filters"),
            format!("{kind}-ran")
        );
    }
}

#[tokio::test]
async fn implicit_clone_never_executes_smudge_script_from_destination_only_config() {
    clone_case("smudge", "devbox-review", false).await;
}
#[tokio::test]
async fn implicit_clone_never_executes_process_script_from_destination_only_config() {
    clone_case("process", "devbox-review", false).await;
}
#[tokio::test]
async fn implicit_worktree_never_executes_smudge_script_from_branch_only_config() {
    worktree_case("smudge", "devbox-review", false).await;
}
#[tokio::test]
async fn implicit_worktree_never_executes_process_script_from_branch_only_config() {
    worktree_case("process", "devbox-review", false).await;
}
#[tokio::test]
async fn required_clone_filter_fails_without_execution_or_registration() {
    clone_case("process", "devbox-review", true).await;
}
#[tokio::test]
async fn required_worktree_filter_fails_without_execution_or_losing_partial_resources() {
    worktree_case("smudge", "devbox-review", true).await;
}
#[tokio::test]
async fn filter_subsection_equals_and_quote_cannot_bypass_checkout_isolation() {
    clone_case("smudge", "a=b", false).await;
    worktree_case("process", "a'b", false).await;
}

#[tokio::test]
async fn empty_repository_clone_remains_registered_without_checkout() {
    let mut daemon = TestDaemon::fixture();
    let source = daemon.home.path().join("empty-source");
    std::fs::create_dir(&source).expect("owned source");
    daemon.git(&source, &["init", "-q", "-b", "main"]);
    daemon.launch();
    let client = daemon.client().await;
    let target = daemon.home.path().join("empty-clone");
    let response = client
        .raw_call("projects.clone", json!({"url":source,"dir":target}), None)
        .await
        .expect("empty clone still registers");
    let project: Value = serde_json::from_str(&response).expect("project");
    assert_eq!(project["path"], target.to_str().expect("owned target"));
    assert!(target.join(".git").is_dir());
}
