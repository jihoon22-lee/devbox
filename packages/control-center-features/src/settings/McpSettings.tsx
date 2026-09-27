import { useEffect, useRef, useState } from "react";
import { useOperation } from "@devbox/hooks";
import { toolsCall } from "../calls";
import type { AgentMcpSettings } from "../generated/AgentMcpSettings";
import type { AgentMcpStatus } from "../generated/AgentMcpStatus";
import { toolsMessages } from "../issues";
import "./McpSettings.css";
function windowsPath(path: string): string {
  return path.startsWith("\\\\?\\") ? path.slice(4) : path;
}
export function defaultWslPath(path: string): string | null {
  const match = /^([A-Za-z]):[\\/](.*)$/.exec(windowsPath(path));
  if (!match || /[\u0000-\u001f\u007f]/.test(path)) return null;
  const tail = match[2].replace(/\\/g, "/");
  if (tail.split("/").some((part) => part === "." || part === "..")) return null;
  return `/mnt/${match[1].toLowerCase()}/${tail}`;
}
function shellQuote(value: string): string {
  return `'${value.replace(/'/g, "'\\''")}'`;
}
export default function McpSettings() {
  const [status, setStatus] = useState<AgentMcpStatus | null>(null);
  const [loadIssue, setLoadIssue] = useState<string | null>(null);
  const [notice, setNotice] = useState("");
  const reload = useRef<(() => void) | null>(null);
  const serial = useRef(0);
  const saving = useRef(false);
  const operation = useOperation();
  useEffect(() => {
    let active = true;
    const load = () => {
      if (saving.current) return;
      const revision = ++serial.current;
      void toolsCall("mcp_settings", {}).then(
        (value) => {
          if (active && revision === serial.current) {
            setStatus(value);
            setLoadIssue(null);
          }
        },
        () => {
          if (active && revision === serial.current) setLoadIssue(toolsMessages.mcp_settings_unavailable);
        },
      );
    };
    reload.current = load;
    load();
    window.addEventListener("focus", load);
    return () => {
      active = false;
      reload.current = null;
      window.removeEventListener("focus", load);
    };
  }, []);
  async function change(patch: Partial<AgentMcpSettings>) {
    if (!status?.launcherPath || saving.current) return;
    saving.current = true;
    ++serial.current;
    setNotice("");
    setLoadIssue(null);
    try {
      const next = await operation.run(() =>
        toolsCall("set_mcp_settings", { settings: { ...status.settings, ...patch } }),
      );
      if (next) setStatus(next);
    } finally {
      saving.current = false;
    }
  }
  async function copy(text: string) {
    setNotice("");
    const success = await operation.run(async () => {
      try {
        await navigator.clipboard.writeText(text);
        return true;
      } catch {
        throw new Error("클립보드에 복사하지 못했습니다.");
      }
    });
    if (success) setNotice("복사했습니다.");
  }
  const error = operation.issue ?? loadIssue;
  const path = status?.launcherPath ? windowsPath(status.launcherPath) : null;
  const wsl = path ? defaultWslPath(path) : null;
  const claude = path ? `claude mcp add --scope user devbox -- "$(wslpath -u ${shellQuote(path)})" --mcp-stdio` : "";
  const codex = wsl ? `[mcp_servers.devbox]\ncommand = ${JSON.stringify(wsl)}\nargs = ["--mcp-stdio"]` : "";
  return (
    <section className="panel mcp-settings" aria-label="MCP 서버">
      <h2>MCP 서버</h2>
      {!status && !error && <p role="status">MCP 설정을 확인하는 중입니다.</p>}
      {status && !path && <p>설치형에서만 사용할 수 있습니다.</p>}
      {status && path && (
        <>
          <label>
            <input
              type="checkbox"
              checked={status.settings.enabled}
              disabled={operation.busy}
              onChange={(event) => {
                void change({ enabled: event.currentTarget.checked });
              }}
            />
            MCP 서버 사용
          </label>
          <label>
            <input
              type="checkbox"
              checked={status.settings.allowNoteCapture}
              disabled={operation.busy || !status.settings.enabled}
              onChange={(event) => {
                void change({ allowNoteCapture: event.currentTarget.checked });
              }}
            />
            노트 기록 허용
          </label>
          <p>현재 노트 폴더의 Inbox에 새 노트를 만듭니다.</p>
          <label>
            <input
              type="checkbox"
              checked={status.settings.allowTaskRun}
              disabled={operation.busy || !status.settings.enabled}
              onChange={(event) => {
                void change({ allowTaskRun: event.currentTarget.checked });
              }}
            />
            신뢰한 작업 실행 허용
          </label>
          <p>Workspace에서 신뢰한 작업을 실행할 수 있습니다.</p>
          {status.settings.enabled && (
            <>
              <p>
                Windows 실행 경로: <code>{path}</code>
              </p>
              <h3>Claude Code (WSL)</h3>
              <pre>{claude}</pre>
              <button
                disabled={operation.busy}
                aria-label="Claude Code 등록 명령 복사"
                onClick={() => {
                  void copy(claude);
                }}
              >
                복사
              </button>
              <h3>Codex (WSL)</h3>
              <p>~/.codex/config.toml에 추가합니다.</p>
              {codex ? (
                <>
                  <pre>{codex}</pre>
                  <button
                    disabled={operation.busy}
                    aria-label="Codex 설정 복사"
                    onClick={() => {
                      void copy(codex);
                    }}
                  >
                    복사
                  </button>
                </>
              ) : (
                <p>wslpath -u로 실행 경로를 확인해 주세요.</p>
              )}
              <p>WSL 자동 마운트 위치를 바꿨다면 wslpath -u로 변환한 경로를 쓰세요.</p>
            </>
          )}
        </>
      )}
      {notice && <p role="status">{notice}</p>}
      {error && (
        <>
          <p role="alert">{error}</p>
          <button
            disabled={operation.busy}
            onClick={() => {
              operation.clearIssue();
              setLoadIssue(null);
              reload.current?.();
            }}
          >
            다시 확인
          </button>
        </>
      )}
    </section>
  );
}
