import { invoke } from "@tauri-apps/api/core";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { redactKnownTokenPatterns } from "../features/tools/api/lib/secretPatterns";
import { Button } from "../ui";
import type { GateStatus } from "./ConnectionGate";

/** `daemon_log_tail` clamps larger requests to this bound and keeps the newest 64 KiB. */
const LINES = 200;
type Log = { state: "loading" } | { state: "read"; text: string } | { state: "failed"; reason: string };
const reasonOf = (error: unknown) =>
  error && typeof error === "object" && "message" in error && typeof error.message === "string" ? error.message : "";
const pre = { margin: 0, whiteSpace: "pre-wrap", overflowWrap: "anywhere", fontSize: "var(--text-xs)" } as const;

const STEP: Partial<Record<GateStatus["state"], string>> = {
  setupFailed: "WSL 쪽 설치",
  settingsInvalid: "앱 설정 읽기",
};

/** The failed step and the newest WSL-side log lines. The log is read once, when opened. */
export function GateDetails({ failure, onClose }: { failure: GateStatus; onClose: () => void }) {
  // The WSL log cannot explain the app's own settings file, and finding its distro needs that file.
  const withLog = failure.state !== "settingsInvalid";
  const [log, setLog] = useState<Log>({ state: "loading" });
  const [notice, setNotice] = useState("");
  const box = useRef<HTMLPreElement>(null);
  useEffect(() => {
    if (!withLog) return;
    let current = true;
    invoke<string>("daemon_log_tail", { lines: LINES }).then(
      (text) => {
        // Shown and copied text are the same redacted text.
        if (current) setLog({ state: "read", text: redactKnownTokenPatterns(text) });
      },
      (error: unknown) => {
        // A failed run's message can carry journal lines too.
        if (current) setLog({ state: "failed", reason: redactKnownTokenPatterns(reasonOf(error)) });
      },
    );
    return () => {
      current = false;
    };
  }, [withLog]);
  // The cause is usually in the newest lines, so the box opens at its end.
  useLayoutEffect(() => {
    if (log.state === "read" && box.current) box.current.scrollTop = box.current.scrollHeight;
  }, [log.state]);
  const step = STEP[failure.state] ?? "WSL 쪽 devbox 연결";
  const text =
    log.state === "loading"
      ? "로그를 읽는 중입니다…"
      : log.state === "read"
        ? log.text.trimEnd() || "기록된 로그가 없습니다."
        : `로그를 읽지 못했습니다. WSL 배포판이 실행 중인지 확인해 주세요.${log.reason ? `\n${log.reason}` : ""}`;
  const copy = async () => {
    try {
      const recent = withLog ? `\n최근 로그\n${text}\n` : "";
      await navigator.clipboard.writeText(`실패한 단계: ${step}\n${failure.detail ?? ""}\n${recent}`);
      setNotice("자세한 내용을 복사했습니다");
    } catch {
      setNotice("자세한 내용을 복사하지 못했습니다");
    }
  };
  return (
    <section
      aria-label="연결 실패 자세히"
      style={{
        display: "grid",
        gap: "var(--space-2)",
        maxHeight: "50vh",
        overflow: "auto",
        padding: "var(--space-3) var(--space-4)",
        borderTop: "1px solid var(--border)",
        borderBottom: "1px solid var(--border)",
        background: "var(--bg-0)",
      }}
    >
      <p style={{ margin: 0 }}>
        <strong>실패한 단계</strong> {step}
      </p>
      {failure.detail && <pre style={pre}>{failure.detail}</pre>}
      {withLog && (
        <>
          <strong>최근 로그</strong>
          <pre ref={box} style={{ ...pre, maxHeight: 240, overflow: "auto", color: "var(--fg-muted)" }}>
            {text}
          </pre>
        </>
      )}
      <span style={{ display: "flex", gap: 8 }}>
        <Button disabled={withLog && log.state === "loading"} onClick={() => void copy()}>
          복사
        </Button>
        <Button onClick={onClose}>닫기</Button>
      </span>
      {notice && <p role="status">{notice}</p>}
    </section>
  );
}
