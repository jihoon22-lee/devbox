#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "devbox", version)]
struct Cli {
    #[arg(long, global = true)]
    instance: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 백그라운드 데몬을 실행한다(보통 systemd가 실행).
    Daemon {
        #[arg(long)]
        foreground: bool,
    },
    /// stdin/stdout을 데몬 소켓에 잇는다(Windows 앱이 wsl.exe로 실행).
    Bridge,
    /// 코딩 에이전트에 프로젝트·작업 도구를 제공하는 MCP stdio 서버.
    Mcp,
    /// 개발·E2E용 WebSocket gateway.
    DevGateway {
        #[arg(long, default_value_t = 1451)]
        port: u16,
        #[arg(long, default_value = "http://localhost:1450")]
        origin: String,
        #[arg(long)]
        token: Option<String>,
    },
    /// systemd unit과 사용자 환경 파일을 설치한다.
    Setup {
        #[arg(long)]
        json: bool,
    },
    /// 이 앱 채널의 서비스·실행 파일만 정리하고 데이터·설정은 보존한다.
    Uninstall {
        #[arg(long)]
        json: bool,
    },
    /// 환경을 점검한다.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// 에이전트 도구의 상태를 알린다.
    Agent {
        #[command(subcommand)]
        cmd: AgentCmd,
    },
    /// 작업 명령의 출력과 종료 결과를 기록한다.
    Exec {
        #[arg(long)]
        run: String,
        #[arg(long)]
        attempt: Option<String>,
        #[arg(long)]
        secrets_stdin: bool,
        #[arg(last = true, required = true)]
        command: Vec<std::ffi::OsString>,
    },
    /// Each secret-bearing systemd attempt acquires fresh secrets from its daemon.
    ExecAttempt {
        #[arg(long)]
        run: String,
        #[arg(last = true, required = true)]
        command: Vec<std::ffi::OsString>,
    },
    /// Internal Windows target worker.
    RunWindows {
        #[arg(long)]
        run: String,
    },
    /// Export a recovery key, or import one from standard input.
    Secrets {
        #[command(subcommand)]
        cmd: SecretsCmd,
    },
    /// systemd ExecStopPost에서 종료 결과를 보충한다.
    RunExited {
        #[arg(long)]
        run: String,
        #[arg(long)]
        attempt: Option<String>,
    },
    /// 버전만 출력한다.
    Version,
}

#[derive(Subcommand)]
enum SecretsCmd {
    ExportKey,
    ImportKey,
}

#[derive(Subcommand)]
enum AgentCmd {
    Event {
        #[arg(long, value_parser = ["claude", "codex", "setup", "exit"])]
        from: String,
        #[arg(long, conflicts_with_all = ["ok", "failed"])]
        code: Option<i32>,
        #[arg(long, conflicts_with = "failed")]
        ok: bool,
        #[arg(long)]
        failed: bool,
        payload: Option<String>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if let Some(instance) = cli.instance {
        devbox_cli::set_instance(&instance)?;
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    if let Cmd::Exec {
        run,
        attempt,
        secrets_stdin,
        command,
    } = cli.cmd
    {
        let status = rt.block_on(devbox_cli::exec::run(run, attempt, secrets_stdin, command))?;
        drop(rt);
        devbox_cli::exec::exit_like_child(status);
    }
    if let Cmd::ExecAttempt { run, command } = cli.cmd {
        let status = rt.block_on(devbox_cli::exec_attempt::run(run, command))?;
        drop(rt);
        devbox_cli::exec::exit_like_child(status);
    }
    if let Cmd::RunWindows { run } = cli.cmd {
        let status = rt.block_on(devbox_cli::windows_exec::run(run))?;
        drop(rt);
        devbox_cli::exec::exit_like_child(status);
    }
    rt.block_on(async move {
        match cli.cmd {
            Cmd::Daemon { foreground } => devbox_cli::daemon::run(foreground).await,
            Cmd::Bridge => devbox_cli::bridge::run().await,
            Cmd::Mcp => devbox_cli::mcp::run().await,
            Cmd::DevGateway {
                port,
                origin,
                token,
            } => devbox_cli::gateway::run(port, origin, token).await,
            Cmd::Setup { json } => devbox_cli::setup::run(json).await,
            Cmd::Uninstall { json } => devbox_cli::setup::uninstall::run(json).await,
            Cmd::Doctor { json } => devbox_cli::doctor::run(json).await,
            Cmd::Agent {
                cmd:
                    AgentCmd::Event {
                        from,
                        code,
                        ok,
                        failed,
                        payload,
                    },
            } => {
                use devbox_agents::hooks::HookSource;
                let source = match from.as_str() {
                    "claude" => HookSource::Claude,
                    "codex" => HookSource::Codex,
                    "setup" => HookSource::Setup,
                    _ => HookSource::Exit,
                };
                let code = code.or(if ok {
                    Some(0)
                } else if failed {
                    Some(1)
                } else {
                    None
                });
                devbox_cli::agent_event::run(source, code, payload).await;
                Ok(())
            }
            Cmd::Secrets { cmd } => {
                devbox_cli::secrets::recovery(matches!(cmd, SecretsCmd::ImportKey)).await
            }
            Cmd::RunWindows { .. } => {
                unreachable!("Windows worker handled before runtime teardown")
            }
            Cmd::ExecAttempt { .. } => unreachable!("attempt handled before runtime teardown"),
            Cmd::Exec { .. } => unreachable!("exec handled before runtime teardown"),
            Cmd::RunExited { run, attempt } => devbox_cli::run_exited::run(run, attempt).await,
            Cmd::Version => {
                println!("{}", env!("CARGO_PKG_VERSION"));
                Ok(())
            }
        }
    })
}
