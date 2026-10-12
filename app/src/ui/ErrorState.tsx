import { type ReactNode, useState } from "react";
import { Button } from "./Button";

export function ErrorState({
  title,
  action,
  diagnosticId,
}: {
  title: string;
  action?: ReactNode;
  diagnosticId?: string;
}) {
  const [copyMessage, setCopyMessage] = useState("");
  async function copyDiagnostic() {
    try {
      await navigator.clipboard.writeText(`devbox 진단 ID: ${diagnosticId}`);
      setCopyMessage("진단 ID를 복사했습니다");
    } catch {
      setCopyMessage("복사하지 못했습니다. 진단 ID를 직접 복사하세요");
    }
  }
  return (
    <div
      role="alert"
      style={{ padding: "var(--space-6)", display: "flex", flexDirection: "column", gap: "var(--space-3)" }}
    >
      <p style={{ margin: 0, fontSize: "var(--text-md)" }}>{title}</p>
      <div style={{ display: "flex", gap: "var(--space-2)" }}>
        {action}
        {diagnosticId && <Button onClick={() => void copyDiagnostic()}>진단 복사</Button>}
      </div>
      {diagnosticId && <code>{diagnosticId}</code>}
      {copyMessage && <p role="status">{copyMessage}</p>}
    </div>
  );
}
