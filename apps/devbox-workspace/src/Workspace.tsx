import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { ProductShell, type ShellContentProps } from "@devbox/product-shell";

import { nativeMode, type Description } from "@devbox/product-shell/api";
import { configureProductTransport } from "@devbox/workspace-features/transport";
import RegistryGate, { type Registry } from "./RegistryGate";
import { componentCall } from "./native";
import ProjectDefinitions from "./ProjectDefinitions";

const Overview = lazy(() => import("@devbox/workspace-features/overview"));
const Source = lazy(() => import("@devbox/workspace-features/source"));
const NativeSource = lazy(() => import("./Source"));
const Dependencies = lazy(() => import("@devbox/workspace-features/dependencies"));
const Files = lazy(() => import("@devbox/workspace-features/files"));
let displayed: Description | undefined;
let connected = false;

function NativeContent({route, description, refreshContext}: ShellContentProps) {
  displayed = description;
  if (!connected) {
    configureProductTransport(<T,>(component: string, method: string, args: Record<string, unknown>) => {
      const snapshot = displayed;
      if (!snapshot) return Promise.reject(new Error("제품 연결 정보를 확인하지 못했습니다."));
      const ownerRoute = component === "workspace.files" || component === "workspace.lsp" ? "files"
        : component === "workspace.source" ? "source" : component === "workspace.dependencies" ? "dependencies" : "overview";
      return componentCall<T>(snapshot, component, method, args, ownerRoute);
    });
    connected = true;
  }
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
  const [definitionsEditing, setDefinitionsEditing] = useState(false);
  const [registrySignal, setRegistrySignal] = useState(0);
  const refreshRegistry = useCallback(() => setRegistrySignal(value => value + 1), []);
  const [filesVisited, setFilesVisited] = useState(route === "files");
  const markReady = useCallback(() => setReady(true), []);
  useEffect(() => {if (route === "files") setFilesVisited(true);}, [route]);
  return <>
    <div hidden={ready && route === "files"}>
      <RegistryGate context={description.context} onContextChanged={refreshContext} onReady={markReady} editing={editing || definitionsEditing || dependenciesBusy || sourceBusy || sourceDirty} refreshSignal={registrySignal} onSnapshot={setRegistry}/>
    </div>
    {ready && description.context && <div hidden={route !== "overview"}>
      <ProjectDefinitions description={description} onDirtyChange={setDefinitionsEditing} onChanged={refreshRegistry}/>
    </div>}
    {ready && (sourceVisited || route === "source") && <div className="workspace-feature-source" hidden={route !== "source"}>
      {!selectedTree ? <p role="status">작업할 프로젝트를 선택해 주세요.</p> : <Suspense fallback={<p role="status">Source 화면을 불러오고 있습니다…</p>}>
        <NativeSource key={JSON.stringify(description.context)} description={description} root={selectedTree.binding.root} editorPending={editing} onBusyChange={setSourceBusy} onDirtyChange={setSourceDirty}/>
      </Suspense>}
    </div>}
    {ready && (dependenciesVisited || route === "dependencies") && <div className="workspace-feature-source" hidden={route !== "dependencies"}>
      {!selectedTree ? <p role="status">분석할 프로젝트를 선택해 주세요.</p> : <Suspense fallback={<p role="status">의존성 화면을 불러오고 있습니다…</p>}>
        <Dependencies repo={{path:selectedTree.binding.root, canonicalKey:JSON.stringify(description.context), hasWorktrees:false}} onBusyChange={setDependenciesBusy}/>
      </Suspense>}
    </div>}
    {ready && (filesVisited || route === "files") && <div className="workspace-feature-files" hidden={route !== "files"}>
      <Suspense fallback={<p role="status">편집기를 불러오고 있습니다…</p>}>
        <Files contextKey={JSON.stringify(description.context)} active={route === "files"} onDirtyChange={setEditing}/>
      </Suspense>
    </div>}
  </>;
}

function Content({ route, description }: ShellContentProps) {
  const group = route === "files" ? "files" : ["source", "dependencies"].includes(route) ? "source" : route === "overview" ? "overview" : "unavailable";
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
    {(visited.has("files") || group === "files") && <div className="workspace-feature-files" hidden={group !== "files"}>
      <Suspense fallback={<p role="status">편집기를 불러오고 있습니다…</p>}><Files active={group === "files"}/></Suspense>
    </div>}
  </>;
}

export default function Workspace() {
  return <ProductShell product="workspace" renderContent={props => nativeMode ? <NativeContent {...props}/> : <Content {...props}/>}/>;
}
