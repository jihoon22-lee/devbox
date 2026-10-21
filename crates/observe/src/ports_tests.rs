use crate::ports::{is_listening_in, parse_net_tcp, unit_from_cgroup};
use std::fs;

fn table(addr: &str, port: u16, state: &str, inode: u64) -> String {
    format!(
        "sl local_address rem_address st tx_queue rx_queue tr tm->when retrnsmt uid timeout inode\n0: {addr}:{port:04X} 00000000:0000 {state} 00000000:00000000 00:00000000 00000000 1000 0 {inode}\n"
    )
}
#[test]
fn is_listening_accepts_ipv4_ipv6_loopback_and_wildcard() {
    let dir = tempfile::tempdir().expect("fixture result");
    fs::create_dir(dir.path().join("net")).expect("fixture result");
    for (file, addr, expected) in [
        ("tcp", "0100007F", "127.0.0.1"),
        ("tcp", "00000000", "0.0.0.0"),
        ("tcp6", "00000000000000000000000001000000", "::1"),
        ("tcp6", "00000000000000000000000000000000", "::"),
    ] {
        let input = table(addr, 12345, "0A", 71);
        fs::write(dir.path().join("net").join(file), &input).expect("fixture result");
        assert!(is_listening_in(dir.path(), 12345));
        let rows = parse_net_tcp(&input, file == "tcp6");
        assert_eq!(rows[0].addr, expected);
        assert_eq!(rows[0].inode, 71);
        fs::write(
            dir.path().join("net").join(file),
            table(addr, 12345, "01", 71),
        )
        .expect("fixture result");
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
    let temp = tempfile::tempdir().expect("fixture result");
    fs::create_dir_all(temp.path().join("net")).expect("fixture result");
    fs::write(
        temp.path().join("net/tcp"),
        table("0100007F", 3456, "0A", 42),
    )
    .expect("fixture result");
    fs::create_dir_all(temp.path().join("23/fd")).expect("fixture result");
    fs::write(
        temp.path().join("23/stat"),
        crate::processes_tests::stat(23, 0, 9),
    )
    .expect("fixture result");
    fs::write(
        temp.path().join("23/cgroup"),
        "0::/user.slice/devbox-run-a.service\n",
    )
    .expect("fixture result");
    std::os::unix::fs::symlink("socket:[42]", temp.path().join("23/fd/4")).expect("fixture result");
    std::os::unix::fs::symlink("/project/sub", temp.path().join("23/cwd")).expect("fixture result");
    fs::create_dir(temp.path().join("24")).expect("fixture result"); // disappearing/denied fd tolerated
    fs::write(temp.path().join("24/fd"), "not a readable fd directory").expect("fixture result");
    assert_eq!(
        socket_inode_to_pid(temp.path(), &[42])
            .expect("fixture result")
            .get(&42)
            .map(|(pid, _)| *pid),
        Some(23)
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
    fs::write(temp.path().join("stat"), "btime 1000\n").expect("fixture result");
    let proc = crate::ProcFs::fixture(temp.path().into(), std::sync::Arc::new(|| 0), 100, 4096);
    let rows = ports_snapshot(&proc, &roots, &runs).expect("fixture result");
    assert_eq!(
        serde_json::to_value(&rows[0]).expect("fixture result")["startTimeMs"],
        1_000_090
    );
    assert_eq!(rows[0].pid, Some(23));
    assert_eq!(rows[0].run_id.as_deref(), Some("run-a"));
    assert_eq!(rows[0].project_id.as_deref(), Some("p"));
    assert_eq!(rows[0].process.as_deref(), Some("worker (a) b)"));
    fs::remove_file(temp.path().join("23/stat")).expect("fixture result");
    let rows = ports_snapshot(&proc, &roots, &runs).expect("fixture result");
    assert_eq!(rows[0].pid, Some(23));
    assert_eq!(rows[0].start_time_ms, None);
    fs::write(
        temp.path().join("23/stat"),
        crate::processes_tests::stat(23, 0, 9),
    )
    .expect("fixture result");
    fs::remove_file(temp.path().join("stat")).expect("fixture result");
    assert_eq!(
        ports_snapshot(&proc, &roots, &runs).expect("fixture result")[0].start_time_ms,
        None
    );
}
#[tokio::test]
async fn owned_tcp_listener_is_observed_with_pid_without_docker() {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("fixture result");
    let port = listener.local_addr().expect("fixture result").port();
    let temp = tempfile::tempdir().expect("fixture result");
    let service = crate::ObserveService::new(temp.path().join("no-docker"));
    assert!(crate::is_listening(port));
    let rows = service.ports(vec![], vec![]).await.expect("fixture result");
    let row = rows
        .iter()
        .find(|r| r.port == port && r.pid == Some(std::process::id()))
        .expect("fixture value");
    assert_eq!(
        row.start_time_ms,
        Some(
            crate::processes::identity(std::process::id())
                .expect("fixture result")
                .start_time_ms
        )
    );
    service.shutdown().await;
}

#[test]
fn run_readiness_requires_owned_inodes_for_every_ipv4_and_ipv6_hint() {
    use crate::run_ports_listening_in;
    let temp = tempfile::tempdir().expect("owned proc fixture");
    let root = temp.path();
    fs::create_dir(root.join("net")).expect("net fixture");
    fs::write(root.join("net/tcp"), table("0100007F", 3456, "0A", 42)).expect("IPv4 fixture");
    fs::write(
        root.join("net/tcp6"),
        table("00000000000000000000000001000000", 3457, "0A", 43),
    )
    .expect("IPv6 fixture");
    for (pid, unit, inodes) in [
        (23, "unrelated.service", vec![42, 43]),
        (24, "devbox-run-owned.service", vec![42]),
    ] {
        let path = root.join(pid.to_string());
        fs::create_dir_all(path.join("fd")).expect("process fixture");
        fs::write(path.join("stat"), crate::processes_tests::stat(pid, 0, 9))
            .expect("process stat");
        fs::write(
            path.join("cgroup"),
            format!("0::/user.slice/{unit}/workers\n"),
        )
        .expect("process unit");
        for inode in inodes {
            std::os::unix::fs::symlink(
                format!("socket:[{inode}]"),
                path.join(format!("fd/{inode}")),
            )
            .expect("socket fd");
        }
    }
    // PID 24 is the service child, while the wrapper has a different PID.
    let ready =
        |ports: &[u16]| run_ports_listening_in(root, ports, "devbox-run-owned.service", Some(25));
    assert!(
        ready(&[3456]),
        "an unrelated lower PID sharing the FD must not hide its unit owner"
    );
    assert!(
        !ready(&[3456, 3457]),
        "an unrelated IPv6 listener cannot satisfy the second hint"
    );
    assert!(
        !run_ports_listening_in(root, &[3456], "owned.service", None),
        "unit names match whole components"
    );
    std::os::unix::fs::symlink("socket:[43]", root.join("24/fd/43")).expect("owned IPv6 socket");
    assert!(
        ready(&[3456, 3457]),
        "both IPv4 and IPv6 belong to the service child"
    );
    fs::remove_file(root.join("24/stat")).expect("disappeared child");
    assert!(
        !ready(&[3456, 3457]),
        "unverified process ownership cannot ready the service"
    );
}
