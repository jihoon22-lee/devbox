//! A private inherited-pipe helper. EOF cancels work before atomic replacement.
#[cfg(target_os = "linux")]
fn main() {
    use std::{
        io,
        sync::{
            atomic::{AtomicU32, AtomicU64, Ordering},
            mpsc, Arc,
        },
        time::{Duration, Instant},
    };
    use workspace_wsl::{read_frame, write_frame, Request, Response};
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 || args[0] != "--session" || !workspace_wsl::token(&args[1]) {
        std::process::exit(2);
    }
    let session = args[1].clone();
    let started = Instant::now();
    let deadline = Arc::new(AtomicU64::new(0));
    // 0 live, 1 orderly EOF/cancel, 2 malformed stream, 124 deadline.
    let stop = Arc::new(AtomicU32::new(0));
    let watchdog_deadline = deadline.clone();
    let watchdog_stop = stop.clone();
    std::thread::spawn(move || {
        let mut retiring = None;
        loop {
            let due = watchdog_deadline.load(Ordering::Acquire);
            if due != 0 && started.elapsed().as_millis() >= u128::from(due) {
                let _ = watchdog_stop.compare_exchange(0, 124, Ordering::AcqRel, Ordering::Acquire);
            }
            let code = watchdog_stop.load(Ordering::Acquire);
            if code != 0 {
                let since = retiring.get_or_insert_with(Instant::now);
                if since.elapsed() >= Duration::from_secs(5) {
                    // Only read/atomic-file operations exist here. A blocked
                    // syscall cannot turn the old/new-file boundary into an
                    // in-place partial write. Never add child execution without
                    // a separate confirmed process-retirement owner.
                    std::process::exit(if code == 1 { 0 } else { code as i32 });
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader_stop = stop.clone();
    std::thread::spawn(move || {
        let mut input = io::stdin().lock();
        loop {
            let code = match read_frame::<_, Request>(&mut input) {
                Ok(Some(request)) => {
                    if sender.try_send(request).is_ok() {
                        continue;
                    }
                    2
                }
                Ok(None) => 1,
                Err(_) => 2,
            };
            let _ = reader_stop.compare_exchange(0, code, Ordering::AcqRel, Ordering::Acquire);
            break;
        }
        // Disconnect wakes an idle owner; an active save observes the shared
        // cancellation at its precommit guard and drops only its owned temp.
    });
    let mut engine = workspace_wsl::engine::Engine::default();
    let mut sequence = 0u64;
    let mut output = io::stdout().lock();
    for request in receiver {
        if stop.load(Ordering::Acquire) != 0 {
            break;
        }
        if request.validate(&session).is_err() || sequence.checked_add(1) != Some(request.sequence)
        {
            stop.store(2, Ordering::Release);
            break;
        }
        sequence = request.sequence;
        let due = started.elapsed().as_millis() as u64 + u64::from(request.budget_ms);
        deadline.store(due, Ordering::Release);
        let guard = || {
            if stop.load(Ordering::Acquire) != 0 {
                Err("wsl_request_cancelled")
            } else if started.elapsed().as_millis() >= u128::from(due) {
                Err("wsl_timeout")
            } else {
                Ok(())
            }
        };
        let result = engine
            .dispatch_guarded(&request, &guard)
            .map_err(str::to_owned);
        if stop.load(Ordering::Acquire) != 0 {
            break;
        }
        let response = Response {
            version: workspace_wsl::VERSION,
            session_id: session.clone(),
            request_id: request.request_id.clone(),
            sequence,
            result,
        };
        if write_frame(&mut output, &response).is_err() {
            stop.store(1, Ordering::Release);
            break;
        }
        deadline.store(0, Ordering::Release);
    }
    // Graceful completion drops native grants and any staging owner first.
    drop(engine);
    drop(output);
    let code = stop.load(Ordering::Acquire);
    if code > 1 {
        std::process::exit(code as i32);
    }
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This private Workspace helper requires Linux.");
    std::process::exit(2);
}
