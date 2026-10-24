use crate::{
    input::{Reader, load},
    model::*,
    osv::Auditor,
};
use std::sync::{Arc, Mutex};
pub struct DepsService {
    auditor: Arc<Auditor>,
    emit: Arc<dyn Fn(AuditProgress) + Send + Sync>,
    jobs: Mutex<Vec<AuditJob>>,
}
impl DepsService {
    pub fn new(auditor: Auditor, emit: Arc<dyn Fn(AuditProgress) + Send + Sync>) -> Arc<Self> {
        Arc::new(Self {
            auditor: Arc::new(auditor),
            emit,
            jobs: Mutex::new(vec![]),
        })
    }
    pub fn start(
        self: &Arc<Self>,
        project: String,
        inventory: DependencyInventory,
        reader: Reader,
    ) -> Result<AuditJob, DepsError> {
        let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        if jobs
            .iter()
            .any(|j| j.project_id == project && j.state == AuditState::Running)
            || jobs
                .iter()
                .filter(|j| j.state == AuditState::Running)
                .count()
                >= 4
        {
            return Err(DepsError::Busy);
        }
        let last_success = jobs
            .iter()
            .rev()
            .find(|j| j.project_id == project)
            .and_then(|j| j.result.clone().or_else(|| j.last_success.clone()));
        let job = AuditJob {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: project.clone(),
            fingerprint: inventory.fingerprint.clone(),
            state: AuditState::Running,
            completed: 0,
            total: 0,
            cached: false,
            error: None,
            result: None,
            last_success,
        };
        if jobs.len() >= 32
            && let Some(index) = jobs.iter().position(|j| j.state != AuditState::Running)
        {
            jobs.remove(index);
        }
        jobs.push(job.clone());
        drop(jobs);
        self.notify(&job);
        let this = self.clone();
        let id = job.id.clone();
        tokio::spawn(async move {
            let s = this.clone();
            let progress_id = id.clone();
            let progress = Arc::new(move |completed, total| {
                s.update(
                    &progress_id,
                    |j| {
                        j.completed = completed;
                        j.total = total;
                    },
                    completed == total || completed % 100 == 0,
                );
            });
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(180),
                this.auditor.audit(&project, &inventory, progress),
            )
            .await
            .unwrap_or(Err(AuditFailure::Network));
            let result = match result {
                Ok(value) => match load(reader).await {
                    Ok(current) if current.fingerprint == inventory.fingerprint => Ok(value),
                    _ => Err(AuditFailure::InputChanged),
                },
                error => error,
            };
            this.update(
                &id,
                |j| match result {
                    Ok((report, cached)) => {
                        j.state = AuditState::Succeeded;
                        j.cached = cached;
                        j.result = Some(report);
                        j.last_success = None;
                    }
                    Err(error) => {
                        j.state = AuditState::Failed;
                        j.error = Some(error);
                    }
                },
                true,
            );
        });
        Ok(job)
    }
    pub fn status(&self, project: &str, id: &str) -> Result<AuditJob, DepsError> {
        self.jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|j| j.project_id == project && j.id == id)
            .cloned()
            .ok_or(DepsError::JobNotFound)
    }
    fn update(&self, id: &str, change: impl FnOnce(&mut AuditJob), notify: bool) {
        let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(job) = jobs.iter_mut().find(|j| j.id == id) {
            change(job);
            let event = notify.then(|| Self::event(job));
            drop(jobs);
            if let Some(event) = event {
                (self.emit)(event);
            }
        }
    }
    fn event(j: &AuditJob) -> AuditProgress {
        AuditProgress {
            id: j.id.clone(),
            project_id: j.project_id.clone(),
            state: j.state,
            completed: j.completed,
            total: j.total,
        }
    }
    fn notify(&self, j: &AuditJob) {
        (self.emit)(Self::event(j));
    }
}
