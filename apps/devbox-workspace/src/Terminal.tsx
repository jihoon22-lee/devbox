import { useCallback, useEffect, useState } from "react";
import type { Description, ProjectContext } from "@devbox/product-shell/api";
import type { Registry } from "./RegistryGate";
import { componentCall } from "./native";
import TerminalImport from "./TerminalImport";
import DevelopmentSessions from "./DevelopmentSessions";

interface Session {id:string;context:ProjectContext|null;state:string}
const labels:Record<string,string>={preparing:"준비 중",active:"실행 중",stopping:"종료 중",stopped:"종료됨",interrupted:"복구 검토 필요"};
export default function Terminal({description,registry}:{description:Description;registry:Registry|null}) {
  const [sessions,setSessions]=useState<Session[]>([]);
  const [busy,setBusy]=useState(false);
  const [issue,setIssue]=useState("");
  const call=useCallback(<T,>(method:string,args:Record<string,unknown>={})=>componentCall<T>(description,"workspace.terminal",method,args,"terminal"),[description]);
  const refresh=useCallback(async()=>{setSessions(await call<Session[]>("terminal_sessions"));},[call]);
  useEffect(()=>{let current=true;void call<Session[]>("terminal_sessions").then(value=>{if(current)setSessions(value);}).catch(()=>{if(current)setIssue("터미널 세션 목록을 읽지 못했습니다.");});return()=>{current=false;};},[call]);
  const action=async(method:string,args:Record<string,unknown>)=>{
    setBusy(true);setIssue("");
    try {await call(method,args);await refresh();}catch {setIssue("터미널 작업을 완료하지 못했습니다. 상태를 새로 고친 뒤 다시 확인해 주세요.");}finally{setBusy(false);}
  };
  const open=async()=>{
    const key=`workspace-terminal-open:${description.handshake.installationId}:${JSON.stringify(description.context)}`;
    setBusy(true);setIssue("");
    try {
      const operationId=sessionStorage.getItem(key)??crypto.randomUUID();
      sessionStorage.setItem(key,operationId);
      await call("open_terminal",{operationId});
      sessionStorage.removeItem(key);
      await refresh();
    }catch {setIssue("터미널 열기를 완료하지 못했습니다. 같은 요청으로 다시 확인할 수 있습니다.");}finally{setBusy(false);}
  };
  return <section className="workspace-terminal-manager">
    <h1>터미널</h1>
    <TerminalImport description={description} active={true}/>
    <DevelopmentSessions description={description} registry={registry}/>
    <p>프로젝트별 터미널을 보조 창에서 엽니다. 창을 숨기거나 새로 고쳐도 실행 중인 터미널은 유지됩니다.</p>
    <button disabled={busy} onClick={()=>void open()}>{description.context?"현재 프로젝트의 터미널 열기":"터미널 열기"}</button>{" "}
    <button disabled={busy} onClick={()=>{setIssue("");setBusy(true);void refresh().catch(()=>setIssue("터미널 세션 목록을 읽지 못했습니다.")).finally(()=>setBusy(false));}}>새로 고침</button>
    {!description.context&&<p>프로젝트 선택 없이도 터미널과 배포판을 사용할 수 있습니다.</p>}
    {issue&&<p role="alert">{issue}</p>}
    <ul>{sessions.map(session=><li key={session.id}>
      <span>{session.context?`${registry?.projects.find(project=>project.id===session.context?.projectId)?.name??"연결되지 않은 프로젝트"} · ${registry?.worktrees.find(tree=>tree.id===session.context?.worktreeId)?.binding.root??"작업 폴더 확인 필요"}`:"프로젝트 없는 터미널"} · {labels[session.state]??"상태 확인 필요"}</span>{" "}
      {session.state==="active"&&<button disabled={busy} onClick={()=>void action("focus_terminal",{id:session.id})}>창 표시</button>}{" "}
      {["active","stopping"].includes(session.state)&&<button disabled={busy} onClick={()=>void action("stop_terminal",{id:session.id})}>이 터미널 종료</button>}
      {session.state==="interrupted"&&<p>이전 프로세스의 실행 상태를 이어받지 않았습니다. 작업과 시작 명령을 확인한 뒤 새 터미널을 열어 주세요.</p>}
    </li>)}</ul>
  </section>;
}
