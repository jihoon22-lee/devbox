import { lazy, Suspense, useEffect, useState } from "react";
import { ProductShell, type ShellContentProps } from "@devbox/product-shell";
import { listen } from "@tauri-apps/api/event";
import { nativeMode } from "@devbox/product-shell/api";
import { componentInvoke } from "@devbox/api-studio-features/transport";
const invokeNavigation = componentInvoke("api-studio.api");
const Requests = lazy(() => import("@devbox/api-studio-features/requests"));
const Webhooks = lazy(() => import("@devbox/api-studio-features/webhooks"));
const Transforms = lazy(() => import("@devbox/api-studio-features/transforms"));

function Content({ route, navigate }: ShellContentProps) {
  useEffect(() => {
    if (!nativeMode) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const consume = async () => {
      if (disposed) return;
      const pending = await invokeNavigation<unknown>("peek_pending_navigation");
      if (disposed || !pending || typeof pending !== "object") return;
      const value = pending as Record<string, unknown>;
      if (typeof value.id !== "string" || !/^[a-f0-9]{32}$/.test(value.id)
        || (value.route !== "requests" && value.route !== "transforms")) return;
      navigate(value.route);
      await invokeNavigation("ack_pending_navigation", { id: value.id });
    };
    const wake = () => { void consume().catch(() => undefined); };
    // Events only wake the reader. Peek+ack retains a cold delivery until a
    // mounted consumer handles it, including delayed listener registration.
    void listen<unknown>("api-studio://navigate", wake).then((stop) => {
      if (disposed) stop(); else { unlisten = stop; wake(); }
    }).catch(wake);
    return () => { disposed = true; unlisten?.(); };
  }, [navigate]);
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
