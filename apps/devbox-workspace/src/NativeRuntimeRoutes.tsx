import {lazy,Suspense,useEffect,useRef,useState} from "react";
import {listen} from "@tauri-apps/api/event";
import type {Description} from "@devbox/product-shell/api";
import type {RuntimeLogOpenRequest} from "@devbox/workspace-features/logs";
import {runtimeDestination,runtimeLogRequest,runtimeDiagnostic} from "./runtimeNavigation";
const Tasks=lazy(()=>import("@devbox/workspace-features/tasks"));
const Runtime=lazy(()=>import("@devbox/workspace-features/runtime"));
const Logs=lazy(()=>import("@devbox/workspace-features/logs"));
const RuntimeImport=lazy(()=>import("./RuntimeImport"));
const RuntimeSettingsImport=lazy(()=>import("./RuntimeSettingsImport"));
type Diagnostic={id:string;relativePath:string;line:number;column:number|null};
export default function NativeRuntimeRoutes({route,description,navigate,tasksDirty,onDirtyChange,onDiagnostic}:{route:string;description:Description;navigate:(route:string)=>void;tasksDirty:boolean;onDirtyChange:(dirty:boolean)=>void;onDiagnostic:(request:Diagnostic)=>void}) {
  const [engineVisited, setEngineVisited] = useState(() => new Set([route]));
  const [runtimeLogOpen, setRuntimeLogOpen] = useState<RuntimeLogOpenRequest | null>(null);
  const [runtimeNotice, setRuntimeNotice] = useState("");
  const [navigationReady,setNavigationReady]=useState(false);
  const [runtimeSettingsRevision,setRuntimeSettingsRevision]=useState(0);
  const [logSettingsRevision,setLogSettingsRevision]=useState(0);
  const current = useRef({route, context:description.context, navigate, tasksDirty, onDiagnostic});
  current.current = {route, context:description.context, navigate, tasksDirty, onDiagnostic};
  useEffect(() => { setEngineVisited(previous => previous.has(route) ? previous : new Set([...previous, route])); }, [route]);
  useEffect(() => {
    let disposed = false;
    const stops: Array<() => void> = [];
    const keep = (stop: () => void) => { if (disposed) stop(); else {stops.push(stop);if(stops.length===3)setNavigationReady(true);} };
    void listen<unknown>("workspace://runtime-navigate", event => {
      if (disposed) return;
      const state = current.current;
      const destination = runtimeDestination(event.payload, state.context, state.route);
      if (!destination) return;
      setRuntimeNotice(destination === "tasks" && state.tasksDirty ? "작성 중인 내용을 저장하거나 취소하면 요청한 작업으로 이동합니다." : "");
      state.navigate(destination);
    }).then(keep).catch(() => { if (!disposed) setRuntimeNotice("작업 이동 연결을 시작하지 못했습니다."); });
    void listen<unknown>("workspace://runtime-log", event => {
      if (disposed) return;
      const state = current.current;
      const request = runtimeLogRequest(event.payload, state.context, state.route);
      if (!request) return;
      setRuntimeNotice("");
      setRuntimeLogOpen(request);
      state.navigate("logs");
    }).then(keep).catch(() => { if (!disposed) setRuntimeNotice("실행 로그 연결을 시작하지 못했습니다."); });
    void listen<unknown>("workspace://runtime-diagnostic", event => { if (disposed) return; const state=current.current; const request=runtimeDiagnostic(event.payload,state.context,state.route); if(request)state.onDiagnostic(request); }).then(keep).catch(() => { if (!disposed) setRuntimeNotice("진단 위치 연결을 시작하지 못했습니다."); });
    return () => { disposed = true; stops.forEach(stop => stop()); };
  }, []);
  if(!navigationReady)return <p role="status">{runtimeNotice||"실행 화면 연결을 준비하고 있습니다…"}</p>;
  return <>
    {runtimeNotice && <p role="status">{runtimeNotice}</p>}
    {(engineVisited.has("tasks") || route === "tasks") && <div className="workspace-feature-tasks" hidden={route !== "tasks"} inert={route !== "tasks"}>
      <Suspense fallback={<p role="status">작업과 서비스를 불러오고 있습니다…</p>}><RuntimeImport description={description} active={route === "tasks"} blocked={tasksDirty}/><Tasks active={route === "tasks"} onDirtyChange={onDirtyChange}/></Suspense>
    </div>}
    {(engineVisited.has("runtime") || route === "runtime") && <div className="workspace-feature-runtime" hidden={route !== "runtime"} inert={route !== "runtime"}>
      <Suspense fallback={<p role="status">프로세스와 포트를 불러오고 있습니다…</p>}><RuntimeSettingsImport description={description} active={route === "runtime"} kind="runtime" onImported={()=>setRuntimeSettingsRevision(value=>value+1)}/><Runtime active={route === "runtime"} settingsRevision={runtimeSettingsRevision}/></Suspense>
    </div>}
    {(engineVisited.has("logs") || route === "logs") && <div className="workspace-feature-logs" hidden={route !== "logs"} inert={route !== "logs"}>
      <Suspense fallback={<p role="status">로그 화면을 불러오고 있습니다…</p>}><RuntimeSettingsImport description={description} active={route === "logs"} kind="logs" onImported={()=>setLogSettingsRevision(value=>value+1)}/><Logs settingsRevision={logSettingsRevision} active={route === "logs"} openRequest={runtimeLogOpen}/></Suspense>
    </div>}
  </>;
}
