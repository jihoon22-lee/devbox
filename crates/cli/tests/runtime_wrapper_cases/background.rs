use super::*;

fn background_case(close_pipes: bool) {
    let f = Fixture::new();
    let script = format!(
        r#"import os,pathlib,subprocess,time
program="import os,pathlib,signal,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); pathlib.Path(os.environ['READY']+'.background').write_text(str(os.getpid())); time.sleep(30)"
kwargs={{'stdout':subprocess.DEVNULL,'stderr':subprocess.DEVNULL}} if {close_pipes} else {{}}
subprocess.Popen(['python3','-c',program],**kwargs)
while not pathlib.Path(os.environ['READY']+'.background').exists(): time.sleep(.01)
pathlib.Path(os.environ['READY']).write_text(str(os.getpid()))
print('leader tail',flush=True)
raise SystemExit(3)
"#,
        close_pipes = if close_pipes { "True" } else { "False" }
    );
    let mut owned = f.spawn(&script, "background", None);
    f.ready();
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        if let Some(status) = owned.child.try_wait().expect("wait wrapper") {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "wrapper must finish after direct child exits"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(status.code(), Some(3));
    assert_eq!(f.exit().code, Some(3));
    assert!(f.log().contains("\to\tleader tail\n"));
    let pid = std::fs::read_to_string(f.temp.path().join("child-pid.background"))
        .expect("background pid");
    if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        let state = stat
            .rsplit_once(')')
            .expect("proc stat")
            .1
            .split_whitespace()
            .next()
            .expect("state");
        assert_eq!(state, "Z", "owned descendant must no longer run");
    }
}
#[test]
fn background_inheriting_pipes_cannot_delay_direct_child_exit() {
    background_case(false);
}
#[test]
fn background_with_closed_pipes_is_also_terminated() {
    background_case(true);
}

struct EscapedFixture(PathBuf);
impl Drop for EscapedFixture {
    fn drop(&mut self) {
        if let Ok(pid) = std::fs::read_to_string(&self.0)
            && let Ok(pid) = pid.parse::<i32>()
        {
            let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
        }
    }
}
#[test]
fn background_outside_group_has_bounded_drain_and_is_cleaned_only_by_fixture() {
    let f = Fixture::new();
    let escaped = EscapedFixture(f.temp.path().join("child-pid.escaped"));
    let script = r#"import os,pathlib,subprocess,time
program="import os,pathlib,time; pathlib.Path(os.environ['READY']+'.escaped').write_text(str(os.getpid())); print('escaped output',flush=True); time.sleep(30)"
subprocess.Popen(['python3','-c',program],start_new_session=True)
while not pathlib.Path(os.environ['READY']+'.escaped').exists(): time.sleep(.01)
pathlib.Path(os.environ['READY']).write_text(str(os.getpid()))
print('leader tail',flush=True)
raise SystemExit(3)
"#;
    let mut owned = f.spawn(script, "escaped", None);
    f.ready();
    let end = Instant::now() + Duration::from_secs(3);
    let status = loop {
        if let Some(status) = owned.child.try_wait().expect("wrapper wait") {
            break status;
        }
        assert!(Instant::now() < end, "escaped pipe must not retain wrapper");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(status.code(), Some(3));
    assert_eq!(f.exit().code, Some(3));
    assert!(
        f.log()
            .contains("\ts\t자식 종료 뒤 남은 출력 대기를 중단했습니다")
    );
    assert!(f.log().contains("\to\tleader tail\n"));
    let pid = std::fs::read_to_string(&escaped.0)
        .expect("owned escaped pid")
        .parse::<i32>()
        .expect("pid");
    assert!(
        kill(Pid::from_raw(pid), None).is_ok(),
        "wrapper must not chase processes outside its group"
    );
}
