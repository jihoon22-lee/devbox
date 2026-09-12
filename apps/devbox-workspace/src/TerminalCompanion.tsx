import { lazy, Suspense, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isProjectContext, makeRequest, type Handshake, type ProjectContext } from "@devbox/product-shell/api";
import { configureProductTransport } from "@devbox/workspace-features/transport";
import { initializeProductLayout } from "@devbox/workspace-features/terminal-layout";
import { configureTerminalStorage, initializeTerminalPreferences } from "@devbox/workspace-features/terminal-storage";

const Terminal=lazy(()=>import("@devbox/workspace-features/terminal"));
interface Peer {handshake:Handshake;context:ProjectContext|null;id:string}
let initialization:Promise<Peer>|undefined;
function connect():Promise<Peer> {
  initialization??=invoke<Peer>("plugin:workspace|terminal_describe").then(async peer=>{
    if(peer.handshake.product!=="workspace"||peer.handshake.protocolVersion!==1||(peer.context!==null&&!isProjectContext(peer.context))
      ||!peer.id||!/^[a-f0-9-]{36}$/.test(peer.id))throw new Error("invalid companion");
    configureTerminalStorage(peer.handshake.installationId,peer.id);
    configureProductTransport(<T,>(component:string,method:string,args:Record<string,unknown>)=>{
      if(component!=="workspace.terminal")return Promise.reject(new Error("이 창에서 사용할 수 없는 기능입니다."));
      const header=makeRequest(peer.handshake,"terminal",Date.now(),peer.context);
      // Frequent bounded output pulls expire promptly in the native replay cache.
      header.deadlineMs=Date.now()+(method==="terminal_output"?1000:30_000);
      return invoke<T>("plugin:workspace|terminal_execute",{request:{header,method,args}});
    },peer.handshake.installationId);
    await initializeTerminalPreferences();
    const header=makeRequest(peer.handshake,"terminal",Date.now(),peer.context);
    const layout=await invoke<{revision:string;layout:unknown}>("plugin:workspace|terminal_execute",{request:{header,method:"terminal_layout",args:{}}});
    initializeProductLayout(peer.id,layout);
    return peer;
  });
  return initialization;
}
export default function TerminalCompanion() {
  const [peer,setPeer]=useState<Peer|null>(null);
  const [error,setError]=useState(false);
  useEffect(()=>{let current=true;void connect().then(value=>{if(current)setPeer(value);}).catch(()=>{if(current)setError(true);});return()=>{current=false;};},[]);
  if(error)return <main><p role="alert">터미널 창의 세션을 확인하지 못했습니다. Workspace의 터미널 화면에서 다시 열어 주세요.</p></main>;
  if(!peer)return <p role="status">터미널 세션에 연결하고 있습니다…</p>;
  return <Suspense fallback={<p role="status">터미널을 불러오고 있습니다…</p>}><Terminal/></Suspense>;
}
