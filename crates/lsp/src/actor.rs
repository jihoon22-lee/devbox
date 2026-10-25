use crate::{
    process::{Event, Process},
    relay::{Relay, RelayOutput},
    servers::ServerSpec,
    supervisor::Supervisor,
    wire::Client,
    *,
};
use devbox_protocol::stream::StreamEnds;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
pub(crate) enum Command {
    Attach {
        ends: StreamEnds,
        reply: oneshot::Sender<Result<(), LspError>>,
    },
    Message {
        id: u32,
        message: RelayMessage,
    },
    Captured {
        client: u32,
        job: u64,
        result: Result<devbox_documents::CodeCapture, LspError>,
    },
    Detach {
        id: u32,
    },
    Restart {
        reply: oneshot::Sender<Result<(), LspError>>,
    },
}
#[derive(Clone)]
pub(crate) struct Handle {
    pub root: PathBuf,
    pub commands: mpsc::Sender<Command>,
    pub status: Arc<Mutex<ServerInfo>>,
    pub stop: CancellationToken,
    pub done: CancellationToken,
}
pub(crate) fn spawn(
    project: ProjectContext,
    spec: ServerSpec,
    executable: PathBuf,
    generations: Arc<AtomicU64>,
    stop: CancellationToken,
    tasks: &TaskTracker,
) -> Handle {
    let (commands, receiver) = mpsc::channel(32);
    let status = Arc::new(Mutex::new(ServerInfo {
        language: spec.language,
        command: spec.command.clone(),
        args: spec.args.clone(),
        install: spec.install.clone(),
        state: ServerState::Starting,
        generation: 0,
        pid: None,
        memory_bytes: None,
        clients: 0,
    }));
    let done = CancellationToken::new();
    let handle = Handle {
        root: project.root.clone(),
        commands: commands.clone(),
        status: status.clone(),
        stop: stop.clone(),
        done: done.clone(),
    };
    tasks.spawn(async move {
        let root_uri = crate::uri::file_uri(&project.root).expect("validated canonical root");
        let mut actor = Actor {
            project,
            spec,
            executable,
            generations,
            status,
            stop,
            commands,
            receiver,
            process: None,
            relay: Relay::new(root_uri, 0),
            clients: BTreeMap::new(),
            policy: Supervisor::default(),
            started: Instant::now(),
            retry_at: None,
            init_deadline: None,
            tasks: TaskTracker::new(),
            captures: crate::capture::Captures::default(),
        };
        actor.run().await;
        done.cancel();
    });
    handle
}
struct Actor {
    project: ProjectContext,
    spec: ServerSpec,
    executable: PathBuf,
    generations: Arc<AtomicU64>,
    status: Arc<Mutex<ServerInfo>>,
    stop: CancellationToken,
    commands: mpsc::Sender<Command>,
    receiver: mpsc::Receiver<Command>,
    process: Option<Process>,
    relay: Relay,
    clients: BTreeMap<u32, Client>,
    policy: Supervisor,
    started: Instant,
    retry_at: Option<Instant>,
    init_deadline: Option<Instant>,
    tasks: TaskTracker,
    captures: crate::capture::Captures,
}
impl Actor {
    fn now(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
    fn info(&self) -> std::sync::MutexGuard<'_, ServerInfo> {
        self.status.lock().expect("LSP status")
    }
    fn generation(&self) -> u64 {
        self.info().generation
    }
    fn diagnostic_generation(&self, active: bool) {
        let _ = (self.project.diagnostics)(DiagnosticEvent::Generation {
            generation: self.generation(),
            active,
        });
    }
    async fn start(&mut self) -> Result<(), LspError> {
        let process = Process::spawn(&self.executable, &self.spec, &self.project.root)?;
        let generation = self.generations.fetch_add(1, Ordering::SeqCst) + 1;
        let previous = self.generation();
        self.policy.started();
        self.relay = Relay::new(
            crate::uri::file_uri(&self.project.root).ok_or(LspError::OutsideProject)?,
            generation,
        );
        {
            let mut info = self.info();
            info.generation = generation;
            info.pid = Some(process.pid);
            info.state = ServerState::Starting;
        }
        self.process = Some(process);
        self.retry_at = None;
        self.init_deadline = None;
        self.diagnostic_generation(true);
        if self.clients.is_empty() {
            self.policy.ensure_idle(self.now());
        }
        if previous > 0 {
            (self.project.restarted)(Restarted {
                project_id: self.project.project_id.clone(),
                language: self.spec.language,
                failed: false,
                generation,
            });
        }
        Ok(())
    }
    async fn stop_process(&mut self) {
        self.diagnostic_generation(false);
        if let Some(process) = self.process.take() {
            process.stop().await;
        }
        self.info().pid = None;
    }
    fn close_clients(&mut self) {
        self.captures.cancel_all();
        let clients = std::mem::take(&mut self.clients);
        for (_, client) in clients {
            client.close(&self.tasks);
            self.policy.left(self.now());
        }
        self.info().clients = 0;
        self.init_deadline = None;
    }
    async fn failed(&mut self) {
        self.close_clients();
        self.stop_process().await;
        if let Some(delay) = self.policy.crashed(self.now()) {
            self.retry_at = Some(Instant::now() + Duration::from_millis(delay));
            self.info().state = ServerState::Starting;
        } else {
            self.retry_at = None;
            self.info().state = ServerState::Failed;
            (self.project.restarted)(Restarted {
                project_id: self.project.project_id.clone(),
                language: self.spec.language,
                failed: true,
                generation: self.generation(),
            });
        }
    }
    fn detach(&mut self, id: u32) -> Result<(), LspError> {
        self.captures.cancel(id, None);
        if let Some(client) = self.clients.remove(&id) {
            client.close(&self.tasks);
            self.policy.left(self.now());
        }
        self.info().clients = self.clients.len() as u32;
        let output = self.relay.detach(id);
        if let Some(process) = &self.process {
            for json in output.server {
                process.send(&json)?;
            }
        }
        Ok(())
    }
    fn deliver(&mut self, output: RelayOutput) -> Result<(), LspError> {
        if let Some(process) = &self.process {
            for json in output.server {
                process.send(&json)?;
            }
        }
        for diagnostics in output.diagnostics {
            (self.project.diagnostics)(DiagnosticEvent::Publish {
                generation: self.generation(),
                diagnostics,
            })?;
        }
        let mut close = output.close;
        for (id, json) in output.clients {
            if let Some(client) = self.clients.get(&id)
                && client.send(json).is_err()
            {
                close.push(id);
            }
        }
        for id in close {
            self.detach(id)?;
        }
        if self.relay.ready() {
            self.init_deadline = None;
            self.info().state = ServerState::Ready;
        }
        Ok(())
    }
    async fn command(&mut self, command: Command) -> Result<(), LspError> {
        match command {
            Command::Attach { ends, reply } => {
                if self.policy.failed || self.retry_at.is_some() {
                    let _ = reply.send(Err(LspError::Failed));
                    let _ =
                        tokio::time::timeout(Duration::from_millis(250), ends.tx.finish()).await;
                    return Ok(());
                }
                if self.process.is_none() {
                    self.start().await?;
                }
                if let Err(error) = self.relay.attach(ends.id) {
                    let _ = reply.send(Err(error));
                    let _ =
                        tokio::time::timeout(Duration::from_millis(250), ends.tx.finish()).await;
                    return Ok(());
                }
                let id = ends.id;
                self.clients
                    .insert(id, Client::start(ends, self.commands.clone(), &self.tasks));
                self.policy.joined(self.now());
                self.info().clients = self.clients.len() as u32;
                if !self.relay.ready() && self.init_deadline.is_none() {
                    self.init_deadline = Some(Instant::now() + Duration::from_secs(10));
                }
                if reply.send(Ok(())).is_err() {
                    self.detach(id)?;
                }
            }
            Command::Captured {
                client,
                job,
                result,
            } => {
                let generation = self.generation();
                match self
                    .captures
                    .complete(client, job, generation, result, &mut self.relay)
                {
                    Ok(Some(message)) => {
                        let request = serde_json::from_str::<serde_json::Value>(&message.json)
                            .ok()
                            .and_then(|v| v.get("id").cloned())
                            .unwrap_or_default();
                        match self
                            .relay
                            .client(client, message, &self.project.access)
                            .await
                        {
                            Ok(output) => self.deliver(output)?,
                            Err(_) => {
                                self.relay.proofs.cancel(client, &request);
                                self.deliver(RelayOutput {
                                    clients: vec![(client, crate::workspace::refused(request))],
                                    ..Default::default()
                                })?;
                            }
                        }
                    }
                    Err(request) => self.deliver(RelayOutput {
                        clients: vec![(client, crate::workspace::refused(request))],
                        ..Default::default()
                    })?,
                    Ok(None) => {}
                }
            }
            Command::Detach { id } => self.detach(id)?,
            Command::Message { id, message } => {
                if !self.clients.contains_key(&id) {
                    return Ok(());
                }
                if let Some((method, request)) = crate::capture::method(&message) {
                    if method == "$/cancelRequest" {
                        self.captures.cancel(id, request.as_ref());
                    }
                    if method == "textDocument/rename" {
                        let generation = self.generation();
                        if let Err((request, _)) = self.captures.start(
                            id,
                            generation,
                            message,
                            &mut self.relay,
                            crate::capture::CaptureTask {
                                access: self.project.capture.clone(),
                                commands: self.commands.clone(),
                                tasks: &self.tasks,
                                stop: &self.stop,
                            },
                        ) {
                            self.deliver(RelayOutput {
                                clients: vec![(id, crate::workspace::refused(request))],
                                ..Default::default()
                            })?;
                        }
                        return Ok(());
                    }
                }
                match self.relay.client(id, message, &self.project.access).await {
                    Ok(output) => self.deliver(output)?,
                    Err(_) => self.detach(id)?,
                }
            }
            Command::Restart { reply } => {
                self.close_clients();
                self.stop_process().await;
                self.policy.manual();
                self.retry_at = None;
                let result = self.start().await;
                let failed = result.is_err();
                let _ = reply.send(result);
                if failed {
                    return Err(LspError::Failed);
                }
            }
        }
        Ok(())
    }
    async fn tick(&mut self) -> Result<(), LspError> {
        self.relay.proofs.expire();
        let closed: Vec<_> = self
            .clients
            .iter()
            .filter(|(_, c)| c.tx.closed().is_cancelled())
            .map(|(id, _)| *id)
            .collect();
        for id in closed {
            self.detach(id)?;
        }
        if self.retry_at.is_some_and(|at| Instant::now() >= at) {
            self.start().await?;
        }
        if self.process.is_some() && self.policy.idle_due(self.now()) {
            self.stop_process().await;
            self.info().state = ServerState::IdleStopped;
            self.init_deadline = None;
        }
        if self.init_deadline.is_some_and(|at| Instant::now() >= at) {
            return Err(LspError::Failed);
        }
        Ok(())
    }
    async fn run(&mut self) {
        if self.start().await.is_err() {
            self.failed().await;
        }
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            let result = tokio::select! {
             biased;
             _=self.stop.cancelled()=>break,
             command=self.receiver.recv()=>{let Some(command)=command else{break;};self.command(command).await},
             event=async{match &mut self.process{Some(process)=>process.event().await,None=>std::future::pending().await}}=>{
              match event{Event::Failed=>Err(LspError::Failed),Event::Message(json)=>match self.relay.server(&json,&self.project.access).await{Ok(output)=>self.deliver(output),Err(error)=>Err(error)}}
             },
             _=tick.tick()=>self.tick().await,
            };
            if result.is_err() {
                self.failed().await;
            }
        }
        self.close_clients();
        self.stop_process().await;
        self.tasks.close();
        self.tasks.wait().await;
    }
}
