//! Rename capture runs under daemon/client admission and remains accounted until actual exit.
use crate::{
    LspError, RelayMessage, WorkspaceBaseline, actor::Command, adapters::CaptureAccess,
    relay::Relay,
};
use devbox_documents::{CaptureReason, CodeOmission};
use serde_json::Value;
use std::collections::BTreeMap;
use tokio::sync::{Semaphore, mpsc};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
static WORKERS: Semaphore = Semaphore::const_new(4);
const METADATA_LIMIT: usize = 4 * 1024 * 1024;

struct Pending {
    job: u64,
    generation: u64,
    message: RelayMessage,
    request: Value,
    open: BTreeMap<String, WorkspaceBaseline>,
    cancel: CancellationToken,
}
#[derive(Default)]
pub(crate) struct Captures {
    next: u64,
    jobs: BTreeMap<u32, Pending>,
}
pub(crate) struct CaptureTask<'a> {
    pub access: CaptureAccess,
    pub commands: mpsc::Sender<Command>,
    pub tasks: &'a TaskTracker,
    pub stop: &'a CancellationToken,
}
impl Captures {
    pub fn cancel(&self, client: u32, request: Option<&Value>) {
        if let Some(job) = self.jobs.get(&client)
            && request.is_none_or(|id| id == &job.request)
        {
            job.cancel.cancel();
        }
    }
    pub fn cancel_all(&self) {
        for job in self.jobs.values() {
            job.cancel.cancel();
        }
    }
    pub fn start(
        &mut self,
        client: u32,
        generation: u64,
        message: RelayMessage,
        relay: &mut Relay,
        task: CaptureTask<'_>,
    ) -> Result<(), (Value, LspError)> {
        let CaptureTask {
            access,
            commands,
            tasks,
            stop,
        } = task;
        let value: Value =
            serde_json::from_str(&message.json).map_err(|_| (Value::Null, LspError::Protocol))?;
        let request = value
            .get("id")
            .filter(|id| id.as_i64().is_some() || id.as_str().is_some_and(|s| s.len() <= 256))
            .cloned()
            .ok_or((Value::Null, LspError::Protocol))?;
        if self.jobs.contains_key(&client) || !relay.capture_allowed(client) {
            return Err((request, LspError::LimitExceeded));
        }
        let permit = WORKERS
            .try_acquire()
            .map_err(|_| (request.clone(), LspError::LimitExceeded))?;
        self.next += 1;
        let job = self.next;
        let cancel = stop.child_token();
        self.jobs.insert(
            client,
            Pending {
                job,
                generation,
                message,
                request,
                open: relay.documents.snapshots(client),
                cancel: cancel.clone(),
            },
        );
        let priority = relay.take_capture_hints(client);
        let shutdown = stop.clone();
        tasks.spawn(async move {
            // Do not select/drop this future on cancellation: its blocking worker must exit.
            let result = access(priority, cancel.clone()).await;
            drop(permit);
            let completed = Command::Captured {
                client,
                job,
                result,
            };
            // Ordinary cancellation still needs durable completion accounting. Only
            // actor shutdown can discard a completion after the worker really exits.
            tokio::select! { _ = shutdown.cancelled() => {}, _ = commands.send(completed) => {} }
        });
        Ok(())
    }
    pub fn complete(
        &mut self,
        client: u32,
        job: u64,
        generation: u64,
        result: Result<devbox_documents::CodeCapture, LspError>,
        relay: &mut Relay,
    ) -> Result<Option<RelayMessage>, Value> {
        if self.jobs.get(&client).is_none_or(|p| p.job != job) {
            return Ok(None);
        }
        let pending = self.jobs.remove(&client).expect("matched job");
        if pending.cancel.is_cancelled() || pending.generation != generation {
            return Ok(None);
        }
        let capture = result.map_err(|_| pending.request.clone())?;
        if crate::uri::file_uri(&capture.root).as_deref() != Some(relay.root())
            || relay.documents.snapshots(client) != pending.open
        {
            return Err(pending.request);
        }
        let mut covered = BTreeMap::new();
        let mut omitted = Vec::new();
        let mut metadata: usize = pending.open.values().map(baseline_bytes).sum();
        let mut stopped = capture.stopped;
        for baseline in capture.covered {
            let Some(uri) = crate::uri::file_uri(&capture.root.join(&baseline.key.path)) else {
                continue;
            };
            if !relay.documents.compatible_disk(&uri, &baseline.text_hash) {
                if !retain_omission(
                    &mut omitted,
                    &mut metadata,
                    &mut stopped,
                    CodeOmission {
                        key: baseline.key,
                        reason: CaptureReason::Changed,
                    },
                ) {
                    break;
                }
                continue;
            }
            let baseline = WorkspaceBaseline {
                uri,
                disk_rev: Some(baseline.rev),
                text_hash: baseline.text_hash,
                lease_id: None,
                version: None,
            };
            let size = baseline_bytes(&baseline);
            if metadata.saturating_add(size) > METADATA_LIMIT {
                stopped = Some(devbox_documents::CaptureReason::MetadataLimit);
                break;
            }
            metadata += size;
            covered.insert(baseline.uri.clone(), baseline);
        }
        let mut proof = crate::workspace::captured(client, pending.request, pending.open, covered);
        for omission in capture.omitted {
            if !retain_omission(&mut omitted, &mut metadata, &mut stopped, omission) {
                break;
            }
        }
        proof.omitted = omitted;
        proof.stopped = stopped;
        relay.proofs.insert(generation, proof);
        Ok(Some(pending.message))
    }
}
/// Only dispatch recognizes this method. A server cannot invoke capture or create a proof.
pub(crate) fn method(message: &RelayMessage) -> Option<(String, Option<Value>)> {
    let value: Value = serde_json::from_str(&message.json).ok()?;
    Some((
        value.get("method")?.as_str()?.into(),
        value.pointer("/params/id").cloned(),
    ))
}

fn baseline_bytes(baseline: &WorkspaceBaseline) -> usize {
    2 * baseline.uri.len()
        + baseline.text_hash.len()
        + baseline.lease_id.as_ref().map_or(0, String::len)
        + baseline
            .disk_rev
            .as_ref()
            .map_or(0, |r| r.hash.len() + r.mtime_ns.len() + r.ino.len())
        + 128
}

/// Scanner and relay-side exclusions share the retained-proof metadata budget.
fn retain_omission(
    omitted: &mut Vec<CodeOmission>,
    metadata: &mut usize,
    stopped: &mut Option<CaptureReason>,
    omission: CodeOmission,
) -> bool {
    let size = omission
        .key
        .path
        .len()
        .saturating_add(omission.key.root.len())
        .saturating_add(64);
    if metadata.saturating_add(size) > METADATA_LIMIT {
        *stopped = Some(CaptureReason::MetadataLimit);
        return false;
    }
    *metadata += size;
    omitted.push(omission);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn omission_metadata_exact_fit_and_overflow_retain_the_stop_reason() {
        let record = CodeOmission {
            key: devbox_documents::DocKey {
                root: "project:p".into(),
                path: "b.rs".into(),
            },
            reason: CaptureReason::Changed,
        };
        let mut metadata = METADATA_LIMIT - record.key.root.len() - record.key.path.len() - 64;
        let mut omitted = vec![];
        let mut stopped = None;
        assert!(retain_omission(
            &mut omitted,
            &mut metadata,
            &mut stopped,
            record.clone()
        ));
        assert_eq!(metadata, METADATA_LIMIT);
        assert_eq!(stopped, None);
        assert!(!retain_omission(
            &mut omitted,
            &mut metadata,
            &mut stopped,
            record
        ));
        assert_eq!(metadata, METADATA_LIMIT);
        assert_eq!(omitted.len(), 1, "unbudgeted record is not retained");
        assert_eq!(omitted[0].reason, CaptureReason::Changed);
        assert_eq!(stopped, Some(CaptureReason::MetadataLimit));
    }
}
