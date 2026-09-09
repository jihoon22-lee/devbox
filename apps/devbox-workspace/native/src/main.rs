//! A private, inherited-pipe helper. It never opens a network listener.
#[cfg(target_os = "linux")]
fn main() {
    use std::{
        io,
        sync::{
            atomic::{AtomicU64, Ordering},
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
    let watchdog = deadline.clone();
    std::thread::spawn(move || loop {
        let due = watchdog.load(Ordering::Acquire);
        if due != 0 && started.elapsed().as_millis() >= u128::from(due) {
            std::process::exit(124);
        }
        std::thread::sleep(Duration::from_millis(10));
    });
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut input = io::stdin().lock();
        loop {
            match read_frame::<_, Request>(&mut input) {
                Ok(Some(request)) => {
                    if sender.try_send(request).is_err() {
                        std::process::exit(2);
                    }
                }
                // Initial observation operations have no child processes or
                // writes. EOF also terminates a blocked metadata worker.
                Ok(None) => std::process::exit(0),
                Err(_) => std::process::exit(2),
            }
        }
    });
    let mut engine = workspace_wsl::engine::Engine::default();
    let mut sequence = 0_u64;
    let mut output = io::stdout().lock();
    for request in receiver {
        if request.validate(&session).is_err() || sequence.checked_add(1) != Some(request.sequence)
        {
            std::process::exit(2);
        }
        sequence = request.sequence;
        deadline.store(
            started.elapsed().as_millis() as u64 + u64::from(request.budget_ms),
            Ordering::Release,
        );
        let response = Response {
            version: workspace_wsl::VERSION,
            session_id: session.clone(),
            request_id: request.request_id.clone(),
            sequence,
            result: engine.dispatch(&request).map_err(str::to_owned),
        };
        if write_frame(&mut output, &response).is_err() {
            std::process::exit(1);
        }
        deadline.store(0, Ordering::Release);
    }
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This private Workspace helper requires Linux.");
    std::process::exit(2);
}
