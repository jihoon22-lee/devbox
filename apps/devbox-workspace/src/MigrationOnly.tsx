// Only importer views mount while the suite is awaiting activation.
import {useCallback,useState} from "react";
import type {ShellContentProps} from "@devbox/product-shell";
import RegistryGate from "./RegistryGate";
import RuntimeImport from "./RuntimeImport";
import TerminalImport from "./TerminalImport";
import LegacySessionImport from "./LegacySessionImport";
import LegacyRecoveryImport from "./LegacyRecoveryImport";
import LegacyLspImport from "./LegacyLspImport";
export default function MigrationOnly({description,route,refreshContext}:ShellContentProps){
 const [ready,setReady]=useState(false),[session,setSession]=useState(false),[recovery,setRecovery]=useState(false),[lsp,setLsp]=useState(false);
 const markReady=useCallback(()=>setReady(true),[]),applied=useCallback(()=>{},[]);
 return <section aria-label="Workspace 데이터 이전">
  <p>기존 데이터를 검토하고 가져올 수 있습니다. 작업·터미널·편집기는 Suite 활성화 후 사용할 수 있습니다.</p>
  <div hidden={ready&&route!=="overview"}><RegistryGate migrationOnly context={description.context} onContextChanged={refreshContext} onReady={markReady} editing={session||recovery||lsp}/></div>
  {ready&&["tasks","runtime","logs"].includes(route)&&<RuntimeImport description={description} active blocked={session||recovery||lsp}/>}
  {ready&&route==="terminal"&&<TerminalImport description={description} active/>}
  {ready&&route==="files"&&<>
   <LegacySessionImport description={description} disabled={recovery||lsp} onBusyChange={setSession} onApplied={applied}/>
   <LegacyRecoveryImport description={description} disabled={session||lsp} onBusyChange={setRecovery} onApplied={applied}/>
   <LegacyLspImport description={description} disabled={session||recovery} onBusyChange={setLsp} onApplied={applied}/>
  </>}
  {ready&&!['overview','tasks','runtime','logs','terminal','files'].includes(route)&&<p>이전은 프로젝트·파일·작업·터미널 화면에서 진행할 수 있습니다.</p>}
 </section>;
}
