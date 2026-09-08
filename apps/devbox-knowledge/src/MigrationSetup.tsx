import { useCallback, useEffect, useRef, useState } from "react";
import { componentInvoke } from "@devbox/knowledge-features/transport";
import { issueError } from "./transport";
const invoke = componentInvoke("knowledge.migration");
type Source = "notes" | "activity" | "search";
const names: Record<Source, string> = { notes: "Knowledge · 노트 설정과 템플릿", activity: "Life Log · 활동 이력과 설정", search: "Everything+ · 검색 위치와 저장 검색" };
interface SourceInfo { source: Source; available: boolean }
interface Plan {
  id: string; phase: "building" | "prepared" | "activating" | "activated" | "cancelling" | "cancelled" | "rolling_back" | "rolled_back";
  preparedAtMs: number; hasPrevious: boolean; vault: string | null;
  sources: { source: Source; bytes: number; report: { imported: number; repeated: number; conflicts: number; retired: number; reservedRootIds: number } }[];
}
interface Job { state: "running" | "succeeded" | "failed" | "cancelled"; cancellationRequested?: boolean; value?: { plan: Plan; active: boolean }; issue?: string }
export default function MigrationSetup({ onActivated, onBack }: { onActivated: () => void; onBack: () => void }) {
  const [sources, setSources] = useState<SourceInfo[]>([]);
  const [selected, setSelected] = useState<Source[]>([]);
  const [plans, setPlans] = useState<Plan[]>([]);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [loading, setLoading] = useState(true);
  const [blocked, setBlocked] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [jobId, setJobId] = useState<string | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const jobRef = useRef<string | null>(null);
  const busyRef = useRef(false);
  const aliveRef = useRef(true);
  const initializedSelection = useRef(false);
  const activatedRef = useRef(onActivated);
  activatedRef.current = onActivated;
  const refresh = useCallback(async () => {
    const [sources, plans] = await Promise.all([invoke<SourceInfo[]>("list_import_sources"), invoke<Plan[]>("list_imports")]);
    if (!aliveRef.current) return;
    setSources(sources); setPlans(plans);
    if (!initializedSelection.current) { initializedSelection.current = true; setSelected(sources.filter(s => s.available).map(s => s.source)); }
  }, []);
  useEffect(() => {
    aliveRef.current = true;
    void refresh().catch(error => { if (aliveRef.current) setError(error instanceof Error ? error.message : "이전 데이터를 확인하지 못했습니다."); })
      .finally(() => { if (aliveRef.current) setLoading(false); });
    return () => { aliveRef.current = false; if (jobRef.current) void invoke("cancel_import_job", { jobId: jobRef.current }).catch(() => {}); };
  }, [refresh]);
  useEffect(() => {
    if (!jobId) return;
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      try {
        const job = await invoke<Job>("import_job", { jobId });
        if (!alive) return;
        if (job.state === "running") { setCancelling(job.cancellationRequested === true); timer = setTimeout(() => void poll(), 250); return; }
        jobRef.current = null; busyRef.current = false; setJobId(null); setCancelling(false);
        if (job.state === "succeeded" && job.value) {
          if (job.value.active) { activatedRef.current(); return; }
          setPlan(job.value.plan.phase === "prepared" ? job.value.plan : null);
        } else {
          const error = issueError(job.issue); setError(error.message);
          if (error.name === "restart_required") setBlocked(true);
        }
        await refresh();
      } catch (error) {
        if (!alive) return;
        // A poll failure does not prove the native job stopped. Request its
        // cancellation and keep the durable plan available for next startup.
        void invoke("cancel_import_job", { jobId }).catch(() => {});
        setError(error instanceof Error ? error.message : "가져오기 상태를 확인하지 못했습니다. 다시 시작해 확인해 주세요.");
        setBlocked(true); setCancelling(true);
      }
    };
    void poll();
    return () => { alive = false; if (timer) clearTimeout(timer); };
  }, [jobId, refresh]);
  const start = async (method: string, args: Record<string, unknown>) => {
    if (busyRef.current || blocked) return;
    busyRef.current = true; setLoading(true); setError(null); setCancelling(false);
    try {
      const value = await invoke<{ jobId: string }>(method, args);
      jobRef.current = value.jobId;
      if (!aliveRef.current) { void invoke("cancel_import_job", { jobId: value.jobId }).catch(() => {}); return; }
      setJobId(value.jobId);
    } catch (error) {
      busyRef.current = false;
      if (aliveRef.current) { setError(error instanceof Error ? error.message : "가져오기를 시작하지 못했습니다."); if (error instanceof Error && error.name === "restart_required") setBlocked(true); }
    } finally { if (aliveRef.current) setLoading(false); }
  };
  const cancel = async () => {
    if (!jobRef.current) return;
    setCancelling(true);
    try { await invoke("cancel_import_job", { jobId: jobRef.current }); }
    catch (error) { setError(error instanceof Error ? error.message : "취소 요청을 보내지 못했습니다."); }
  };
  const busy = loading || jobId !== null;
  return <section className="knowledge-migration" aria-labelledby="migration-title">
    <h2 id="migration-title">가져오기 검토</h2>
    <p>이전 앱 데이터의 일관된 사본에서 사용자 설정과 이력을 준비합니다. Markdown·이미지 파일은 원래 위치에 보존하고, 현재 저장소를 덮어쓰지 않습니다.</p>
    <fieldset disabled={busy || blocked}><legend>가져올 이전 앱</legend>
      {sources.map(source => <label key={source.source}><input type="checkbox" checked={selected.includes(source.source)} disabled={!source.available} onChange={event => setSelected(previous => event.target.checked ? [...previous, source.source] : previous.filter(s => s !== source.source))}/>{names[source.source]}{!source.available && " · 이전 데이터 없음"}</label>)}
    </fieldset>
    <button type="button" disabled={busy || blocked || selected.length === 0} onClick={() => void start("prepare_import", { sources: selected })}>가져오기 미리보기 준비</button>
    {jobId && <div role="status"><p>{cancelling ? "취소를 요청했습니다. 이미 적용된 데이터는 유지합니다." : "가져오기 작업을 진행하고 있습니다…"}</p><button type="button" disabled={cancelling} onClick={() => void cancel()}>작업 취소</button></div>}
    {error && <p role="alert">{error}</p>}
    {plan && <section aria-labelledby="import-preview-title">
      <h3 id="import-preview-title">적용할 미리보기</h3>
      <p>{new Date(plan.preparedAtMs).toLocaleString()}에 준비한 사본입니다.</p>
      {plan.vault && <p>노트 위치: <span className="knowledge-migration-path">{plan.vault}</span></p>}
      <table><caption>이전 데이터와 현재 데이터의 처리 결과</caption><thead><tr><th scope="col">이전 앱</th><th scope="col">가져올 항목</th><th scope="col">반복 항목</th><th scope="col">충돌</th><th scope="col">종료한 임시 상태</th></tr></thead>
        <tbody>{plan.sources.map(source => <tr key={source.source}><th scope="row">{names[source.source]}</th><td>{source.report.imported}</td><td>{source.report.repeated}</td><td>{source.report.conflicts}</td><td>{source.report.retired}</td></tr>)}</tbody>
      </table>
      <p>충돌한 기존 설정과 수정은 유지하며, 처음 가져오는 같은 이름의 템플릿은 별도 이름으로 보존합니다. 반복 가져오기는 이미 수정·삭제한 항목을 되돌리지 않습니다. 이전 앱의 수집 동의는 가져오지 않으며, 미완료 전달은 만료 상태로 보존합니다.</p>
      {plan.sources.some(source => source.report.reservedRootIds > 0) && <p>삭제된 검색 위치를 가리키는 저장 검색은 다른 위치로 바뀌지 않도록 연결되지 않은 상태로 유지합니다.</p>}
      <p>이전 Knowledge 앱을 완전히 종료한 뒤 적용해 주세요. 검색 인덱스는 연결 가능한 위치부터 다시 준비합니다.</p>
      <button type="button" disabled={busy || blocked} onClick={() => void start("activate_import", { planId: plan.id })}>미리보기를 확인하고 적용</button>
      <button type="button" disabled={busy || blocked} onClick={() => void start("discard_import", { planId: plan.id })}>이 미리보기 취소</button>
    </section>}
    {plans.some(plan => !["cancelled", "rolled_back"].includes(plan.phase)) && <section aria-labelledby="import-recovery-title"><h3 id="import-recovery-title">준비·복구 기록</h3>
      <p>전환 뒤 변경된 데이터가 있으면 되돌리기를 거부하고 두 저장소를 모두 보존합니다. 되돌려도 새로 만든 Markdown·이미지 파일은 삭제하지 않습니다.</p>
      <ul>{plans.filter(plan => !["cancelled", "rolled_back"].includes(plan.phase)).map((saved, index) => <li key={saved.id}>
        <span>가져오기 {index + 1} · {saved.phase === "prepared" ? "적용 전" : ["building", "cancelling"].includes(saved.phase) ? "준비 미완료" : "전환 기록"}</span>{" "}
        {saved.phase === "prepared" && <button type="button" disabled={busy || blocked} onClick={() => setPlan(saved)}>미리보기 열기</button>}
        {["prepared", "building", "cancelling"].includes(saved.phase) && <button type="button" disabled={busy || blocked} onClick={() => void start("discard_import", { planId: saved.id })}>준비 사본 삭제</button>}
        {["activating", "activated", "rolling_back"].includes(saved.phase) && <button type="button" disabled={busy || blocked} onClick={() => { if (window.confirm("변경이 없는 경우에만 이전 저장소로 돌아갑니다. 새로 만든 Markdown과 이미지는 보존됩니다. 진행할까요?")) void start("rollback_import", { planId: saved.id }); }}>이전 저장소로 되돌리기</button>}
      </li>)}</ul>
    </section>}
    <button type="button" disabled={busy} onClick={onBack}>시작 화면으로</button>
  </section>;
}
