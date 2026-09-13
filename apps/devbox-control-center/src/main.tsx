import React, {lazy,Suspense,useState,useCallback,useEffect,useRef} from "react";
import ReactDOM from "react-dom/client";
import {ProductShell,type ShellContentProps} from "@devbox/product-shell";
import "./App.css";
import {onShortcut,triggerShortcut} from "@devbox/product-shell/commands";
import {nativeMode} from "@devbox/product-shell/api";
const HostedLauncher=lazy(()=>import("./HostedLauncher"));
const LauncherImport=lazy(()=>import("./LauncherImport"));
const Commands=lazy(()=>import("./Commands"));
const RouteView=lazy(()=>import("@devbox/product-shell/route-view"));
function Content(props:ShellContentProps) {
  const [launcher,setLauncher]=useState(false);
  const [shortcutIssue,setShortcutIssue]=useState("");
  const current=useRef(props);current.current=props;
  const shortcutBusy=useRef(false);
  const close=useCallback(()=>setLauncher(false),[]);
  useEffect(()=>{
    if(!nativeMode)return;
    let active=true,remove:(()=>void)|undefined,composing=false;
    const compositionStart=()=>{composing=true;},compositionEnd=()=>{composing=false;};
    document.addEventListener("compositionstart",compositionStart,true);
    document.addEventListener("compositionend",compositionEnd,true);
    void onShortcut(event=>{
      // Preserve an active modal and IME/editor focus. The global bindings never
      // register Ctrl+C, so Terminal SIGINT remains owned by Terminal.
      if(((event.wasFocused||event.command==="control-center.launcher")&&document.querySelector("dialog[open], [aria-modal=true]"))||(event.wasFocused&&composing))return;
      const focused=document.activeElement;
      if(event.wasFocused&&focused instanceof HTMLElement&&(focused.matches("input,textarea,[contenteditable=true]")||focused.closest(".xterm")))return;
      if(event.command==="control-center.launcher")setLauncher(true);
      else if(!shortcutBusy.current){
        shortcutBusy.current=true;setShortcutIssue("");const state=current.current;
        void triggerShortcut(state.description,state.route,event.command,crypto.randomUUID()).catch(()=>setShortcutIssue("단축키 작업을 확인하지 못했습니다. 연결한 제품의 실행 상태와 프로젝트·터미널을 확인해 주세요.")).finally(()=>{shortcutBusy.current=false;});
      }
    }).then(unlisten=>{if(active)remove=unlisten;else unlisten();});
    return()=>{active=false;remove?.();document.removeEventListener("compositionstart",compositionStart,true);document.removeEventListener("compositionend",compositionEnd,true);};
  },[]);
  const feature=props.description.features.find(feature=>feature.route===props.route);
  return <>{shortcutIssue&&<p role="alert">{shortcutIssue}</p>}<button onClick={()=>setLauncher(true)}>Launcher 열기</button>{launcher&&<Suspense fallback={<p role="status">Launcher를 불러오고 있습니다…</p>}><HostedLauncher {...props} close={close}/></Suspense>}<Suspense fallback={<p role="status">화면을 불러오고 있습니다…</p>}>{
    props.route==="products"?<Commands {...props}/>:props.route==="migration"?<LauncherImport {...props}/>:feature?<RouteView description={props.description} feature={feature}/>:null
  }</Suspense></>;
}
ReactDOM.createRoot(document.getElementById("root")!).render(<React.StrictMode><ProductShell product="control-center" renderContent={Content}/></React.StrictMode>);
