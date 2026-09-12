import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { ProductShell, type ShellContentProps } from "@devbox/product-shell";

import { nativeMode, type Description } from "@devbox/product-shell/api";
import { configureProductTransport } from "@devbox/workspace-features/transport";
import RegistryGate, { type Registry } from "./RegistryGate";
import { componentCall } from "./native";
import ProjectDefinitions from "./ProjectDefinitions";
import { listen } from "@tauri-apps/api/event";
import type { RuntimeLogOpenRequest } from "@devbox/workspace-features/logs";
import { runtimeDestination, runtimeLogRequest, runtimeDiagnostic } from "./runtimeNavigation";
import {sourceFilePath} from "./sourceNavigation";

const Overview = lazy(() => import("@devbox/workspace-features/overview"));
const Source = lazy(() => import("@devbox/workspace-features/source"));
const NativeSource = lazy(() => import("./Source"));
const Dependencies = lazy(() => import("@devbox/workspace-features/dependencies"));
const Files = lazy(() => import("@devbox/workspace-features/files"));
const RuntimeImport = lazy(() => import("./RuntimeImport"));
const Tasks = lazy(() => import("@devbox/workspace-features/tasks"));
const Runtime = lazy(() => import("@devbox/workspace-features/runtime"));
const Logs = lazy(() => import("@devbox/workspace-features/logs"));
const LegacyLspImport = lazy(() => import("./LegacyLspImport"));
const LegacyRecoveryImport = lazy(() => import("./LegacyRecoveryImport"));
const LegacySessionImport = lazy(() => import("./LegacySessionImport"));
let displayed: Description | undefined;
let connected = false;

function NativeContent({route, description, refreshContext, navigate}: ShellContentProps) {
  displayed = description;
  if (!connected) {
    configureProductTransport(<T,>(component: string, method: string, args: Record<string, unknown>) => {
      const snapshot = displayed;
      if (!snapshot) return Promise.reject(new Error("제품 연결 정보를 확인하지 못했습니다."));
      const ownerRoute = component === "workspace.files" || component === "workspace.lsp" ? "files"
        : component === "workspace.source" ? "source" : component === "workspace.dependencies" ? "dependencies"
        : component === "workspace.runtime" ? "tasks" : component === "workspace.logs" ? "logs"
        : component === "workspace.processes" || component === "workspace.process-actions" ? "runtime" : "overview";
      return componentCall<T>(snapshot, component, method, args, ownerRoute);
    }, description.handshake.installationId);
    connected = true;
  }
  const [tasksDirty, setTasksDirty] = useState(false);
  const [engineVisited, setEngineVisited] = useState(() => new Set([route]));
  const [runtimeLogOpen, setRuntimeLogOpen] = useState<RuntimeLogOpenRequest | null>(null);
  const [runtimeNotice, setRuntimeNotice] = useState("");
  const diagnosticOpen = useRef<(value: unknown) => void>(() => {});
  const current = useRef({route, context:description.context, navigate, tasksDirty});
  current.current = {route, context:description.context, navigate, tasksDirty};
  useEffect(() => { setEngineVisited(previous => previous.has(route) ? previous : new Set([...previous, route])); }, [route]);
  useEffect(() => {
    let disposed = false;
    const stops: Array<() => void> = [];
    const keep = (stop: () => void) => { if (disposed) stop(); else stops.push(stop); };
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
    void listen<unknown>("workspace://runtime-diagnostic", event => { if (!disposed) diagnosticOpen.current(event.payload); }).then(keep).catch(() => { if (!disposed) setRuntimeNotice("진단 위치 연결을 시작하지 못했습니다."); });
    return () => { disposed = true; stops.forEach(stop => stop()); };
  }, []);
  const [ready, setReady] = useState(false);
  const [registry, setRegistry] = useState<Registry | null>(null);
  const [dependenciesBusy, setDependenciesBusy] = useState(false);
  const [sourceBusy, setSourceBusy] = useState(false);
  const [sourceDirty, setSourceDirty] = useState(false);
  const [sourceVisited, setSourceVisited] = useState(route === "source");
  useEffect(() => {if (route === "source") setSourceVisited(true);}, [route]);
  const [dependenciesVisited, setDependenciesVisited] = useState(route === "dependencies");
  useEffect(() => {if (route === "dependencies") setDependenciesVisited(true);}, [route]);
  const selectedTree = registry?.worktrees.find(tree => tree.projectId === description.context?.projectId && tree.id === description.context.worktreeId && tree.revision === description.context.revision);
  const [editing, setEditing] = useState(false);
  const [sessionImportBusy,setSessionImportBusy]=useState(false);
  const [recoveryImportBusy,setRecoveryImportBusy]=useState(false);
  const [lspImportBusy,setLspImportBusy]=useState(false);
  const [sessionRevision,setSessionRevision]=useState(0);
  const [definitionsEditing, setDefinitionsEditing] = useState(false);
  const [registrySignal, setRegistrySignal] = useState(0);
  const refreshRegistry = useCallback(() => setRegistrySignal(value => value + 1), []);
  const [fileRequest, setFileRequest] = useState<{id:string;contextKey:string;path:string;line:number|null;column?:number|null}|null>(null);
  const reloadImportedSession=useCallback(()=>{setFileRequest(null);setSessionRevision(value=>value+1);},[]);
  const [sourceNavigationError, setSourceNavigationError] = useState("");
  const [registrationRequest,setRegistrationRequest]=useState<{id:string;path:string;name:string;target:NonNullable<Description["context"]>["target"]}|null>(null);
  const proposeWorktree = (path:string) => {
    if (!description.context) return;
    setRegistrationRequest({id:crypto.randomUUID(),path,target:description.context.target,name:registry?.projects.find(project=>project.id===description.context?.projectId)?.name??""});
    navigate("source");
  };
  const openSourceFile = (relative:string,line:number|null) => {
    if (!selectedTree) return;
    const path = sourceFilePath(selectedTree.binding.root,relative);
    if (!path || (line !== null && (!Number.isSafeInteger(line) || line < 1))) {
      setSourceNavigationError("Git에서 받은 파일 경로 또는 위치를 열 수 없습니다."); return;
    }
    setSourceNavigationError("");
    setFileRequest({id:crypto.randomUUID(),contextKey:JSON.stringify(description.context),path,line});
    setFilesVisited(true); navigate("files");
  };
  const [filesVisited, setFilesVisited] = useState(route === "files");
  diagnosticOpen.current = value => {
    const request = runtimeDiagnostic(value, description.context, route);
    if (!request || !selectedTree) return;
    const path = sourceFilePath(selectedTree.binding.root, request.relativePath);
    if (!path) return;
    setFileRequest({id:request.id,contextKey:JSON.stringify(description.context),path,line:request.line,column:request.column});
    setFilesVisited(true); navigate("files");
  };
  const markReady = useCallback(() => setReady(true), []);
  useEffect(() => {if (route === "files") setFilesVisited(true);}, [route]);
  return <>
    <div hidden={ready && route === "files"}>
      <RegistryGate context={description.context} onContextChanged={refreshContext} onReady={markReady} editing={tasksDirty || editing || sessionImportBusy || recoveryImportBusy || lspImportBusy || definitionsEditing || dependenciesBusy || sourceBusy || sourceDirty} refreshSignal={registrySignal} onSnapshot={setRegistry} suggestedRoot={registrationRequest}/>
    </div>
    {runtimeNotice && <p role="status">{runtimeNotice}</p>}
    {ready && description.context && <div hidden={route !== "overview"}>
      <ProjectDefinitions description={description} onDirtyChange={setDefinitionsEditing} onChanged={refreshRegistry}/>
    </div>}
    {ready && (sourceVisited || route === "source") && <div className="workspace-feature-source" hidden={route !== "source"}>
      {sourceNavigationError && <p role="alert">{sourceNavigationError}</p>}
      {!selectedTree ? <p role="status">작업할 프로젝트를 선택해 주세요.</p> : <Suspense fallback={<p role="status">Source 화면을 불러오고 있습니다…</p>}>
        <NativeSource key={JSON.stringify(description.context)} description={description} root={selectedTree.binding.root} editorPending={editing} onBusyChange={setSourceBusy} onDirtyChange={setSourceDirty} onOpenFile={openSourceFile} onProposeWorktree={proposeWorktree}/>
      </Suspense>}
    </div>}
    {ready && (dependenciesVisited || route === "dependencies") && <div className="workspace-feature-source" hidden={route !== "dependencies"}>
      {!selectedTree ? <p role="status">분석할 프로젝트를 선택해 주세요.</p> : <Suspense fallback={<p role="status">의존성 화면을 불러오고 있습니다…</p>}>
        <Dependencies repo={{path:selectedTree.binding.root, canonicalKey:JSON.stringify(description.context), hasWorktrees:false}} onBusyChange={setDependenciesBusy}/>
      </Suspense>}
    </div>}
    {ready && (engineVisited.has("tasks") || route === "tasks") && <div className="workspace-feature-tasks" hidden={route !== "tasks"} inert={route !== "tasks"}>
      <Suspense fallback={<p role="status">작업과 서비스를 불러오고 있습니다…</p>}><RuntimeImport description={description} active={route === "tasks"} blocked={tasksDirty}/><Tasks active={route === "tasks"} onDirtyChange={setTasksDirty}/></Suspense>
    </div>}
    {ready && (engineVisited.has("runtime") || route === "runtime") && <div className="workspace-feature-runtime" hidden={route !== "runtime"} inert={route !== "runtime"}>
      <Suspense fallback={<p role="status">프로세스와 포트를 불러오고 있습니다…</p>}><Runtime active={route === "runtime"}/></Suspense>
    </div>}
    {ready && (engineVisited.has("logs") || route === "logs") && <div className="workspace-feature-logs" hidden={route !== "logs"} inert={route !== "logs"}>
      <Suspense fallback={<p role="status">로그 화면을 불러오고 있습니다…</p>}><Logs active={route === "logs"} openRequest={runtimeLogOpen}/></Suspense>
    </div>}
    {ready && (filesVisited || route === "files") && <div className="workspace-feature-files" hidden={route !== "files"}>
      <Suspense fallback={<p role="status">편집기를 불러오고 있습니다…</p>}>
        <LegacySessionImport key={JSON.stringify(description.context)} description={description} disabled={editing||recoveryImportBusy||lspImportBusy} onBusyChange={setSessionImportBusy} onApplied={reloadImportedSession}/>
        <LegacyRecoveryImport key={JSON.stringify(description.context)} description={description} disabled={editing||sessionImportBusy||lspImportBusy} onBusyChange={setRecoveryImportBusy} onApplied={reloadImportedSession}/>
        <LegacyLspImport key={JSON.stringify(description.context)} description={description} disabled={editing||sessionImportBusy||recoveryImportBusy} onBusyChange={setLspImportBusy} onApplied={reloadImportedSession}/>
        <div inert={sessionImportBusy||recoveryImportBusy||lspImportBusy}>
          <Files key={sessionRevision} contextKey={JSON.stringify(description.context)} active={route === "files"&&!sessionImportBusy&&!recoveryImportBusy&&!lspImportBusy} onDirtyChange={setEditing} openRequest={fileRequest}/>
        </div>
      </Suspense>
    </div>}
  </>;
}

function Content({ route, description }: ShellContentProps) {
  const group = ["tasks", "runtime", "logs"].includes(route) ? route : route === "files" ? "files" : ["source", "dependencies"].includes(route) ? "source" : route === "overview" ? "overview" : "unavailable";
  const [visited, setVisited] = useState(() => new Set([group]));
  useEffect(() => {
    setVisited(previous => previous.has(group) ? previous : new Set([...previous, group]));
  }, [group]);
  return <>
    {group === "unavailable" && <section><h1>{description.features.find(feature => feature.route === route)?.label}</h1><p role="status">이 화면의 기능 연결을 준비하고 있습니다.</p></section>}
    {(visited.has("overview") || group === "overview") && <div className="workspace-feature-overview" hidden={group !== "overview"}>
      <Suspense fallback={<p role="status">프로젝트를 불러오고 있습니다…</p>}><Overview/></Suspense>
    </div>}
    {(visited.has("source") || group === "source") && <div className="workspace-feature-source" hidden={group !== "source"}>
      <Suspense fallback={<p role="status">저장소를 불러오고 있습니다…</p>}><Source/></Suspense>
    </div>}
    {(visited.has("tasks") || group === "tasks") && <div className="workspace-feature-tasks" hidden={group !== "tasks"} inert={group !== "tasks"}>
      <Suspense fallback={<p role="status">작업과 서비스를 불러오고 있습니다…</p>}><Tasks active={group === "tasks"}/></Suspense>
    </div>}
    {(visited.has("runtime") || group === "runtime") && <div className="workspace-feature-runtime" hidden={group !== "runtime"} inert={group !== "runtime"}>
      <Suspense fallback={<p role="status">프로세스와 포트를 불러오고 있습니다…</p>}><Runtime active={group === "runtime"}/></Suspense>
    </div>}
    {(visited.has("logs") || group === "logs") && <div className="workspace-feature-logs" hidden={group !== "logs"} inert={group !== "logs"}>
      <Suspense fallback={<p role="status">로그 화면을 불러오고 있습니다…</p>}><Logs active={group === "logs"}/></Suspense>
    </div>}
    {(visited.has("files") || group === "files") && <div className="workspace-feature-files" hidden={group !== "files"}>
      <Suspense fallback={<p role="status">편집기를 불러오고 있습니다…</p>}><Files active={group === "files"}/></Suspense>
    </div>}
  </>;
}

export default function Workspace() {
  return <ProductShell product="workspace" renderContent={props => nativeMode ? <NativeContent {...props}/> : <Content {...props}/>}/>;
}
