import { lazy, Suspense, useEffect, useState } from "react";
import { ProductShell, type ShellContentProps } from "@devbox/product-shell";
const Requests = lazy(() => import("@devbox/api-studio-features/requests"));
const Webhooks = lazy(() => import("@devbox/api-studio-features/webhooks"));
const Transforms = lazy(() => import("@devbox/api-studio-features/transforms"));

function Content({ route }: ShellContentProps) {
  const group = route === "webhooks" || route === "transforms" ? route : "requests";
  const [visited, setVisited] = useState(() => new Set([group]));
  const [apiSection, setApiSection] = useState<"requests" | "protocols" | "history">("requests");
  useEffect(() => {
    setVisited((previous) => previous.has(group) ? previous : new Set([...previous, group]));
    if (route === "requests" || route === "protocols" || route === "history") setApiSection(route);
  }, [group, route]);
  return <>
    {(visited.has("requests") || group === "requests") && <div className="api-feature-requests" hidden={group !== "requests"}>
      <Suspense fallback={<p role="status">요청 화면을 불러오고 있습니다…</p>}><Requests section={group === "requests" ? route as typeof apiSection : apiSection}/></Suspense>
    </div>}
    {(visited.has("webhooks") || group === "webhooks") && <div className="api-feature-webhooks" hidden={group !== "webhooks"}>
      <Suspense fallback={<p role="status">Webhook 화면을 불러오고 있습니다…</p>}><Webhooks/></Suspense>
    </div>}
    {(visited.has("transforms") || group === "transforms") && <div className="api-feature-transforms" hidden={group !== "transforms"}>
      <Suspense fallback={<p role="status">변환 도구를 불러오고 있습니다…</p>}><Transforms/></Suspense>
    </div>}
  </>;
}
export default function Studio() { return <ProductShell product="api-studio" renderContent={(props) => <Content {...props}/>}/>; }
