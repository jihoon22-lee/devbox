import { useCallback, useEffect, useRef, useState } from "react";
import type { Description, ProjectContext } from "@devbox/product-shell/api";
import type { Registry } from "./RegistryGate";
import { componentCall } from "./native";

interface Candidate { id: string; name: string; kind: string; targetKind: string; targetDistro: string | null }
interface Job extends Candidate { command: string; cwd: string | null; envConfigured: boolean }
interface Session { id: string; context: ProjectContext; revision: number; planRevision: string; phase: string; issue: string | null }
interface Snapshot { sessions: Session[]; intents: Record<string, { jobs: string[] }> }
interface Plan { session: Session; jobs: Job[] }
const phaseLabels: Record<string, string> = { preflight: "환경 확인", review: "실행 검토", restoring: "상태 복원 중", preparing: "작업 준비 중", starting: "작업 시작 중", readiness: "준비 상태 확인 중", active: "사용 중", stopping: "정리 중", stopped: "종료됨", degraded: "확인 필요" };

export default function DevelopmentSessions({ description, registry }: { description: Description; registry: Registry | null }) {
  const [jobs, setJobs] = useState<Candidate[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [snapshot, setSnapshot] = useState<Snapshot>({ sessions: [], intents: {} });
  const [plan, setPlan] = useState<Plan | null>(null);
  const [issue, setIssue] = useState("");
  const [busy, setBusy] = useState(false);
  const [truncated, setTruncated] = useState(false);
  const contextKey = JSON.stringify(description.context);
  const current = useRef(contextKey);
  current.current = contextKey;
  const call = useCallback(<T,>(method: string, args: Record<string, unknown> = {}) => componentCall<T>(description, "workspace.terminal", method, args, "terminal"), [description]);

  useEffect(() => {
    let disposed = false;
    let pending = false;
    setPlan(null);
    setBusy(false);
    setIssue("");
    setSelected([]);
    const refresh = async () => {
      if (pending || disposed) return;
      pending = true;
      try { const value = await call<Snapshot>("development_sessions"); if (!disposed) setSnapshot(value); }
      catch { if (!disposed) setIssue("세션 상태를 읽지 못했습니다. 잠시 후 다시 확인해 주세요."); }
      finally { pending = false; }
    };
    void call<{ jobs: Candidate[]; truncated: boolean }>("development_candidates").then(value => { if (!disposed) { setJobs(value.jobs); setTruncated(value.truncated); } }).catch(() => { if (!disposed) setIssue("실행할 작업 목록을 읽지 못했습니다."); });
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1000);
    return () => { disposed = true; window.clearInterval(timer); };
  }, [call, contextKey]);

  const prepare = async (ids = selected) => {
    const context = contextKey;
    if (!description.context) return;
    setBusy(true); setIssue(""); setPlan(null);
    const key = `workspace-development-prepare:${description.handshake.installationId}:${context}:${JSON.stringify(ids)}`;
    try {
      const operationId = sessionStorage.getItem(key) ?? crypto.randomUUID();
      sessionStorage.setItem(key, operationId);
      const next = await call<Plan>("prepare_development_session", { operationId, jobs: ids });
      sessionStorage.removeItem(key);
      if (next.session.phase !== "review") throw new Error("session requires a fresh review");
      if (current.current === context) setPlan(next);
    } catch { if (current.current === context) setIssue("프로젝트 환경이나 작업 정의를 확인하지 못했습니다. 변경된 작업과 대상 배포판을 확인해 주세요."); }
    finally { if (current.current === context) setBusy(false); }
  };
  const start = async (mode: "restoreOnly" | "startReviewed") => {
    if (!plan) return;
    const context = contextKey;
    setBusy(true); setIssue("");
    try {
      await call("start_development_session", { id: plan.session.id, revision: plan.session.revision, planRevision: plan.session.planRevision, mode });
      if (current.current === context) { setPlan(null); setSnapshot(await call<Snapshot>("development_sessions")); }
    } catch { if (current.current === context) setIssue("시작 결과를 확인하지 못했습니다. 세션 상태를 새로 확인하고, 정의가 바뀌었다면 다시 검토해 주세요."); }
    finally { if (current.current === context) setBusy(false); }
  };
  const stop = async (id: string) => {
    const context = contextKey;
    setBusy(true); setIssue("");
    try { await call("stop_development_session", { id }); const next = await call<Snapshot>("development_sessions"); if (current.current === context) setSnapshot(next); }
    catch { if (current.current === context) setIssue("정리를 완료하지 못했습니다. 현재 소유 상태를 확인한 뒤 다시 요청해 주세요."); }
    finally { if (current.current === context) setBusy(false); }
  };

  return <section aria-label="개발 세션" className="workspace-development-sessions">
    <h2>개발 세션</h2>
    <p>작업 폴더의 상태 복원과 작업 실행을 따로 선택합니다. 세션 종료는 이 세션이 시작한 자원만 정리하며, 다른 세션이 사용하는 공유 서비스는 유지합니다.</p>
    {description.context ? <>
      <fieldset disabled={busy}><legend>함께 사용할 작업과 서비스</legend>
        {jobs.map(job => <label key={job.id} className="workspace-session-choice"><input type="checkbox" checked={selected.includes(job.id)} onChange={event => setSelected(previous => event.target.checked ? [...previous, job.id] : previous.filter(id => id !== job.id))} disabled={!selected.includes(job.id) && selected.length >= 16} /> {job.name} · {job.kind === "service" ? "서비스" : "작업"} · {job.targetDistro ?? "Windows"}</label>)}
        {jobs.length === 0 && <p>Tasks &amp; Services에서 작업을 등록하면 함께 실행할 수 있습니다.</p>}
        {truncated && <p role="status">작업 목록이 표시 한도에 도달했습니다. Tasks &amp; Services에서 사용하지 않는 정의를 정리해 주세요.</p>}
      </fieldset>
      <button disabled={busy} onClick={() => void prepare()}>환경 확인·실행 검토</button>
    </> : <p>프로젝트와 작업 폴더를 선택하면 개발 세션을 시작할 수 있습니다.</p>}
    {plan && <section aria-label="개발 세션 실행 검토">
      <h3>실행할 내용 확인</h3>
      {plan.jobs.map(job => <article key={job.id}><strong>{job.name}</strong><pre>{job.command}</pre><p>작업 폴더: {job.cwd ?? "기본 폴더"} · 대상: {job.targetDistro ?? "Windows"} · 환경 설정: {job.envConfigured ? "구성됨" : "없음"}</p>{job.kind === "service" && <p>이미 실행 중인 서비스는 참조만 연결합니다. 새로 시작한 서비스도 다른 세션이 사용하는 동안 유지합니다.</p>}</article>)}
      <button disabled={busy} onClick={() => void start("restoreOnly")}>상태만 이어가기</button>{" "}
      <button disabled={busy} onClick={() => void start("startReviewed")}>검토한 작업 실행</button>{" "}
      <button disabled={busy} onClick={() => { void stop(plan.session.id); setPlan(null); }}>취소</button>
    </section>}
    {issue && <p role="alert">{issue}</p>}
    <ul>{snapshot.sessions.map(session => {
      const project = registry?.projects.find(project => project.id === session.context.projectId)?.name ?? "연결되지 않은 프로젝트";
      const root = registry?.worktrees.find(tree => tree.id === session.context.worktreeId)?.binding.root ?? "작업 폴더 확인 필요";
      const sameContext = JSON.stringify(session.context) === contextKey;
      return <li key={session.id}><strong>{project}</strong> · {root} · {phaseLabels[session.phase] ?? "상태 확인 필요"}{" "}
        {session.phase !== "stopped" && session.issue !== "session_native_owner_lost" && <button disabled={busy} onClick={() => void stop(session.id)}>{session.phase === "stopping" ? "정리 다시 확인" : "내가 시작한 자원 정리"}</button>}{" "}
        {["stopped", "degraded"].includes(session.phase) && sameContext && <button disabled={busy} onClick={() => void prepare(snapshot.intents[session.id]?.jobs ?? [])}>새 계획으로 이어가기</button>}
        {session.issue && <p>{session.issue === "session_native_owner_lost" ? "이전 실행의 소유권을 이어받지 않았습니다. 새 계획을 검토한 뒤 이어가세요." : "작업 시작 또는 정리에 확인이 필요합니다. Tasks & Services의 실행 상태와 로그를 확인해 주세요."}</p>}
      </li>;
    })}</ul>
  </section>;
}
