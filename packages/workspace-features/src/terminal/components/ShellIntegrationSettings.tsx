import { useCallback, useEffect, useRef, useState } from "react";
import { inspectShellIntegration, updateShellIntegration } from "../api";
import type {
  ShellIntegrationInfo,
  ShellIntegrationReport,
  ShellIntegrationStatus,
  ShellKind,
} from "../types";
import type { AskDialog } from "./AppDialog";

interface ShellIntegrationSettingsProps {
  distro: string;
  ask: AskDialog;
  onError: (message: string) => void;
}

const STATUS_LABELS: Readonly<Record<ShellIntegrationStatus, string>> = {
  missing: "미설치",
  current: "사용 중",
  outdated: "업데이트 필요",
  conflict: "marker 충돌",
  blocked: "자동 변경 불가",
};

const SHELL_LABELS: Readonly<Record<ShellKind, string>> = {
  bash: "Bash",
  zsh: "Zsh",
};

export default function ShellIntegrationSettings({
  distro,
  ask,
  onError,
}: ShellIntegrationSettingsProps) {
  const [report, setReport] = useState<ShellIntegrationReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [busyShell, setBusyShell] = useState<ShellKind | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const sequence = useRef(0);
  const mounted = useRef(true);
  const distroRef = useRef(distro);
  distroRef.current = distro;

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      sequence.current += 1;
    };
  }, []);

  const refresh = useCallback(async () => {
    const request = ++sequence.current;
    setNotice(null);
    if (!distro) {
      setReport(null);
      setLoading(false);
      return;
    }
    setReport((current) => current?.distro === distro ? current : null);
    setLoading(true);
    try {
      const next = await inspectShellIntegration(distro);
      if (!mounted.current || request !== sequence.current) return;
      setReport(next);
    } catch {
      if (!mounted.current || request !== sequence.current) return;
      setReport(null);
      onError("WSL 셸 연동 상태를 확인하지 못했습니다.");
    } finally {
      if (mounted.current && request === sequence.current) setLoading(false);
    }
  }, [distro, onError]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const apply = async (integration: ShellIntegrationInfo, action: "install" | "remove") => {
    if (busyShell !== null || integration.status === "conflict" || integration.status === "blocked") return;
    const installing = action === "install";
    const answer = await ask({
      kind: "confirm",
      title: `${SHELL_LABELS[integration.shell]} 셸 연동을 ${installing ? "설치" : "제거"}할까요?`,
      lines: [
        `${integration.rcFile}에서 Devbox marker block만 ${installing ? "추가하거나 복구" : "제거"}합니다.`,
        "기존 파일은 변경 전에 별도 백업하며, 미리보기 이후 파일이 바뀌면 적용하지 않습니다.",
      ],
      detail: integration.blockPreview,
      confirmLabel: installing ? (integration.status === "outdated" ? "복구" : "설치") : "제거",
      danger: true,
    });
    if (!answer.confirmed || !mounted.current || distroRef.current !== distro) return;

    setBusyShell(integration.shell);
    setNotice(null);
    try {
      const result = await updateShellIntegration(
        distro,
        integration.shell,
        action,
        integration.revision,
      );
      if (!mounted.current || distroRef.current !== distro) return;
      setReport((current) => current?.distro === distro ? {
        ...current,
        shells: current.shells.map((item) =>
          item.shell === integration.shell ? result.integration : item
        ),
      } : current);
      const backup = result.backupFile ? ` 백업: ${result.backupFile}` : "";
      setNotice(result.changed
        ? `${SHELL_LABELS[integration.shell]} 연동을 ${installing ? "적용" : "제거"}했습니다.${backup} 새 터미널부터 반영됩니다.`
        : `이미 요청한 ${SHELL_LABELS[integration.shell]} 연동 상태입니다.`);
    } catch {
      if (mounted.current) onError("WSL 셸 연동 파일을 변경하지 못했습니다. 상태를 다시 확인해 주세요.");
    } finally {
      if (mounted.current) setBusyShell(null);
    }
  };

  const copyBlock = async (integration: ShellIntegrationInfo) => {
    try {
      await navigator.clipboard.writeText(integration.blockPreview);
      if (mounted.current) setNotice(`${SHELL_LABELS[integration.shell]} 연동 block을 복사했습니다.`);
    } catch {
      if (mounted.current) onError("셸 연동 block을 클립보드에 복사하지 못했습니다.");
    }
  };

  return (
    <fieldset className="settings-group shell-integration-settings">
      <legend>셸 연동</legend>
      <div className="settings-row">
        <span>
          팬별 현재 경로 추적
          <small>OSC 7을 사용해 각 팬의 cwd를 저장하고 다음 실행에서 복원합니다.</small>
        </span>
        <button
          type="button"
          className="btn compact"
          disabled={loading || busyShell !== null || !distro}
          aria-busy={loading}
          onClick={() => void refresh()}
        >
          {loading ? "확인 중…" : "다시 확인"}
        </button>
      </div>

      {!distro && <div className="banner">먼저 WSL 배포판을 선택하세요.</div>}
      {distro && loading && !report && <div className="dim" role="status">셸 설정을 확인하는 중…</div>}
      {report?.shells.map((integration) => {
        const blocked = integration.status === "conflict" || integration.status === "blocked";
        const installing = integration.status === "missing" || integration.status === "outdated";
        return (
          <div className="shell-integration-row" key={integration.shell}>
            <div className="shell-integration-summary">
              <strong>{SHELL_LABELS[integration.shell]}</strong>
              {integration.defaultShell && <span className="pane-badge">기본 셸</span>}
              <span className={`shell-integration-status ${integration.status}`}>
                {STATUS_LABELS[integration.status]}
              </span>
              <small>{integration.rcFile}</small>
            </div>
            {blocked && (
              <small className="shell-integration-warning">
                marker가 중복·미완성이거나 rc 파일이 symbolic link/일반 파일이 아니어서 자동 변경하지 않습니다.
              </small>
            )}
            <div className="shell-integration-actions">
              <button
                type="button"
                className="btn compact"
                disabled={busyShell !== null}
                onClick={() => void copyBlock(integration)}
              >
                {SHELL_LABELS[integration.shell]} block 복사
              </button>
              {installing && !blocked && (
                <button
                  type="button"
                  className="btn compact primary"
                  disabled={busyShell !== null}
                  aria-busy={busyShell === integration.shell}
                  aria-label={`${SHELL_LABELS[integration.shell]} 연동 ${integration.status === "outdated" ? "복구" : "설치"}`}
                  onClick={() => void apply(integration, "install")}
                >
                  {integration.status === "outdated" ? "복구" : "설치"}
                </button>
              )}
              {integration.status === "current" && (
                <button
                  type="button"
                  className="btn compact danger"
                  disabled={busyShell !== null}
                  aria-busy={busyShell === integration.shell}
                  aria-label={`${SHELL_LABELS[integration.shell]} 연동 제거`}
                  onClick={() => void apply(integration, "remove")}
                >제거</button>
              )}
            </div>
          </div>
        );
      })}
      {notice && <div className="banner shell-integration-notice" role="status">{notice}</div>}
    </fieldset>
  );
}
