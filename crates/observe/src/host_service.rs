use crate::*;
use devbox_protocol::Internal;
use std::path::PathBuf;
impl ObserveService {
    pub fn with_proc(mut self, proc: ProcFs) -> Self {
        self.proc = proc;
        self
    }
    pub async fn ports(&self, projects: Vec<ProjectRoot>, runs: Vec<UnitRun>) -> R<Vec<PortInfo>> {
        let permit = self
            .host_slots
            .clone()
            .acquire_owned()
            .await
            .internal("acquire host query")?;
        let proc = self.proc.clone();
        let work = tokio::task::spawn_blocking(move || {
            (
                crate::ports::ports_snapshot(&proc, &projects, &runs),
                permit,
            )
        });
        let (rows, _permit) = work.await.internal("read host ports")?;
        let mut rows = rows?;
        rows.extend(crate::docker_ports::snapshot(&self.docker).await);
        rows.sort_by(|a, b| {
            (a.port, &a.addr, &a.container_id).cmp(&(b.port, &b.addr, &b.container_id))
        });
        if serde_json::to_vec(&rows)
            .internal("measure ports response")?
            .len()
            > 768 * 1024
        {
            return Err(ObserveError::LimitExceeded.into());
        }
        Ok(rows)
    }
    pub async fn processes(
        &self,
        p: ProcessesQuery,
        projects: Vec<ProjectRoot>,
        runs: Vec<UnitRun>,
    ) -> R<Vec<ProcessInfo>> {
        let permit = self
            .host_slots
            .clone()
            .acquire_owned()
            .await
            .internal("acquire host query")?;
        let proc = self.proc.clone();
        let sampler = self.process_sampler.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            sampler
                .lock()
                .map_err(|_| devbox_protocol::Fail::Internal("process sampler poisoned".into()))?
                .sample(&proc, &projects, &runs, p.project_id.as_deref())
        })
        .await
        .internal("read processes")?
    }
    pub async fn kill(&self, p: KillProcess) -> R<KillResult> {
        let pid = p.pid;
        let permit = self
            .host_slots
            .clone()
            .acquire_owned()
            .await
            .internal("acquire host mutation")?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            crate::processes::kill_process(p)
        })
        .await
        .internal("signal process")??;
        Ok(KillResult { pid })
    }
    pub async fn system(&self, home: PathBuf) -> R<SystemInfo> {
        let permit = self
            .host_slots
            .clone()
            .acquire_owned()
            .await
            .internal("acquire host query")?;
        let proc = self.proc.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            crate::system::system_snapshot(&proc, &home)
        })
        .await
        .internal("read system resources")?
    }
}
