import { useEffect, useRef, useState, type ReactNode } from "react";
import { nativeMode } from "@devbox/product-shell/api";
import { componentInvoke } from "@devbox/api-studio-features/transport";
import { applyBrowserPatch, captureBrowser, type BrowserPatch, type Summary } from "./protocol";
const invoke = componentInvoke("api-studio.migration");
type LegacyApp = "api-playground" | "webhook-lab" | "developer-toolbox";
const labels: Record<LegacyApp, string> = { "api-playground": "API Playground", "webhook-lab": "Webhook Lab", "developer-toolbox": "Developer Toolbox" };
interface Status { profiles?: { id: string; bytes: number }[]; busy: boolean; operationId?: string; reviewNeeded?: boolean; pending?: string | null; sources?: { app: LegacyApp; present: boolean; readable: boolean }[] }
interface Review { review: { id: string; summary: Summary; snapshot: string }; issues: { store: string; code: string; count: number }[] }
export function MigrationStartup({ children }: { children: ReactNode }) { return nativeMode ? <NativeStartup>{children}</NativeStartup> : <>{children}</>; }
function NativeStartup({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(false); const [status, setStatus] = useState<Status | null>(null);
  const [selected, setSelected] = useState<LegacyApp[]>([]); const [review, setReview] = useState<Review | null>(null);
  const [profiles, setProfiles] = useState<string[] | null>(null);
  const [busy, setBusy] = useState<"preparing" | "applying" | null>(null); const [error, setError] = useState<string | null>(null);
  const [completed, setCompleted] = useState<string | null>(null); const [attempt, setAttempt] = useState(0);
  const mounted = useRef(true); const operation = useRef<string | null>(null);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  useEffect(() => {
    let alive = true; let timer: ReturnType<typeof setTimeout> | undefined;
    async function load() {
      try {
        const value = await invoke<Status>("migration_status"); if (!alive) return;
        setStatus(value); setProfiles((value.profiles ?? []).map((profile) => profile.id));
        if (value.busy) { operation.current = value.operationId ?? null; timer = setTimeout(() => { void load(); }, 250); return; }
        if (!value.reviewNeeded && !value.pending) { await invoke("finish_startup"); if (alive) setReady(true); }
        else setSelected((value.sources ?? []).filter((source) => source.present && source.readable).map((source) => source.app));
      } catch (cause) { if (alive) setError(cause instanceof Error ? cause.message : "가져오기 상태를 확인하지 못했습니다."); }
    }
    void load(); return () => { alive = false; if (timer) clearTimeout(timer); };
  }, [attempt]);
  async function open() {
    setError(null);
    try { await invoke("finish_startup"); if (mounted.current) setReady(true); }
    catch (cause) { if (mounted.current) setError(cause instanceof Error ? cause.message : "앱을 열지 못했습니다."); }
  }
  async function prepare() {
    const id = crypto.randomUUID(); operation.current = id; setBusy("preparing"); setError(null); setReview(null); setCompleted(null);
    try {
      const value = await invoke<Review>("prepare_migration", { operationId: id, sources: selected, browser: captureBrowser(), profileIds: profiles });
      if (mounted.current && operation.current === id) setReview(value);
    } catch (cause) { if (mounted.current) setError(cause instanceof Error ? cause.message : "기존 데이터를 확인하지 못했습니다."); }
    finally { if (mounted.current) { setBusy(null); operation.current = null; } }
  }
  async function cancel() {
    const id = operation.current; if (!id) return;
    try { await invoke("cancel_migration", { operationId: id }); }
    catch (cause) { if (mounted.current) setError(cause instanceof Error ? cause.message : "취소 상태를 확인하지 못했습니다."); }
  }
  async function apply(id: string, rollback: boolean) {
    const operationId = crypto.randomUUID(); operation.current = operationId; setBusy("applying"); setError(null);
    try {
      const patch = rollback
        ? await invoke<BrowserPatch>("rollback_migration", { operationId, id })
        : await invoke<BrowserPatch>("apply_migration", { operationId, id, browser: captureBrowser() });
      const browser = applyBrowserPatch(patch);
      await invoke("acknowledge_migration", { operationId: crypto.randomUUID(), id, browser, rollback });
      if (mounted.current) { setStatus((previous) => previous ? { ...previous, pending: null } : previous); setReview(null); setCompleted(rollback ? "가져오기 이전 상태로 되돌렸습니다." : "데이터를 가져왔습니다. 요청을 보내거나 서버를 시작하기 전에 환경과 설정을 확인해 주세요."); }
    } catch (cause) {
      if (mounted.current) {
        setError(cause instanceof Error ? cause.message : "가져오기를 완료하지 못했습니다.");
        try { const value = await invoke<Status>("migration_status"); if (mounted.current) setStatus(value); } catch { /* Keep the original actionable error. */ }
      }
    } finally { if (mounted.current) { setBusy(null); operation.current = null; } }
  }
  if (ready) return <>{children}<button className="migration-reopen" onClick={() => {
    if (window.confirm("열린 요청을 저장했나요? 앱을 다시 시작하고 기존 데이터 가져오기를 엽니다.")) void invoke("request_import_on_restart").catch(() => setError("앱을 다시 시작하지 못했습니다."));
  }}>기존 데이터 가져오기…</button>{error && <p role="alert">{error}</p>}</>;
  return <section className="migration-start" aria-labelledby="migration-title">
    <h1 id="migration-title">기존 데이터 가져오기</h1>
    <p>기존 앱의 원본은 보존하며, 선택한 데이터를 API Studio에 추가합니다. 요청·서버·프로토콜 세션은 시작하지 않습니다.</p>
    {error && <p role="alert">{error}</p>}
    {completed && <p role="status">{completed}</p>}
    {!status && !error && <p role="status">저장된 데이터 상태를 확인하고 있습니다…</p>}
    {status?.pending ? <>
      <p>중단된 가져오기가 있습니다. 반영을 계속하거나 가져오기 이전 상태로 되돌릴 수 있습니다.</p>
      <div className="migration-actions"><button disabled={!!busy} onClick={() => void apply(status.pending!, false)}>계속 반영</button><button disabled={!!busy} onClick={() => void apply(status.pending!, true)}>이전 상태로 되돌리기</button></div>
    </> : <>
      {status?.sources && !completed && <fieldset disabled={!!busy}><legend>가져올 앱</legend>{status.sources.map((source) => <label key={source.app}>
        <input type="checkbox" checked={selected.includes(source.app)} disabled={!source.present || !source.readable} onChange={(event) => setSelected((old) => event.target.checked ? [...old, source.app] : old.filter((app) => app !== source.app))}/>
        {labels[source.app]} {!source.present ? "· 저장된 데이터 없음" : !source.readable ? "· 저장 위치 확인 필요" : ""}
      </label>)}</fieldset>}
      {!!status?.profiles?.length && selected.includes("webhook-lab") && <details><summary>가져올 모의 서버 프로필 선택 ({profiles?.length ?? 0}개)</summary>
        <p>프로필은 한 번에 20 MB까지 가져올 수 있습니다. 큰 저장소는 나누어 가져와 주세요.</p>
        <fieldset disabled={!!busy}><legend>모의 서버 프로필</legend>{status.profiles.map((profile) => <label key={profile.id}>
          <input type="checkbox" checked={profiles?.includes(profile.id) ?? false} onChange={(event) => setProfiles((old) => event.target.checked ? [...(old ?? []), profile.id] : (old ?? []).filter((id) => id !== profile.id))}/>
          {profile.id} · {(profile.bytes / 1024).toFixed(1)} KB
        </label>)}</fieldset>
      </details>}
      {review && <div className="migration-review" data-migration-review>
        <p>추가 {review.review.summary.added}개 · 기존 일치 {review.review.summary.matched}개 · 이미 가져옴 {review.review.summary.alreadyImported}개</p>
        {review.review.summary.conflicts > 0 && <p>ID·이름 조정 또는 기존 연결 유지: {review.review.summary.conflicts}건</p>}
        {review.review.summary.capacityExcluded > 0 && <p>현재 저장 한도로 제외한 항목: {review.review.summary.capacityExcluded}개</p>}
        {review.issues.map((issue, index) => <p key={index}>{issue.code === "secret-reconnect-required" ? "다시 연결해야 하는 비밀 값" : issue.code === "legacy-history-excluded" ? "원문 안전성을 확인할 수 없어 제외한 v1 History 저장소" : "마스킹하거나 제외한 항목"}: {issue.count}개</p>)}
        <button disabled={!!busy} onClick={() => void apply(review.review.id, false)}>이 계획으로 가져오기</button>
      </div>}
      <div className="migration-actions">
        {!completed && <button disabled={!!busy || selected.length === 0 || status?.busy} onClick={() => void prepare()}>선택한 데이터 확인</button>}
        {busy === "preparing" || status?.busy ? <button onClick={() => void cancel()}>확인 취소</button> : null}
        <button disabled={!!busy || status?.busy || !status} onClick={() => void open()}>{completed ? "API Studio 열기" : "가져오기 없이 계속"}</button>
      </div>
    </>}
    {busy && <p role="status">{busy === "preparing" ? "기존 데이터를 확인하고 있습니다… API Playground는 닫아 주세요." : "데이터 반영 상태를 기록하고 있습니다…"}</p>}
    {error && !busy && <button onClick={() => { setError(null); setAttempt((old) => old + 1); }}>상태 다시 확인</button>}
  </section>;
}
