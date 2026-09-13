import {useIncomingReview} from "@devbox/product-shell/incoming";
import { useCallback, useEffect, useState } from "react";
import type { Description, ProjectContext } from "@devbox/product-shell/api";
import type { Registry } from "./RegistryGate";
import { componentCall } from "./native";
import TerminalImport from "./TerminalImport";
import DevelopmentSessions from "./DevelopmentSessions";

interface Commands {profiles:Array<{id:string;name:string;revision:string}>}
interface Session {id:string;context:ProjectContext|null;state:string;restoreGeneration:number}
const labels:Record<string,string>={preparing:"준비 중",active:"실행 중",stopping:"종료 중",stopped:"종료됨",interrupted:"복구 검토 필요"};
export default function Terminal({description,registry}:{description:Description;registry:Registry|null}) {
  const {review:incoming,clear:clearIncoming}=useIncomingReview();
  const [commands,setCommands]=useState<Commands>({profiles:[]});
  const [profileId,setProfileId]=useState("");
  const [sessions,setSessions]=useState<Session[]>([]);
  const [busy,setBusy]=useState(false);
  const [issue,setIssue]=useState("");
  const call=useCallback(<T,>(method:string,args:Record<string,unknown>={})=>componentCall<T>(description,"workspace.terminal",method,args,"terminal"),[description]);
  const refresh=useCallback(async()=>{
    const [sessions,commands]=await Promise.all([call<Session[]>("terminal_sessions"),call<Commands>("terminal_commands")]);
    setSessions(sessions);setCommands(commands);
  },[call]);
  useEffect(()=>{let current=true;void Promise.all([call<Session[]>("terminal_sessions"),call<Commands>("terminal_commands")]).then(([sessions,commands])=>{if(current){setSessions(sessions);setCommands(commands);}}).catch(()=>{if(current)setIssue("터미널 세션 목록을 읽지 못했습니다.");});return()=>{current=false;};},[call]);
  useEffect(()=>{
    if(incoming?.route!=="terminal"||incoming.target.kind!=="entity"||incoming.target.entity!=="terminalProfile")return;
    let disposed=false;const id=incoming.target.id;
    void call<Commands>("terminal_commands").then(value=>{
      if(disposed)return;setCommands(value);
      const profile=value.profiles.find(profile=>profile.id===id&&profile.revision===incoming.commandRevision);
      if(profile)setProfileId(profile.id);else setIssue("받은 터미널 프로필이 변경되었거나 삭제되었습니다. 다시 선택해 주세요.");
      clearIncoming();
    }).catch(()=>{if(!disposed)setIssue("받은 터미널 프로필을 확인하지 못했습니다.");});
    return()=>{disposed=true;};
  },[incoming,call,clearIncoming]);

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
  const openProfile=async()=>{
    const profile=commands.profiles.find(profile=>profile.id===profileId);
    if(!profile||busy)return;
    const key="workspace-terminal-profile:"+JSON.stringify(description.context)+":"+profile.id+":"+profile.revision;
    setBusy(true);setIssue("");
    try{
      const operationId=sessionStorage.getItem(key)??crypto.randomUUID();
      sessionStorage.setItem(key,operationId);
      await call("open_terminal_profile",{operationId,profileId:profile.id,revision:profile.revision});
      sessionStorage.removeItem(key);await refresh();
    }catch{setIssue("프로필이 바뀌었거나 창을 열지 못했습니다. 목록을 새로 고친 뒤 다시 확인해 주세요.");}
    finally{setBusy(false);}
  };
  const summon=async(session:Session)=>{
    if(busy)return;
    const key="workspace-terminal-summon:"+session.id;
    setBusy(true);setIssue("");
    try{
      const saved=sessionStorage.getItem(key);
      const request=saved?JSON.parse(saved):{operationId:crypto.randomUUID(),terminalId:session.id,deadlineMs:Date.now()+30_000};
      if(request.deadlineMs<=Date.now()){sessionStorage.removeItem(key);setIssue("창 전환 요청이 만료되었습니다. 다시 선택해 주세요.");return;}
      sessionStorage.setItem(key,JSON.stringify(request));
      await call("summon_terminal",request);sessionStorage.removeItem(key);
    }catch{setIssue("창 전환을 확인하지 못했습니다. 같은 요청으로 다시 확인할 수 있습니다.");}
    finally{setBusy(false);}
  };
  const restore=async(session:Session)=>{
    const key="workspace-terminal-restore:"+description.handshake.installationId+":"+session.id+":"+session.restoreGeneration;
    setBusy(true);setIssue("");
    try {
      const operationId=sessionStorage.getItem(key)??crypto.randomUUID();
      sessionStorage.setItem(key,operationId);
      await call("restore_terminal",{id:session.id,operationId,expectedGeneration:session.restoreGeneration});
      sessionStorage.removeItem(key);await refresh();
    }catch{setIssue("재연결을 확인하지 못했습니다. 같은 요청으로 다시 확인하거나 목록을 새로 고쳐 주세요.");}
    finally{setBusy(false);}
  };
  return <section className="workspace-terminal-manager">
    <h1>터미널</h1>
    <TerminalImport description={description} active={true}/>
    <DevelopmentSessions description={description} registry={registry}/>
    <p>프로젝트별 터미널을 보조 창에서 엽니다. 창을 숨기거나 새로 고쳐도 실행 중인 터미널은 유지됩니다.</p>
    <button disabled={busy} onClick={()=>void open()}>{description.context?"현재 프로젝트의 터미널 열기":"터미널 열기"}</button>{" "}
    <button disabled={busy} onClick={()=>{setIssue("");setBusy(true);void refresh().catch(()=>setIssue("터미널 세션 목록을 읽지 못했습니다.")).finally(()=>setBusy(false));}}>새로 고침</button>
    <label>저장한 프로필 <select value={profileId} disabled={busy} onChange={event=>setProfileId(event.target.value)}>
      <option value="">프로필 선택</option>{commands.profiles.map(profile=><option key={profile.id} value={profile.id}>{profile.name}</option>)}
    </select></label>{" "}<button disabled={busy||!commands.profiles.some(profile=>profile.id===profileId)} onClick={()=>void openProfile()}>프로필로 터미널 열기</button>
    {!description.context&&<p>프로젝트 선택 없이도 터미널과 배포판을 사용할 수 있습니다.</p>}
    {issue&&<p role="alert">{issue}</p>}
    <ul>{sessions.map(session=><li key={session.id}>
      <span>{session.context?`${registry?.projects.find(project=>project.id===session.context?.projectId)?.name??"연결되지 않은 프로젝트"} · ${registry?.worktrees.find(tree=>tree.id===session.context?.worktreeId)?.binding.root??"작업 폴더 확인 필요"}`:"프로젝트 없는 터미널"} · {labels[session.state]??"상태 확인 필요"}</span>{" "}
      {session.state==="active"&&<button disabled={busy} onClick={()=>void action("focus_terminal",{id:session.id})}>창 표시</button>}{" "}
      {session.state==="active"&&JSON.stringify(session.context)===JSON.stringify(description.context)&&<button disabled={busy} onClick={()=>void summon(session)}>창 표시·숨김</button>}{" "}
      {["active","stopping"].includes(session.state)&&<button disabled={busy} onClick={()=>void action("stop_terminal",{id:session.id})}>이 터미널 종료</button>}
      {["stopped","interrupted"].includes(session.state)&&<button disabled={busy} onClick={()=>void restore(session)}>상태만 다시 연결</button>}
      {session.state==="interrupted"&&<p>저장한 레이아웃으로 다시 연결할 수 있습니다. 시작 명령은 보내지 않으며 기존 tmux·zellij 세션은 유지합니다.</p>}
    </li>)}</ul>
  </section>;
}
