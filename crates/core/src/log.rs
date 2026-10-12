//! Process-wide logging. Keep the returned guard alive until shutdown.
use crate::{CoreError, Paths};
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io::Write as _;
use std::sync::{Mutex, OnceLock};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::{EnvFilter, Layer};

const RECENT: usize = 50;
static INIT: Mutex<bool> = Mutex::new(false);
static RING: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
fn ring() -> &'static Mutex<VecDeque<String>> {
    RING.get_or_init(|| Mutex::new(VecDeque::with_capacity(RECENT)))
}
struct RecentLayer;
struct Fields(String);
impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let _ = write!(self.0, " {}={value:?}", field.name());
    }
}
impl<S: tracing::Subscriber> Layer<S> for RecentLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        // User Debug implementations may panic: format before taking the ring lock.
        let mut fields = Fields(format!(
            "{} {}",
            event.metadata().level(),
            event.metadata().target()
        ));
        event.record(&mut fields);
        let mut queue = ring().lock().unwrap_or_else(|poison| poison.into_inner());
        if queue.len() == RECENT {
            queue.pop_front();
        }
        queue.push_back(fields.0);
    }
}

/// Flushes the non-blocking file writer on drop; retain it for the process lifetime.
#[must_use = "keep the logging guard alive until shutdown"]
pub struct LogGuard {
    _worker: tracing_appender::non_blocking::WorkerGuard,
}

pub fn init_logging(paths: &Paths, stderr: bool) -> Result<LogGuard, CoreError> {
    let mut initialized = INIT.lock().unwrap_or_else(|poison| poison.into_inner());
    if *initialized {
        return Err(CoreError::Io {
            what: "install logging subscriber".into(),
            source: std::io::Error::other("a global subscriber is already installed"),
        });
    }
    std::fs::create_dir_all(&paths.log_dir).map_err(crate::io_err("create log dir"))?;
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("daemon")
        .filename_suffix("log")
        .max_log_files(7)
        .build(&paths.log_dir)
        .map_err(|error| CoreError::Io {
            what: "log appender".into(),
            source: std::io::Error::other(error),
        })?;
    let (writer, worker) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let file = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(writer);
    let console = stderr.then(|| tracing_subscriber::fmt::layer().with_writer(std::io::stderr));
    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(RecentLayer)
        .with(file)
        .with(console);
    tracing::subscriber::set_global_default(subscriber).map_err(|error| CoreError::Io {
        what: "install logging subscriber".into(),
        source: std::io::Error::other(error.to_string()),
    })?;
    *initialized = true;
    Ok(LogGuard { _worker: worker })
}

pub fn install_panic_hook(paths: &Paths, version: &'static str) {
    let dir = paths.log_dir.clone();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let mut text = format!(
            "version: {version}\nthread: {}\nlocation: {}\nmessage: {}\nrecent:\n",
            thread.name().unwrap_or("unnamed"),
            info.location()
                .map(|location| location.to_string())
                .unwrap_or_default(),
            info.payload()
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
                .unwrap_or("non-string panic payload")
        );
        // Never wait for a lock from a panic hook, including a reentrant panic.
        append_recent(&mut text, ring());
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        write_crash(&dir, ms, &text);
        let _ = writeln!(
            std::io::stderr().lock(),
            "devbox panicked; see {}",
            dir.display()
        );
    }));
}

fn append_recent(text: &mut String, recent: &Mutex<VecDeque<String>>) {
    match recent.try_lock() {
        Ok(queue) => {
            for line in queue.iter() {
                text.push_str("  ");
                text.push_str(line);
                text.push('\n');
            }
        }
        Err(_) => text.push_str("recent events unavailable (ring busy or poisoned)\n"),
    }
}

fn write_crash(dir: &std::path::Path, ms: u128, text: &str) {
    for suffix in 0..1024 {
        let name = if suffix == 0 {
            format!("crash-{ms}.log")
        } else {
            format!("crash-{ms}-{suffix}.log")
        };
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(name))
        {
            Ok(mut file) => {
                let _ = file.write_all(text.as_bytes());
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn busy_ring_never_blocks_the_panic_snapshot() {
        let recent = super::Mutex::new(super::VecDeque::new());
        let _locked = recent.lock().expect("lock fixture state");
        let mut text = String::new();
        super::append_recent(&mut text, &recent);
        assert!(text.contains("unavailable"));
    }

    #[test]
    fn collision_preserves_existing_crash_files() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        std::fs::write(temp.path().join("crash-42.log"), "existing").expect("write fixture file");
        super::write_crash(temp.path(), 42, "first");
        super::write_crash(temp.path(), 42, "second");
        for (name, content) in [
            ("crash-42.log", "existing"),
            ("crash-42-1.log", "first"),
            ("crash-42-2.log", "second"),
        ] {
            assert_eq!(
                std::fs::read_to_string(temp.path().join(name)).expect("read fixture text"),
                content
            );
        }
    }
}
