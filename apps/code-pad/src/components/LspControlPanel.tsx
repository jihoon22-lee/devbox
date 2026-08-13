import { useEffect, useMemo, useRef, useState } from "react";
import {
  languageServerStatuses,
  loadLspConfig,
  saveLspConfig,
  startLanguageServer,
  stopLanguageServer,
} from "../api";
import type {
  LanguageServerStatus,
  LoadedLspConfig,
  LspConfig,
  LspServerRef,
} from "../types";
import ManagedInstallerPanel from "./ManagedInstallerPanel";

const LANGUAGE_OPTIONS = [
  ["rust", "Rust"],
  ["typescript", "TypeScript"],
  ["javascript", "JavaScript"],
  ["python", "Python"],
  ["json", "JSON"],
  ["html", "HTML"],
  ["css", "CSS"],
] as const;

const CAPABILITY_LABELS: Array<[keyof LanguageServerStatus["capabilities"], string]> = [
  ["diagnostics", "진단"],
  ["completion", "완성"],
  ["hover", "호버"],
  ["definition", "정의"],
  ["references", "참조"],
  ["rename", "이름 변경"],
  ["formatting", "포맷"],
];

function emptyConfig(workspaceRoot: string | null): LspConfig {
  return {
    version: 1,
    enabled: false,
    workspace_root: workspaceRoot ?? "",
    server_by_language: {},
    custom_servers: [],
    update_policy: "manual",
  };
}

function editableCommand(server: LspServerRef | undefined): { executable: string; args: string } {
  if (!server) return { executable: "", args: "" };
  if (server.kind === "local") {
    return {
      executable: server.executable || server.installed_path,
      args: server.args.join("\n"),
    };
  }
  if (server.kind === "custom") {
    return { executable: server.executable, args: server.args.join("\n") };
  }
  return { executable: server.installed_path ?? "", args: "" };
}

function parseArgs(value: string): string[] {
  // Each line is one argv item, never a shell command. This preserves spaces
  // inside an argument without inventing cmd.exe or PowerShell quoting rules.
  return value.split(/\r?\n/u).map((part) => part.trim()).filter(Boolean);
}

function statusLabel(status: LanguageServerStatus["status"]): string {
  switch (status) {
    case "starting": return "시작 중";
    case "ready": return "준비됨";
    case "degraded": return "성능 저하";
    case "crashed": return "비정상 종료";
    default: return "중지됨";
  }
}

interface Props {
  workspaceRoot: string | null;
  onClose: () => void;
  onConfigChanged?: (config: LspConfig) => void;
}

export default function LspControlPanel({ workspaceRoot, onClose, onConfigChanged }: Props) {
  const [loaded, setLoaded] = useState<LoadedLspConfig | null>(null);
  const [config, setConfig] = useState<LspConfig>(() => emptyConfig(workspaceRoot));
  const [statuses, setStatuses] = useState<LanguageServerStatus[]>([]);
  const [selectedLanguage, setSelectedLanguage] = useState("rust");
  const [serverKind, setServerKind] = useState<"local" | "custom">("local");
  const [executable, setExecutable] = useState("");
  const [args, setArgs] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [hasUnsavedChanges, setHasUnsavedChanges] = useState(false);
  const [formDirty, setFormDirty] = useState(false);
  const busyRef = useRef(false);

  const runningLanguages = useMemo(
    () => new Set(statuses.map((status) => status.languageId)),
    [statuses],
  );

  const refreshStatuses = async () => {
    try {
      setStatuses(await languageServerStatuses());
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  };

  useEffect(() => {
    let cancelled = false;
    void Promise.all([loadLspConfig(), languageServerStatuses()])
      .then(([nextLoaded, nextStatuses]) => {
        if (cancelled) return;
        const nextConfig = { ...nextLoaded.config };
        if (workspaceRoot && nextConfig.workspace_root !== workspaceRoot) {
          nextConfig.workspace_root = workspaceRoot;
          setHasUnsavedChanges(true);
        }
        setLoaded(nextLoaded);
        setConfig(nextConfig);
        setStatuses(nextStatuses);
      })
      .catch((cause) => {
        if (!cancelled) setError(cause instanceof Error ? cause.message : String(cause));
      });
    const timer = window.setInterval(() => void refreshStatuses(), 2_000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
    // The dialog loads one persisted snapshot when opened.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const command = editableCommand(config.server_by_language[selectedLanguage]);
    const selected = config.server_by_language[selectedLanguage];
    setServerKind(selected?.kind === "custom" ? "custom" : "local");
    setExecutable(command.executable);
    setArgs(command.args);
    setFormDirty(false);
  }, [config.server_by_language, selectedLanguage]);

  const run = async (operation: () => Promise<void>) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      await operation();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const updateServer = () => {
    const command = executable.trim();
    if (!command) {
      setError("실행 파일의 절대 경로를 입력하세요.");
      return;
    }
    const next: LspServerRef = serverKind === "local"
      ? { kind: "local", installed_path: command, executable: null, args: parseArgs(args) }
      : { kind: "custom", executable: command, args: parseArgs(args) };
    setConfig((current) => ({
      ...current,
      server_by_language: { ...current.server_by_language, [selectedLanguage]: next },
    }));
    setHasUnsavedChanges(true);
    setFormDirty(false);
    setError(null);
  };

  const removeServer = () => {
    setConfig((current) => {
      const serverByLanguage = { ...current.server_by_language };
      delete serverByLanguage[selectedLanguage];
      return { ...current, server_by_language: serverByLanguage };
    });
    setHasUnsavedChanges(true);
    setFormDirty(false);
  };

  const handleSave = () => void run(async () => {
    const next = {
      ...config,
      enabled: config.enabled && Boolean(config.workspace_root),
      workspace_root: workspaceRoot ?? config.workspace_root,
    };
    await saveLspConfig(next, loaded?.persist_allowed === false);
    setConfig(next);
    setLoaded({ config: next, persist_allowed: true, error: null });
    setStatuses([]);
    setHasUnsavedChanges(false);
    onConfigChanged?.(next);
  });

  const handleStart = (languageId: string) => void run(async () => {
    await startLanguageServer(languageId);
    await refreshStatuses();
  });

  const handleStop = (languageId: string) => void run(async () => {
    await stopLanguageServer(languageId);
    await refreshStatuses();
  });

  return (
    <div className="modal-backdrop" role="presentation">
      <section className="lsp-panel" role="dialog" aria-modal="true" aria-label="언어 서버 설정">
        <header className="lsp-panel-header">
          <div>
            <p className="eyebrow">LSP 3.17 · LOCAL STDIO</p>
            <h2>언어 서버</h2>
          </div>
          <button type="button" className="toolbar-button" onClick={onClose}>닫기</button>
        </header>

        {error && <p className="lsp-error" role="alert">{error}</p>}
        {loaded?.error && (
          <p className="lsp-warning" role="alert">
            저장된 설정이 손상되었습니다: {loaded.error} 저장하면 기존 파일을 명시적으로 복구합니다.
          </p>
        )}
        {!workspaceRoot && (
          <p className="lsp-warning">먼저 작업 폴더를 지정해야 LSP를 활성화할 수 있습니다.</p>
        )}

        <label className="lsp-toggle">
          <input
            type="checkbox"
            checked={config.enabled}
            disabled={!workspaceRoot || !loaded}
            onChange={(event) => {
              const enabled = event.currentTarget.checked;
              setConfig((current) => ({ ...current, enabled }));
              setHasUnsavedChanges(true);
            }}
          />
          이 작업 폴더에서 언어 서버 사용
        </label>
        <p className="lsp-trust-note">
          서버는 사용자 권한으로 실행됩니다. Code Pad는 셸을 거치지 않고 고정 argv와 작업 폴더만 전달하지만,
          신뢰하는 로컬 실행 파일만 등록하세요.
        </p>

        <div className="lsp-config-grid">
          <label>
            언어
            <select disabled={!loaded || formDirty} value={selectedLanguage} onChange={(event) => setSelectedLanguage(event.currentTarget.value)}>
              {LANGUAGE_OPTIONS.map(([id, label]) => <option key={id} value={id}>{label}</option>)}
            </select>
          </label>
          <label>
            서버 종류
            <select disabled={!loaded} value={serverKind} onChange={(event) => {
              setServerKind(event.currentTarget.value as "local" | "custom");
              setFormDirty(true);
            }}>
              <option value="local">설치된 로컬 서버</option>
              <option value="custom">사용자 정의 stdio 서버</option>
            </select>
          </label>
          <label className="lsp-wide-field">
            실행 파일 절대 경로
            <input disabled={!loaded} value={executable} onChange={(event) => {
              setExecutable(event.currentTarget.value);
              setFormDirty(true);
            }} placeholder="C:\\Tools\\rust-analyzer.exe" />
          </label>
          <label className="lsp-wide-field">
            인자 (한 줄에 하나, 셸 문법 사용 안 함)
            <textarea disabled={!loaded} value={args} onChange={(event) => {
              setArgs(event.currentTarget.value);
              setFormDirty(true);
            }} placeholder={"--stdio\n--log-level=info"} rows={3} />
          </label>
        </div>
        <div className="lsp-config-actions">
          <button type="button" className="toolbar-button" disabled={!loaded} onClick={removeServer}>이 언어 설정 제거</button>
          <button type="button" className="toolbar-button selected" disabled={!loaded} onClick={updateServer}>이 언어 설정 적용</button>
        </div>

        <section className="lsp-status-section" aria-label="언어 서버 상태">
          <h3>현재 상태</h3>
          {Object.keys(config.server_by_language).length === 0 && (
            <p className="lsp-empty">등록된 언어 서버가 없습니다.</p>
          )}
          {Object.keys(config.server_by_language).sort().map((languageId) => {
            const status = statuses.find((item) => item.languageId === languageId);
            return (
              <article className="lsp-status-card" key={languageId}>
                <div className="lsp-status-main">
                  <strong>{languageId}</strong>
                  <span className={`lsp-state ${status?.status ?? "stopped"}`}>
                    {status ? statusLabel(status.status) : "중지됨"}
                  </span>
                  <span>{status?.serverInfo?.name ?? config.server_by_language[languageId].kind}</span>
                  <span>문서 {status?.documentCount ?? 0}</span>
                </div>
                {status && (
                  <div className="lsp-capabilities" aria-label={`${languageId} 기능`}>
                    {CAPABILITY_LABELS.filter(([key]) => Boolean(status.capabilities[key])).map(([, label]) => (
                      <span key={label}>{label}</span>
                    ))}
                    <span>{status.capabilities.positionEncoding}</span>
                    {status.capabilities.legacyPositionEncoding && <span>레거시 위치</span>}
                  </div>
                )}
                {status?.stderr && (
                  <details className="lsp-stderr">
                    <summary>최근 stderr{status.stderrTruncated ? " (일부 생략)" : ""}</summary>
                    <pre>{status.stderr}</pre>
                  </details>
                )}
                <div className="lsp-status-actions">
                  {runningLanguages.has(languageId)
                    ? <button type="button" className="toolbar-button" disabled={busy} onClick={() => handleStop(languageId)}>중지</button>
                    : <button type="button" className="toolbar-button" disabled={busy || !config.enabled || hasUnsavedChanges} onClick={() => handleStart(languageId)}>시작</button>}
                </div>
              </article>
            );
          })}
        </section>

        <ManagedInstallerPanel onError={setError} />

        <footer className="lsp-panel-footer">
          <span>{formDirty ? "먼저 이 언어 설정을 적용하세요." : hasUnsavedChanges ? "변경 사항을 저장해야 서버를 시작할 수 있습니다." : "설정을 저장하면 실행 중인 서버는 안전하게 종료됩니다."}</span>
          <button type="button" className="toolbar-button selected" disabled={busy || !loaded || formDirty} onClick={handleSave}>
            설정 저장
          </button>
        </footer>
      </section>
    </div>
  );
}
