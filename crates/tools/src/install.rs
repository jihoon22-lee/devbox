use crate::ToolsError;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize, Serialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct CheckRef {
    pub id: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum WindowsPackage {
    DockerDesktop,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ts_rs::TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum InstallPlan {
    Wsl {
        command: String,
        title: String,
        shell_command: String,
    },
    Windows {
        package: WindowsPackage,
    },
    Manual {
        text: String,
    },
    None,
}
pub(crate) fn shell_command(command: &str) -> String {
    let script = format!("{command}; exec bash -l");
    format!("bash -lc '{}'", script.replace('\'', "'\\''"))
}
/// Static suggestions only. This module never spawns a process or writes a file.
pub fn install_command(check: CheckRef) -> Result<InstallPlan, devbox_protocol::Fail<ToolsError>> {
    if check.id.len() > 128 || check.id.chars().any(char::is_control) {
        return Err(ToolsError::InvalidInput.into());
    }
    let (title, command) = match check.id.as_str() {
        "tool.tmux" => ("tmux 설치", "sudo apt install tmux"),
        "tool.git" => ("Git 설치", "sudo apt install git"),
        "tool.gh" => ("GitHub CLI 설치", "sudo apt install gh"),
        "tool.pnpm" => ("pnpm 9 설치", "npm i -g pnpm@9"),
        "tool.claude" => ("Claude Code 설치", "npm i -g @anthropic-ai/claude-code"),
        "tool.codex" => ("Codex CLI 설치", "npm i -g @openai/codex"),
        "tool.node" => return Ok(InstallPlan::Manual {
            text:
                "프로젝트에 맞는 Node.js 버전을 공식 설치 안내(https://nodejs.org/)에서 확인하세요"
                    .into(),
        }),
        "tool.rust" => {
            return Ok(InstallPlan::Manual {
                text: "rustup 공식 설치 안내(https://rustup.rs/)를 확인하세요".into(),
            });
        }
        "tool.docker" => {
            return Ok(InstallPlan::Windows {
                package: WindowsPackage::DockerDesktop,
            });
        }
        _ => return Ok(InstallPlan::None),
    };
    Ok(InstallPlan::Wsl {
        command: command.into(),
        title: title.into(),
        shell_command: shell_command(command),
    })
}
