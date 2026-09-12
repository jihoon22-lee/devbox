import { useCallback, useEffect, useState } from "react";
import type { Description } from "@devbox/product-shell/api";
import { componentCall } from "./native";
interface Job { id: string; state: string; issue: string | null }
interface Review { id: string; revision: string; sourceRevision: string; applied: boolean; profiles: Array<{ sourceId: string; name: string; destinationId: string }>; preferenceKeys: string[]; conflicts: string[]; notices: Array<{ store: string; state: string }> }
const states: Record<string,string>={preparing:"복사·검토 중",ready:"가져오기 준비됨",failed:"준비 실패",cancelled:"취소됨",interrupted:"중단됨 — 새로 준비 필요"};
const notices:Record<string,string>={ready:"이전 가능",missing:"원본 없음","explicit-restore-profile":"명시적으로 복원할 프로필로 이전","invalid-or-future-schema":"손상되었거나 지원하지 않는 버전","invalid-or-unsupported":"지원하지 않는 값 — 원본 보존","invalid-future-schema-or-capacity":"형식 또는 프로필 개수 확인 필요","missing-or-ambiguous-browser-store":"WebView 저장소가 없거나 두 위치에 존재함 — 자동 이전 불가"};
export default function TerminalImport({description,active}:{description:Description;active:boolean}) {
  const [jobs,setJobs]=useState<Job[]>([]);
  const [review,setReview]=useState<Review|null>(null);
  const [replace,setReplace]=useState(false);
  const [busy,setBusy]=useState(false);
  const [issue,setIssue]=useState("");
  const call=useCallback(<T,>(method:string,args:Record<string,unknown>={})=>componentCall<T>(description,"workspace.terminal",method,args,"terminal"),[description]);
  const refresh=useCallback(async()=>setJobs(await call<Job[]>("terminal_imports")),[call]);
  useEffect(()=>{if(!active)return;let disposed=false;const poll=()=>void call<Job[]>("terminal_imports").then(value=>{if(!disposed)setJobs(value);}).catch(()=>{if(!disposed)setIssue("이전 작업 목록을 읽지 못했습니다.");});poll();const timer=setInterval(poll,2000);return()=>{disposed=true;clearInterval(timer);};},[active,call]);
  const perform=async(action:()=>Promise<void>)=>{setBusy(true);setIssue("");try{await action();await refresh();}catch{setIssue("이전 작업을 완료하지 못했습니다. WSL Desktop과 Workspace의 터미널 창을 종료하고 상태를 다시 확인해 주세요.");}finally{setBusy(false);}};
  return <details className="workspace-terminal-import"><summary>WSL Desktop 프로필·설정 가져오기</summary>
    <p>기존 WSL Desktop을 종료한 뒤 준비해 주세요. 프로필과 마지막 레이아웃, 고정·최근 경로, 글꼴·복사·사용자 설정을 검토합니다. 가져오기 후 터미널 복원과 시작 명령 실행은 따로 선택합니다.</p>
    <button disabled={busy||jobs.some(job=>job.state==="preparing")} onClick={()=>void perform(async()=>{
      const key=`terminal-import:${description.handshake.installationId}`;
      const id=sessionStorage.getItem(key)??crypto.randomUUID();sessionStorage.setItem(key,id);
      await call("start_terminal_import",{id});sessionStorage.removeItem(key);
    })}>원본을 보존하며 이전 준비</button>
    {issue&&<p role="status">{issue}</p>}
    <ul>{jobs.map(job=><li key={job.id}>{states[job.state]??"상태 확인 필요"}
      {job.issue&&<span> · {job.issue==="terminal_import_copy_cleanup_pending"?"임시 복사본 정리 재시도 필요":"원본 또는 중단된 작업 확인 필요"}</span>}
      {job.state==="preparing"&&<button disabled={busy} onClick={()=>void perform(async()=>{await call("cancel_terminal_import",{id:job.id});})}>취소</button>}
      {job.state==="ready"&&<button disabled={busy} onClick={()=>void perform(async()=>{setReview(await call<Review>("preview_terminal_import",{id:job.id}));setReplace(false);})}>이전 내용 검토</button>}
    </li>)}</ul>
    {review&&<section aria-label="터미널 이전 검토"><p>프로필 {review.profiles.length}개 · 설정 {review.preferenceKeys.length}개</p>
      <ul>{review.profiles.map(profile=><li key={profile.sourceId}>{profile.name}</li>)}</ul>
      <ul>{review.notices.map(notice=><li key={notice.store}>{notice.store}: {notices[notice.state]??"수동 확인 필요"}</li>)}</ul>
      {review.conflicts.length>0&&<label><input type="checkbox" checked={replace} onChange={event=>setReplace(event.target.checked)}/>현재 설정 {review.conflicts.length}개를 검토한 이전 값으로 교체 (선택하지 않으면 현재 값 유지)</label>}
      <p>이름이 같은 기존 프로필은 보존하며 별도 프로필을 추가합니다. 적용 전 Workspace 터미널 창을 종료해 주세요.</p>
      <button disabled={busy||review.applied} onClick={()=>void perform(async()=>{await call("apply_terminal_import",{id:review.id,expectedRevision:review.revision,sourceRevision:review.sourceRevision,replacePreferences:replace});setReview(await call<Review>("preview_terminal_import",{id:review.id}));})}>{review.applied?"이미 적용됨":"검토한 프로필·설정 가져오기"}</button>
    </section>}
  </details>;
}
