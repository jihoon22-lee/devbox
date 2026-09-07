import { useEffect, useRef, useState } from "react";
import { componentInvoke } from "../transport";
import type { DraftOwner } from "../knowledge/api";
import type { OutputSource } from "../transforms/tools/outputPolicy";

export function MockDraftAction({ value, owner, source, status = 200, mediaType = "text", disabled = false,
  requestTarget = "/", requestMethod = null, onSent, label = "Mock 초안 만들기" }: {
  value: string; owner: DraftOwner; source?: OutputSource; status?: number; mediaType?: "json" | "text"; disabled?: boolean;
  requestTarget?: string; requestMethod?: string | null; onSent?: () => void; label?: string;
}) {
  const [busy, setBusy] = useState(false); const running = useRef(false);
  const [notice, setNotice] = useState(""); const mounted = useRef(false); const revision = useRef(0);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; revision.current += 1; }; }, []);
  useEffect(() => { revision.current += 1; setNotice(""); }, [value, status, mediaType, requestTarget, requestMethod]);
  async function send() {
    if (running.current || disabled) return;
    const version = ++revision.current; running.current = true; setBusy(true); setNotice("");
    try {
      await componentInvoke(owner)("send_mock_draft", { output: value, status, mediaType, requestTarget, requestMethod, ...(source ? { source } : {}) });
      if (mounted.current && revision.current === version) {
        setNotice("Mock 규칙 미리보기로 전달했습니다. 규칙 저장과 서버 시작은 별도로 확인하세요.");
        onSent?.();
      }
    } catch (error) {
      if (mounted.current && revision.current === version) setNotice(error instanceof Error && error.message === "mock_draft_busy"
        ? "Webhooks의 기존 규칙 미리보기를 먼저 적용하거나 취소하세요."
        : "Mock 초안을 전달하지 못했습니다. 데스크톱 앱에서 결과와 크기를 확인하세요.");
    } finally { running.current = false; if (mounted.current) setBusy(false); }
  }
  return <span className="mock-draft-action"><button type="button" className="copy-btn" disabled={disabled || busy} onClick={() => void send()}>{busy ? "전달 중…" : label}</button>
    {notice && <span role="status">{notice}</span>}</span>;
}
