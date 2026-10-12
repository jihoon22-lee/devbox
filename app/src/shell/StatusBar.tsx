import { useConnection } from "../rpc/connection";
import { StatusDot } from "../ui";

const LABEL = {
  connecting: "연결 중",
  ready: "데몬 연결됨",
  reconnecting: "다시 연결 중",
  versionMismatch: "버전 불일치",
} as const;
const TONE = { connecting: "progress", ready: "ok", reconnecting: "warn", versionMismatch: "danger" } as const;

export function StatusBar() {
  const { state, daemonVersion } = useConnection();
  return (
    <footer
      style={{
        display: "flex",
        alignItems: "center",
        gap: "var(--space-3)",
        padding: "0 var(--space-3)",
        fontSize: "var(--text-xs)",
        color: "var(--fg-muted)",
        background: "var(--bg-0)",
        borderTop: "1px solid var(--border)",
      }}
    >
      <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
        <StatusDot tone={TONE[state]} label={LABEL[state]} />
        {LABEL[state]}
      </span>
      <span style={{ flex: 1 }} />
      {daemonVersion && <span>데몬 {daemonVersion}</span>}
    </footer>
  );
}
