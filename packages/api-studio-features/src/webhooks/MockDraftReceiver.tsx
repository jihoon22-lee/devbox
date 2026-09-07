import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { restoreFocus, trapDialogKeyDown } from "@devbox/a11y";
import { componentInvoke } from "../transport";
import { isTauri } from "./lib/isTauri";
import { validateRule } from "./lib/ruleValidation";
import type { ResponseRule } from "./api";
import "./mockDraft.css";
const invoke = componentInvoke("api-studio.webhooks");
const ERROR = "Mock 초안을 확인하지 못했습니다. 원래 규칙 초안은 유지됩니다.";
interface Preview { id: string; producer: "api-playground" | "developer-toolbox"; expiresAtMs: number; rule: ResponseRule; redacted: boolean }
function record(value: unknown): value is Record<string, unknown> { return !!value && typeof value === "object" && !Array.isArray(value); }
export function parseMockRule(value: unknown): ResponseRule {
  if (!record(value) || value.id !== "" || value.priority !== 0 || value.delayMs !== 0
    || Object.keys(value).some(key => !["id", "priority", "method", "path", "status", "headers", "body", "delayMs", "sequence"].includes(key))
    || (value.sequence !== undefined && (!Array.isArray(value.sequence) || value.sequence.length !== 0))
    || !Array.isArray(value.headers) || value.headers.length !== 1 || !Array.isArray(value.headers[0])
    || value.headers[0][0] !== "Content-Type" || !["application/json", "text/plain; charset=utf-8"].includes(value.headers[0][1])
    || validateRule(value as unknown as ResponseRule).length) throw new Error(ERROR);
  return value as unknown as ResponseRule;
}
export function parseMockPreview(value: unknown): Preview | null {
  if (value === null) return null;
  if (!record(value) || typeof value.id !== "string" || !/^[a-f0-9]{32}$/u.test(value.id)
    || !["api-playground", "developer-toolbox"].includes(value.producer as string)
    || !Number.isSafeInteger(value.expiresAtMs) || (value.expiresAtMs as number) <= 0
    || typeof value.redacted !== "boolean") throw new Error(ERROR);
  return { id: value.id, producer: value.producer as Preview["producer"], expiresAtMs: value.expiresAtMs as number,
    redacted: value.redacted, rule: parseMockRule(value.rule) };
}
export function MockDraftReceiver({ onApply, disabled, active = true }: { onApply: (rule: ResponseRule) => void; disabled: boolean; active?: boolean }) {
  const [preview, setPreview] = useState<Preview | null>(null);
  const [error, setError] = useState(""); const [busy, setBusy] = useState(false);
  const running = useRef(false); const mounted = useRef(false); const revision = useRef(0);
  const dialog = useRef<HTMLDivElement>(null); const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!isTauri()) return;
    let alive = true; let unlisten: (() => void) | undefined; let checking = false;
    mounted.current = true;
    async function refresh() {
      if (!alive || running.current || checking) return;
      checking = true; const version = revision.current;
      try {
        const result = parseMockPreview(await invoke<unknown>("peek_mock_draft"));
        if (alive && revision.current === version) { setPreview(result); setError(""); }
      } catch (error) {
        if (alive && revision.current === version) {
          if (error instanceof Error && error.message === "mock_draft_expired") setPreview(null);
          setError(ERROR);
        }
      } finally { checking = false; }
    }
    const wake = () => { void refresh(); };
    void listen("api-studio://mock-draft", wake).then(stop => { if (alive) { unlisten = stop; wake(); } else stop(); }).catch(wake);
    const timer = setInterval(wake, 15_000);
    return () => { alive = false; mounted.current = false; revision.current += 1; unlisten?.(); clearInterval(timer); };
  }, []);
  useLayoutEffect(() => {
    if (!preview || !active) return;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    cancel.current?.focus();
    return () => { restoreFocus(previous); };
  }, [preview?.id, active]);
  async function finish(apply: boolean) {
    if (!preview || running.current || (apply && disabled)) return;
    running.current = true; setBusy(true); setError(""); const version = ++revision.current;
    try {
      const result = await invoke<unknown>(apply ? "accept_mock_draft" : "discard_mock_draft", { id: preview.id });
      if (!mounted.current || revision.current !== version) return;
      if (apply) onApply(parseMockRule(result));
      setPreview(null);
    } catch (error) {
      if (mounted.current && revision.current === version) {
        if (error instanceof Error && error.message === "mock_draft_expired") setPreview(null);
        setError(ERROR);
      }
    } finally { running.current = false; if (mounted.current) setBusy(false); }
  }
  if (!active) return null;
  return <>
    {error && !preview && <p role="alert">{error}</p>}
    {preview && <div className="mock-draft-backdrop"><div ref={dialog} className="mock-draft-dialog" role="dialog" aria-modal="true" aria-label="Mock 규칙 초안 미리보기" aria-busy={busy}
      onKeyDown={event => { if (dialog.current) trapDialogKeyDown(event, dialog.current, () => { if (!running.current) void finish(false); }); }}>
      <h2>Mock 규칙 초안 미리보기</h2>
      <p>{preview.producer === "api-playground" ? "Requests" : "Transforms"}에서 전달한 결과입니다. 적용하면 현재 규칙 초안을 바꿉니다. 일치 경로와 메서드를 확인한 뒤 별도로 규칙을 저장하세요.</p>
      <p>서버를 시작하거나 실행 중인 규칙을 자동 변경하지 않습니다.{preview.redacted ? " 민감한 값은 마스킹되었습니다." : ""}</p>
      <dl><div><dt>일치 조건</dt><dd>{preview.rule.method ?? "모든 메서드"} {preview.rule.path}</dd></div><div><dt>응답</dt><dd>{preview.rule.status} · {preview.rule.headers[0][1]}</dd></div></dl>
      <pre aria-label="Mock 응답 본문">{preview.rule.body || "(빈 본문)"}</pre>
      {error && <p role="alert">{error}</p>}
      <button ref={cancel} className="btn" disabled={busy} onClick={() => void finish(false)}>취소</button>
      <button className="btn" disabled={busy || disabled} onClick={() => void finish(true)}>{busy ? "처리 중…" : "현재 규칙 초안 대신 적용"}</button>
    </div></div>}
  </>;
}
