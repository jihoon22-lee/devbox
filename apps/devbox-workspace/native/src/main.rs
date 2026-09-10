//! A private inherited-pipe helper. Native command owners survive cancellation
//! until their descendants have retired; only then is completion acknowledged.
#[cfg(target_os = "linux")]
mod broker;
#[cfg(target_os = "linux")]
mod supervisor;
#[cfg(target_os = "linux")]
fn main() {
    use std::{
        io,
        sync::{
            atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
            mpsc, Arc, Mutex,
        },
        time::{Duration, Instant},
    };
    use workspace_wsl::{control::Input, read_frame, write_frame, Response};
    let native_args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if native_args.first().is_some_and(|arg| arg == "--supervise") {
        std::process::exit(supervisor::run(&native_args[1..]));
    }
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 || args[0] != "--session" || !workspace_wsl::token(&args[1]) {
        std::process::exit(2);
    }
    let session = args[1].clone();
    let started = Instant::now();
    let deadline = Arc::new(AtomicU64::new(0));
    // 0 live, 1 orderly EOF/cancel, 2 malformed stream, 124 deadline.
    let stop = Arc::new(AtomicU32::new(0));
    let cancelled = Arc::new(AtomicBool::new(false));
    // A domain policy cancels its own request on Drop. It must never poison
    // the session-wide signal used by a later, independent Source request.
    let active = Arc::new(Mutex::new(Arc::new(AtomicBool::new(false))));
    let gate = broker::ProcessGate::default();
    let broker = broker::Broker::new(session.clone(), cancelled.clone());
    let watchdog_deadline = deadline.clone();
    let watchdog_stop = stop.clone();
    let watchdog_cancelled = cancelled.clone();
    let watchdog_active = active.clone();
    let watchdog_gate = gate.clone();
    std::thread::spawn(move || {
        let mut retiring = None;
        loop {
            let due = watchdog_deadline.load(Ordering::Acquire);
            if due != 0 && started.elapsed().as_millis() >= u128::from(due) {
                let _ = watchdog_stop.compare_exchange(0, 124, Ordering::AcqRel, Ordering::Acquire);
            }
            let code = watchdog_stop.load(Ordering::Acquire);
            if code != 0 {
                watchdog_cancelled.store(true, Ordering::Release);
                if let Ok(active) = watchdog_active.lock() {
                    active.store(true, Ordering::Release);
                }
                let since = retiring.get_or_insert_with(Instant::now);
                if since.elapsed() >= Duration::from_secs(5) && watchdog_gate.close_if_idle() {
                    // This CAS also closes future command admission. A blocked
                    // read/atomic replacement can still exit at its old/new
                    // file boundary, but an unretired child owner cannot.
                    std::process::exit(if code == 1 { 0 } else { code as i32 });
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader_stop = stop.clone();
    let reader_cancelled = cancelled.clone();
    let reader_active = active.clone();
    let reader_broker = broker.clone();
    std::thread::spawn(move || {
        let mut input = io::stdin().lock();
        loop {
            let code = match read_frame::<_, Input>(&mut input) {
                Ok(Some(Input::Request(request))) => {
                    if sender.try_send(request).is_ok() {
                        continue;
                    }
                    2
                }
                Ok(Some(Input::Control(reply))) => {
                    if reader_broker.reply(reply).is_ok() {
                        continue;
                    }
                    2
                }
                Ok(None) => 1,
                Err(_) => 2,
            };
            let _ = reader_stop.compare_exchange(0, code, Ordering::AcqRel, Ordering::Acquire);
            reader_cancelled.store(true, Ordering::Release);
            if let Ok(active) = reader_active.lock() {
                active.store(true, Ordering::Release);
            }
            break;
        }
    });
    let mut engine = workspace_wsl::engine::Engine::default();
    let mut sequence = 0u64;
    let mut prepared = false;
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
        let result = if request.method == "execution_prepare" {
            if request.root_token.is_some()
                || !request.args.as_object().is_some_and(|args| args.is_empty())
            {
                Err("wsl_request_invalid")
            } else {
                prepared = true;
                Ok(serde_json::json!({"retirement":true}))
            }
        } else if request.method == "source_execute" && !prepared {
            Err("wsl_execution_required")
        } else if request.method == "source_execute" {
            let operation_cancelled = Arc::new(AtomicBool::new(cancelled.load(Ordering::Acquire)));
            if let Ok(mut active) = active.lock() {
                *active = operation_cancelled.clone();
            }
            let result = gate.enter().and_then(|permit| {
                let authorize = broker.authorization(&request, operation_cancelled.clone());
                engine.execute_source(&request, &guard, operation_cancelled, authorize, permit)
            });
            // The successful/failed result cannot acknowledge ownership release
            // while a domain worker still retains a policy/process permit.
            while !gate.is_idle() {
                std::thread::sleep(Duration::from_millis(5));
            }
            result
        } else {
            engine.dispatch_guarded(&request, &guard)
        }
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
        if broker.output.lock().map_or(true, |mut output| {
            write_frame(&mut *output, &response).is_err()
        }) {
            stop.store(1, Ordering::Release);
            break;
        }
        deadline.store(0, Ordering::Release);
    }
    cancelled.store(true, Ordering::Release);
    if let Ok(active) = active.lock() {
        active.store(true, Ordering::Release);
    }
    drop(engine);
    while !gate.close_if_idle() {
        std::thread::sleep(Duration::from_millis(5));
    }
    // Preparation completes before a command frame is sent. Even a truncated
    // subsequent command therefore receives a no-children retirement proof.
    // Existing file-only peers retain their original EOF contract.
    if prepared {
        let _ = broker.retired(sequence);
    }
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
