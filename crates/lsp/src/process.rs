use crate::{
    LspError,
    framing::{Decoder, encode},
    servers::ServerSpec,
};
use std::{path::Path, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
    sync::{OwnedSemaphorePermit, Semaphore, mpsc},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
pub(crate) enum Event {
    Message(String),
    Failed,
}
struct Group(Option<u32>);
impl Drop for Group {
    fn drop(&mut self) {
        #[cfg(not(unix))]
        let _ = self.0;
        #[cfg(unix)]
        if let Some(pid) = self
            .0
            .and_then(|pid| i32::try_from(pid).ok())
            .and_then(rustix::process::Pid::from_raw)
        {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
    }
}
pub(crate) struct Process {
    child: Child,
    group: Group,
    pub pid: u32,
    input: mpsc::Sender<(Vec<u8>, OwnedSemaphorePermit)>,
    output: mpsc::Receiver<String>,
    budget: Arc<Semaphore>,
    fault: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
}
impl Process {
    pub fn spawn(executable: &Path, spec: &ServerSpec, root: &Path) -> Result<Self, LspError> {
        let mut command = Command::new(executable);
        command
            .args(&spec.args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().map_err(|_| LspError::Failed)?;
        let pid = child.id().ok_or(LspError::Failed)?;
        let group = Group(Some(pid));
        let mut stdin = child.stdin.take().ok_or(LspError::Failed)?;
        let mut stdout = child.stdout.take().ok_or(LspError::Failed)?;
        let fault = CancellationToken::new();
        let (input, mut receiver) = mpsc::channel::<(Vec<u8>, OwnedSemaphorePermit)>(8);
        let (sender, output) = mpsc::channel(8);
        let write_fault = fault.clone();
        let writer = tokio::spawn(async move {
            loop {
                tokio::select! {
                 _=write_fault.cancelled()=>break,
                 packet=receiver.recv()=>{let Some((bytes,_permit))=packet else{break;};
                  let result=tokio::select!{_=write_fault.cancelled()=>break,result=tokio::time::timeout(Duration::from_secs(2),stdin.write_all(&bytes))=>result};
                  if !matches!(result,Ok(Ok(()))){write_fault.cancel();break;}
                 }
                }
            }
        });
        let read_fault = fault.clone();
        let reader = tokio::spawn(async move {
            let mut decoder = Decoder::default();
            let mut buffer = [0; 8192];
            loop {
                let read = tokio::select! {_=read_fault.cancelled()=>break,read=stdout.read(&mut buffer)=>read};
                let Ok(size) = read else {
                    read_fault.cancel();
                    break;
                };
                if size == 0 {
                    let _ = decoder.finish();
                    read_fault.cancel();
                    break;
                }
                let Ok(messages) = decoder.push(&buffer[..size]) else {
                    read_fault.cancel();
                    break;
                };
                for json in messages {
                    if sender.try_send(json).is_err() {
                        read_fault.cancel();
                        return;
                    }
                }
            }
        });
        Ok(Self {
            child,
            group,
            pid,
            input,
            output,
            budget: Arc::new(Semaphore::new(4 * 1024 * 1024)),
            fault,
            tasks: vec![writer, reader],
        })
    }
    pub fn send(&self, json: &str) -> Result<(), LspError> {
        let bytes = encode(json).map_err(|_| LspError::Protocol)?;
        let permit = self
            .budget
            .clone()
            .try_acquire_many_owned(bytes.len() as u32)
            .map_err(|_| LspError::LimitExceeded)?;
        self.input
            .try_send((bytes, permit))
            .map_err(|_| LspError::LimitExceeded)
    }
    pub async fn event(&mut self) -> Event {
        tokio::select! {
         _=self.fault.cancelled()=>Event::Failed,
         _=self.child.wait()=>Event::Failed,
         message=self.output.recv()=>message.map(Event::Message).unwrap_or(Event::Failed),
        }
    }
    pub async fn stop(mut self) {
        self.fault.cancel();
        drop(self.group);
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(2), self.child.wait()).await;
        for task in self.tasks.drain(..) {
            let _ = task.await;
        }
    }
}
