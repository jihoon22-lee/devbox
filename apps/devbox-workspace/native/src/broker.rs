//! The helper's private output/approval owner. A pending approval authorizes one
//! already native-validated Git launch and cannot be replayed for another job.
use std::{
    io::{self, Stdout},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use workspace_wsl::{
    control::{ControlInput, ControlOutput},
    Request,
};
type Result<T> = std::result::Result<T, &'static str>;
const CLOSED: usize = 1 << (usize::BITS - 1);

#[derive(Default, Clone)]
pub struct ProcessGate(Arc<AtomicUsize>);
pub struct ProcessPermit(Arc<AtomicUsize>);
impl ProcessGate {
    pub fn enter(&self) -> Result<Arc<ProcessPermit>> {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                if value < 16 {
                    Some(value + 1)
                } else {
                    None
                }
            })
            .map_err(|_| "wsl_request_cancelled")?;
        Ok(Arc::new(ProcessPermit(self.0.clone())))
    }
    pub fn is_idle(&self) -> bool {
        matches!(self.0.load(Ordering::Acquire), 0 | CLOSED)
    }
    pub fn close_if_idle(&self) -> bool {
        self.0
            .compare_exchange(0, CLOSED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            || self.0.load(Ordering::Acquire) == CLOSED
    }
}
impl Drop for ProcessPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
struct Pending {
    request_id: String,
    sequence: u64,
    admission_id: String,
    sender: mpsc::SyncSender<bool>,
}
pub struct Broker {
    session: String,
    pub output: Mutex<Stdout>,
    serial: Mutex<()>,
    pending: Mutex<Option<Pending>>,
    cancelled: Arc<AtomicBool>,
}
impl Broker {
    pub fn new(session: String, cancelled: Arc<AtomicBool>) -> Arc<Self> {
        Arc::new(Self {
            session,
            output: Mutex::new(io::stdout()),
            serial: Mutex::new(()),
            pending: Mutex::new(None),
            cancelled,
        })
    }
    pub fn reply(&self, reply: ControlInput) -> Result<()> {
        let ControlInput::AdmissionReply {
            version,
            session_id,
            request_id,
            sequence,
            admission_id,
            approved,
        } = reply;
        let mut pending = self.pending.lock().map_err(|_| "wsl_protocol_invalid")?;
        let expected = pending.as_ref().ok_or("wsl_protocol_invalid")?;
        if version != workspace_wsl::VERSION
            || session_id != self.session
            || request_id != expected.request_id
            || sequence != expected.sequence
            || admission_id != expected.admission_id
        {
            return Err("wsl_protocol_invalid");
        }
        let expected = pending.take().ok_or("wsl_protocol_invalid")?;
        expected
            .sender
            .try_send(approved)
            .map_err(|_| "wsl_protocol_invalid")
    }
    pub fn authorization(
        self: &Arc<Self>,
        request: &Request,
        operation_cancelled: Arc<AtomicBool>,
    ) -> workspace_wsl::engine::SourceAuthorization {
        let broker = self.clone();
        let request_id = request.request_id.clone();
        let sequence = request.sequence;
        let expires = Instant::now() + Duration::from_millis(u64::from(request.budget_ms));
        Arc::new(move |root| {
            broker.authorize(&request_id, sequence, root, expires, &operation_cancelled)
        })
    }
    fn authorize(
        &self,
        request: &str,
        sequence: u64,
        root: &str,
        expires: Instant,
        operation_cancelled: &AtomicBool,
    ) -> Result<()> {
        let boundary = || {
            if self.cancelled.load(Ordering::Acquire) || operation_cancelled.load(Ordering::Acquire)
            {
                Err("wsl_request_cancelled")
            } else if Instant::now() >= expires {
                Err("wsl_timeout")
            } else {
                Ok(())
            }
        };
        boundary()?;
        // Serialize approval handshakes even if the domain dispatch has several
        // native workers. The pipe retains at most one approval ticket at once.
        let _serial = self.serial.lock().map_err(|_| "wsl_protocol_invalid")?;
        boundary()?;
        let admission_id = uuid::Uuid::new_v4().to_string();
        let (sender, receiver) = mpsc::sync_channel(1);
        *self.pending.lock().map_err(|_| "wsl_protocol_invalid")? = Some(Pending {
            request_id: request.into(),
            sequence,
            admission_id: admission_id.clone(),
            sender,
        });
        let result = (|| {
            workspace_wsl::write_frame(
                &mut *self.output.lock().map_err(|_| "wsl_protocol_invalid")?,
                &ControlOutput::Admission {
                    version: workspace_wsl::VERSION,
                    session_id: self.session.clone(),
                    request_id: request.into(),
                    sequence,
                    admission_id,
                    target_root: root.into(),
                },
            )
            .map_err(|_| "wsl_connection_closed")?;
            loop {
                boundary()?;
                match receiver.recv_timeout(Duration::from_millis(5)) {
                    Ok(true) => {
                        boundary()?;
                        return Ok(());
                    }
                    Ok(false) => return Err("source_review_required"),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => return Err("wsl_request_cancelled"),
                }
            }
        })();
        // Removal also consumes a denied/cancelled ticket. A late ACK cannot
        // release another command waiting for its own native authorization.
        self.pending
            .lock()
            .map_err(|_| "wsl_protocol_invalid")?
            .take();
        result
    }
    pub fn retired(&self, sequence: u64) -> io::Result<()> {
        let mut output = self
            .output
            .lock()
            .map_err(|_| io::Error::other("output unavailable"))?;
        workspace_wsl::write_frame(
            &mut *output,
            &ControlOutput::Retired {
                version: workspace_wsl::VERSION,
                session_id: self.session.clone(),
                sequence,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hard_exit_admission_cannot_cross_a_retained_native_owner() {
        let gate = ProcessGate::default();
        let first = gate.enter().unwrap();
        let worker = first.clone();
        assert!(!gate.close_if_idle());
        drop(first);
        assert!(!gate.close_if_idle());
        drop(worker);
        assert!(gate.close_if_idle());
        assert!(gate.enter().is_err());
    }
}
