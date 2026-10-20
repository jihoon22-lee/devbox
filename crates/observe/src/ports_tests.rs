use crate::ports::{is_listening_in, parse_net_tcp, unit_from_cgroup};
use std::fs;

fn table(addr: &str, port: u16, state: &str, inode: u64) -> String {
    format!(
        "sl local_address rem_address st tx_queue rx_queue tr tm->when retrnsmt uid timeout inode\n0: {addr}:{port:04X} 00000000:0000 {state} 00000000:00000000 00:00000000 00000000 1000 0 {inode}\n"
    )
}
#[test]
fn is_listening_accepts_ipv4_ipv6_loopback_and_wildcard() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("net")).unwrap();
    for (file, addr, expected) in [
        ("tcp", "0100007F", "127.0.0.1"),
        ("tcp", "00000000", "0.0.0.0"),
        ("tcp6", "00000000000000000000000001000000", "::1"),
        ("tcp6", "00000000000000000000000000000000", "::"),
    ] {
        let input = table(addr, 12345, "0A", 71);
        fs::write(dir.path().join("net").join(file), &input).unwrap();
        assert!(is_listening_in(dir.path(), 12345));
        let rows = parse_net_tcp(&input, file == "tcp6");
        assert_eq!(rows[0].addr, expected);
        assert_eq!(rows[0].inode, 71);
        fs::write(
            dir.path().join("net").join(file),
            table(addr, 12345, "01", 71),
        )
        .unwrap();
        assert!(!is_listening_in(dir.path(), 12345));
    }
    assert!(parse_net_tcp("corrupt row", false).is_empty());
}
#[test]
fn unit_from_cgroup_supports_unified_and_legacy_paths() {
    assert_eq!(
        unit_from_cgroup(
            "0::/user.slice/user-1000.slice/user@1000.service/app.slice/devbox-run-a.service\n"
        ),
        Some("devbox-run-a.service".into())
    );
    assert_eq!(
        unit_from_cgroup("2:cpu:/other\n1:name=systemd:/user.slice/devbox-run-b.service/workers\n"),
        Some("devbox-run-b.service".into())
    );
    assert_eq!(unit_from_cgroup("0::/user.slice\n"), None);
}
#[test]
fn socket_inode_to_pid_and_unit_run_project_mapping() {
    use crate::{
        ProjectRoot, UnitRun,
        ports::{ports_snapshot, socket_inode_to_pid},
    };
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("net")).unwrap();
    fs::write(
        temp.path().join("net/tcp"),
        table("0100007F", 3456, "0A", 42),
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("23/fd")).unwrap();
    fs::write(
        temp.path().join("23/stat"),
        crate::processes_tests::stat(23, 0, 9),
    )
    .unwrap();
    fs::write(
        temp.path().join("23/cgroup"),
        "0::/user.slice/devbox-run-a.service\n",
    )
    .unwrap();
    std::os::unix::fs::symlink("socket:[42]", temp.path().join("23/fd/4")).unwrap();
    std::os::unix::fs::symlink("/project/sub", temp.path().join("23/cwd")).unwrap();
    fs::create_dir(temp.path().join("24")).unwrap(); // disappearing/denied fd tolerated
    fs::write(temp.path().join("24/fd"), "not a readable fd directory").unwrap();
    assert_eq!(
        socket_inode_to_pid(temp.path(), &[42]).unwrap().get(&42),
        Some(&23)
    );
    let roots = vec![ProjectRoot {
        id: "cwd-project".into(),
        path: "/project".into(),
    }];
    let runs = vec![UnitRun {
        unit: "devbox-run-a.service".into(),
        run_id: "run-a".into(),
        project_id: "p".into(),
    }];
    let rows = ports_snapshot(temp.path(), &roots, &runs).unwrap();
    assert_eq!(rows[0].pid, Some(23));
    assert_eq!(rows[0].run_id.as_deref(), Some("run-a"));
    assert_eq!(rows[0].project_id.as_deref(), Some("p"));
    assert_eq!(rows[0].process.as_deref(), Some("worker (a) b)"));
}
#[tokio::test]
async fn owned_tcp_listener_is_observed_with_pid_without_docker() {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let temp = tempfile::tempdir().unwrap();
    let service = crate::ObserveService::new(temp.path().join("no-docker"));
    assert!(crate::is_listening(port));
    let rows = service.ports(vec![], vec![]).await.unwrap();
    assert!(
        rows.iter()
            .any(|r| r.port == port && r.pid == Some(std::process::id()))
    );
    service.shutdown().await;
}
