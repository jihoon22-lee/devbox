import { lazy, Suspense, useEffect, useState } from "react";
import { ProductShell, type ShellContentProps } from "@devbox/product-shell";

import { nativeMode } from "@devbox/product-shell/api";
import RegistryGate from "./RegistryGate";

const Overview = lazy(() => import("@devbox/workspace-features/overview"));
const Source = lazy(() => import("@devbox/workspace-features/source"));
const Files = lazy(() => import("@devbox/workspace-features/files"));

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
      <Suspense fallback={<p role="status">편집기를 불러오고 있습니다…</p>}><Files/></Suspense>
    </div>}
  </>;
}

export default function Workspace() {
  return <ProductShell product="workspace" renderContent={props => nativeMode ? <RegistryGate/> : <Content {...props}/>}/>;
}
