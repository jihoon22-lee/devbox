import { useState } from "react";
import type { LspConfig, LspCustomServer } from "../types";

const LANGUAGES = [
  ["rust", "Rust"], ["typescript", "TypeScript"], ["javascript", "JavaScript"],
  ["python", "Python"], ["json", "JSON"], ["html", "HTML"], ["css", "CSS"],
] as const;

interface Props {
  config: LspConfig;
  disabled: boolean;
  onApply: (config: LspConfig) => void;
  onDirty: (dirty: boolean) => void;
}

function withoutLanguage(config: LspConfig, language: string): LspConfig {
  const server_by_language = { ...config.server_by_language };
  delete server_by_language[language];
  const custom_servers = config.custom_servers.map(server => ({
    ...server, language_ids: server.language_ids.filter(id => id !== language),
  })).filter(server => server.language_ids.length > 0);
  return { ...config, server_by_language, custom_servers };
}

export default function WslLspServerSettings({ config, disabled, onApply, onDirty }: Props) {
  const [language, setLanguage] = useState("rust");
  const [dirty, setDirty] = useState(false);
  return <>
    <p className="lsp-trust-note">
      선택한 WSL 배포판에 설치된 서버를 실행합니다. Linux 절대 경로를 입력하고 설정 저장 후 실행 내용을 검토하세요.
      Node 서버는 실제 진입 파일과 Node 실행 파일을 따로 지정합니다.
    </p>
    {Object.values(config.server_by_language).some(server => server.kind === "managed") && <p className="lsp-warning" role="status">
      Windows 관리형 서버 설정이 남아 있습니다. 해당 언어를 Linux 서버로 바꾸거나 설정을 제거한 뒤 실행을 검토하세요.
    </p>}
    <label>
      언어
      <select disabled={disabled || dirty} value={language} onChange={event => setLanguage(event.currentTarget.value)}>
        {LANGUAGES.map(([id, label]) => <option key={id} value={id}>{label}</option>)}
      </select>
    </label>
    <ServerForm key={language} config={config} language={language} disabled={disabled}
      onDirty={value => { setDirty(value); onDirty(value); }}
      onApply={onApply} />
    <p className="lsp-warning" role="status">
      WSL에서는 설치된 서버만 사용하며 자동 설치는 제공하지 않습니다. 이름 변경과 여러 파일에 걸친 편집 적용은 아직 지원하지 않습니다.
    </p>
  </>;
}

function ServerForm({ config, language, disabled, onApply, onDirty }: Props & { language: string }) {
  const custom = config.custom_servers.find(server => server.language_ids.includes(language));
  const direct = config.server_by_language[language];
  const [kind, setKind] = useState<"native" | "node">(custom?.runtime.kind ?? "native");
  const [executable, setExecutable] = useState(custom?.executable
    ?? (direct?.kind === "local" ? direct.executable || direct.installed_path
      : direct?.kind === "custom" ? direct.executable : ""));
  const [runtime, setRuntime] = useState(custom?.runtime.kind === "node" ? custom.runtime.executable : "");
  const [args, setArgs] = useState((custom?.args ?? (direct?.kind !== "managed" ? direct?.args : []) ?? []).join("\n"));
  const [error, setError] = useState<string | null>(null);
  const apply = () => {
    const entry = executable.trim(), node = runtime.trim();
    if (!entry.startsWith("/") || (kind === "node" && !node.startsWith("/"))) {
      setError("선택한 배포판 안의 Linux 절대 경로를 입력하세요.");
      return;
    }
    const next = withoutLanguage(config, language);
    const argv = args.split(/\r?\n/u).map(part => part.trim()).filter(Boolean);
    if (kind === "node") {
      const server: LspCustomServer = {
        source: "user-provided", license: "unknown", version: "unknown",
        ...custom, language_ids: [language], executable: entry, args: argv,
        runtime: { kind: "node", executable: node,
          min_version: custom?.runtime.kind === "node" ? custom.runtime.min_version : null },
      };
      next.custom_servers.push(server);
    } else {
      next.server_by_language[language] = { kind: "local", installed_path: entry, executable: null, args: argv };
    }
    setError(null); onApply(next); onDirty(false);
  };
  return <>
    {error && <p className="lsp-error" role="alert">{error}</p>}
    <div className="lsp-config-grid">
      <label>
        서버 종류
        <select disabled={disabled} value={kind} onChange={event => {
          setKind(event.currentTarget.value as "native" | "node"); onDirty(true);
        }}>
          <option value="native">Linux 실행 파일 (ELF)</option>
          <option value="node">Node 언어 서버</option>
        </select>
      </label>
      <label className="lsp-wide-field">
        {kind === "node" ? "서버 진입 파일 절대 경로" : "실행 파일 절대 경로"}
        <input disabled={disabled} value={executable} onChange={event => {
          setExecutable(event.currentTarget.value); onDirty(true);
        }} placeholder={kind === "node" ? "/opt/lsp/node_modules/typescript-language-server/lib/cli.mjs" : "/opt/lsp/rust-analyzer"} />
      </label>
      {kind === "node" && <label className="lsp-wide-field">
        Node 실행 파일 절대 경로
        <input disabled={disabled} value={runtime} onChange={event => {
          setRuntime(event.currentTarget.value); onDirty(true);
        }} placeholder="/usr/bin/node" />
      </label>}
      <label className="lsp-wide-field">
        인자 (한 줄에 하나, 셸 문법 사용 안 함)
        <textarea disabled={disabled} value={args} onChange={event => {
          setArgs(event.currentTarget.value); onDirty(true);
        }} placeholder="--stdio" rows={3} />
      </label>
    </div>
    <div className="lsp-config-actions">
      <button type="button" className="toolbar-button" disabled={disabled} onClick={() => {
        setExecutable(""); setRuntime(""); setArgs(""); setError(null);
        onApply(withoutLanguage(config, language)); onDirty(false);
      }}>이 언어 설정 제거</button>
      <button type="button" className="toolbar-button selected" disabled={disabled} onClick={apply}>이 언어 설정 적용</button>
    </div>
  </>;
}
