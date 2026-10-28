//! I1 structural contract: actual Windows API/PowerShell behaviour is S7-14 W1 acceptance.
use super::*;
const BOOTSTRAP: &str = include_str!("bootstrap.ps1");
const HOLDER: &str = include_str!("holder.ps1");
const NATIVE: &str = include_str!("native.ps1");
fn at(script: &str, needle: &str) -> usize {
    script
        .find(needle)
        .unwrap_or_else(|| panic!("missing {needle}"))
}
fn after(script: &str, from: usize, needle: &str) -> usize {
    from + at(&script[from..], needle)
}
/// One C# member, from its signature to the first closing brace at member indentation.
fn member<'a>(script: &'a str, signature: &str) -> &'a str {
    let start = at(script, signature);
    &script[start..after(script, start, "\n    }\n")]
}
#[test]
fn bootstrap_starts_the_holder_before_job_assignment() {
    let create = at(
        BOOTSTRAP,
        "[DevboxObservation]::CreateFresh([string]$o.jobName)",
    );
    let start = at(BOOTSTRAP, "[DevboxObservation]::StartHolder($devboxHolder,");
    let assign = at(BOOTSTRAP, "[DevboxObservation]::AssociateFresh($jobHandle)");
    assert!(
        create < start && start < assign,
        "the holder must start outside the job"
    );
    // Only validated control values reach the holder: the nonce-derived job and private paths.
    let call = &BOOTSTRAP[start..after(BOOTSTRAP, start, "\n")];
    assert!(call.contains(
        ",[string]$o.jobName,[string]$o.nonce,[string]$o.holderReadyPath,[string]$o.emptiedPath)"
    ));
    let checked = at(BOOTSTRAP, "SafePath([string]$path)");
    assert!(checked < start);
    assert!(BOOTSTRAP[..checked].contains("$o.closedPath,$o.holderReadyPath,$o.emptiedPath"));
    // A marker or receipt that exists before this attempt's holder is never trusted.
    let fresh = at(BOOTSTRAP, "Exists($o.holderReadyPath)");
    assert!(fresh < create && BOOTSTRAP[fresh..create].contains("Exists($o.emptiedPath)"));
    // The job is still created fresh: an existing name is refused.
    let fresh_job = member(NATIVE, "public static IntPtr CreateFresh(string name) {");
    assert!(fresh_job.contains("error==183") && !fresh_job.contains("AssignProcessToJobObject"));
    let assign_job = member(NATIVE, "public static void AssociateFresh(IntPtr job) {");
    assert!(assign_job.contains("AssignProcessToJobObject(job,GetCurrentProcess())"));
}
#[test]
fn holder_starts_without_inherited_handles_or_a_window() {
    let start = member(
        NATIVE,
        "public static bool StartHolder(string script,string job,string nonce,string ready,string receipt) {",
    );
    // bInheritHandles=false: an inherited stdio or WSL interop pipe would hold the caller's
    // EOF open for the holder's lifetime. CREATE_NO_WINDOW (0x08000000) shows no window.
    assert!(start.contains(
        "CreateProcess(shell,command,IntPtr.Zero,IntPtr.Zero,false,0x08000000,IntPtr.Zero,Environment.SystemDirectory,ref startup,out started)"
    ));
    assert!(!start.contains("STARTF_USESTDHANDLES") && !start.contains("0x100,"));
    assert!(start.contains("\"\\\" -NoProfile -NonInteractive -EncodedCommand \""));
    // Values are base64 literals, so no path character can change the holder script.
    assert!(start.contains("Convert.ToBase64String(new UTF8Encoding(false).GetBytes(values[i]))"));
    assert!(start.contains("CloseHandle(started.Thread); CloseHandle(started.Process);"));
}
#[test]
fn bootstrap_waits_for_holder_ready_before_assignment() {
    let start = at(BOOTSTRAP, "::StartHolder(");
    let ready = after(
        BOOTSTRAP,
        start,
        "[DevboxObservation]::Exists($o.holderReadyPath)",
    );
    let assign = at(BOOTSTRAP, "::AssociateFresh($jobHandle)");
    let assigned = at(BOOTSTRAP, "::WriteNew([string]$o.assignedPath,");
    let command = at(BOOTSTRAP, "ScriptBlock]::Create");
    let close = at(BOOTSTRAP, "CloseHandle($jobHandle)");
    assert!(start < ready && ready < assign && assign < assigned);
    assert!(assigned < command && command < close);
    // The wait is a bounded poll, not a single check.
    assert!(BOOTSTRAP[start..assign].contains("while("));
    assert!(BOOTSTRAP[start..assign].contains("Start-Sleep -Milliseconds 50"));
}
#[test]
fn bootstrap_fails_with_observer_unavailable_when_the_holder_is_not_ready() {
    let start = at(BOOTSTRAP, "::StartHolder(");
    let assign = at(BOOTSTRAP, "::AssociateFresh($jobHandle)");
    let wait = &BOOTSTRAP[start..assign];
    assert_eq!(OBSERVATION_HOLDER_READY_TIMEOUT_MS, 15_000);
    assert!(wait.contains(&format!(
        "ElapsedMilliseconds -lt {OBSERVATION_HOLDER_READY_TIMEOUT_MS}"
    )));
    let unavailable = format!("{{ exit {OBSERVER_UNAVAILABLE_EXIT} }}");
    assert!(
        wait.contains(&unavailable),
        "fixed status before any command"
    );
    // Distinct from success, the generic bootstrap failure and the command's own result path.
    assert!((2..=255).contains(&OBSERVER_UNAVAILABLE_EXIT));
    assert_eq!(BOOTSTRAP.matches(&unavailable).count(), 1);
    assert!(BOOTSTRAP.contains("exit 1"));
}
#[test]
fn holder_queries_and_never_sets_job_information() {
    // JOB_OBJECT_QUERY (4) only: the handle keeps the name and cannot change the job.
    assert!(HOLDER.contains("OpenJobObject(4,false,name)"));
    assert!(HOLDER.contains("QueryInformationJobObject(job,1,out account,size,IntPtr.Zero)"));
    for banned in [
        "SetInformationJobObject",
        "TerminateJobObject",
        "AssignProcessToJobObject",
        "CreateJobObject",
        "LimitFlags",
        "CreateProcess",
    ] {
        assert!(!HOLDER.contains(banned), "{banned}");
    }
    for script in [POWERSHELL_BOOTSTRAP, POWERSHELL_OBSERVATION_PROBE] {
        for banned in [
            "SetInformationJobObject(",
            "TerminateJobObject(",
            "LimitFlags",
        ] {
            assert!(!script.contains(banned), "{banned}");
        }
    }
}
#[test]
fn holder_counts_total_processes_so_a_short_command_still_leaves_a_receipt() {
    // TotalProcesses counts every process ever assigned, so a command that ends between two
    // 500 ms samples is still seen. A sampled Active>=1 would miss it.
    assert!(HOLDER.contains("account.Total>=1 && account.Active==0"));
    assert!(HOLDER.contains(&format!(
        "account.Total==0 && timer.ElapsedMilliseconds>={OBSERVATION_HOLDER_IDLE_MS}"
    )));
    assert!(HOLDER.contains(&format!("Thread.Sleep({OBSERVATION_HOLDER_POLL_MS})")));
    assert_eq!(
        (OBSERVATION_HOLDER_POLL_MS, OBSERVATION_HOLDER_IDLE_MS),
        (500, 60_000)
    );
    assert!(!HOLDER.contains("Active>=1") && !HOLDER.contains("Active>0"));
    // A job that cannot be opened leaves at once, without a marker or receipt.
    let hold = at(HOLDER, "public static void Hold(");
    let open = after(HOLDER, hold, "OpenJobObject(4,false,name)");
    let gone = after(HOLDER, open, "if(job==IntPtr.Zero) return;");
    assert!(gone < after(HOLDER, hold, "WriteNew(ready,"));
}
#[test]
fn holder_writes_the_receipt_with_create_new() {
    let hold = at(HOLDER, "public static void Hold(");
    let open = after(HOLDER, hold, "OpenJobObject(4,false,name)");
    let ready = after(HOLDER, hold, "WriteNew(ready,Identity(nonce))");
    let emptied = after(HOLDER, hold, "account.Total>=1 && account.Active==0");
    let receipt = after(HOLDER, hold, "WriteNew(receipt,body)");
    assert!(open < ready && ready < emptied && emptied < receipt);
    assert_eq!(HOLDER.matches("WriteNew(receipt,").count(), 1);
    assert!(HOLDER.contains(r#"string body="{\"nonce\":\""+nonce+"\"}";"#));
    // The handle stays open until the receipt is written, so the name outlives the receipt.
    assert!(after(HOLDER, receipt, "finally { CloseHandle(job); }") > receipt);
    // The bootstrap's own CreateNew publication and reparse rules, unchanged.
    let write = "public static void WriteNew(string path,string json) {";
    assert_eq!(member(HOLDER, write), member(NATIVE, write));
    assert!(member(HOLDER, write).contains("FileMode.CreateNew"));
    assert!(member(HOLDER, write).contains("File.Move(temporary,path)"));
    let safe = "public static bool SafePath(string path) {";
    assert_eq!(
        member(HOLDER, safe),
        member(NATIVE, safe).replace("'\\\\'", "Path.DirectorySeparatorChar")
    );
}
#[test]
fn holder_script_is_embedded_verbatim_in_the_bootstrap_only() {
    assert!(POWERSHELL_BOOTSTRAP.contains(&format!("$devboxHolder=@'\n{HOLDER}\n'@\n")));
    assert!(!POWERSHELL_OBSERVATION_PROBE.contains("$devboxHolder"));
    // A line starting with '@ would end the literal early, and the C# source is a
    // single-quoted PowerShell string.
    assert!(!HOLDER.lines().any(|line| line.starts_with("'@")));
    let open = "Add-Type -TypeDefinition '\n";
    let source = at(HOLDER, open) + open.len();
    let end = after(HOLDER, source, "\n'\n");
    assert!(!HOLDER[source..end].contains('\''));
    assert!(HOLDER[end..].contains("[DevboxHolder]::Hold($job,$nonce,$ready,$receipt)"));
    // The whole bootstrap is one CreateProcess command-line argument (32,767 characters).
    assert!(POWERSHELL_BOOTSTRAP.len() < 24 * 1024);
}
#[test]
fn windows_control_requires_distinct_holder_paths_in_the_same_private_directory() {
    assert!(control().valid());
    for bad in [
        "",
        "relative",
        "C:\\owned\\..\\control\\x",
        "C:\\other\\x",
        "C:\\owned\\control\\grant.json",
        "C:\\owned\\control\\assigned.json",
    ] {
        let mut ready = control();
        ready.holder_ready_path = bad.into();
        assert!(!ready.valid(), "ready {bad:?}");
        let mut emptied = control();
        emptied.emptied_path = bad.into();
        assert!(!emptied.valid(), "emptied {bad:?}");
    }
    let mut same = control();
    same.emptied_path = same.holder_ready_path.clone();
    assert!(!same.valid());
    let wire = serde_json::to_string(&control()).expect("control");
    assert!(wire.contains(r#""holderReadyPath":"C:\\owned\\control\\holder-ready""#));
    assert!(wire.contains(r#""emptiedPath":"C:\\owned\\control\\emptied.json""#));
}
#[test]
fn an_rc1_control_decodes_as_legacy_and_can_never_launch() {
    let mut legacy = serde_json::to_value(control()).expect("control");
    let fields = legacy.as_object_mut().expect("object");
    fields.remove("holderReadyPath");
    fields.remove("emptiedPath");
    let stored: ObservationControl =
        serde_json::from_value(legacy).expect("an older attempt row stays readable");
    assert!(stored.holder_ready_path.is_empty() && stored.emptied_path.is_empty());
    assert_eq!(stored.shape(), Some(ControlShape::Legacy));
    let run = RunIdentity {
        run_id: "run".into(),
        attempt_id: "attempt".into(),
    };
    // Still observed under rc.1's rule, but never valid for a new command or grant.
    assert_eq!(
        stored.observes(&run, &assigned()),
        Some(ControlShape::Legacy)
    );
    assert!(!stored.valid() && !stored.accepts(&run, &assigned()));
    let launch = RunWindows {
        run_id: "run".into(),
        attempt_id: "attempt".into(),
        cwd: "C:\\owned".into(),
        command: "exit 0".into(),
        env: Default::default(),
        ports: vec![],
        observation: stored,
    };
    assert!(
        !launch.tracked(),
        "no rc.1-shaped control can start a command"
    );
}
#[path = "holder_liveness_tests.rs"]
mod liveness;
