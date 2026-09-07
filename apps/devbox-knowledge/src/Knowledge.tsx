import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { ProductShell, type ShellContentProps } from "@devbox/product-shell";
import { Startup } from "./Startup";
const Notes = lazy(() => import("@devbox/knowledge-features/notes"));
const Activity = lazy(() => import("@devbox/knowledge-features/activity"));
const Search = lazy(() => import("@devbox/knowledge-features/search"));
function Content({ route, navigate }: ShellContentProps) {
  const activateNotes = useCallback(() => navigate("notes"), [navigate]);
  const group = route === "activity" || route === "search" ? route : "notes";
  const [visited, setVisited] = useState(() => new Set([group]));
  useEffect(() => { setVisited(previous => previous.has(group) ? previous : new Set([...previous, group])); }, [group]);
  return <>
    {(visited.has("notes") || group === "notes") && <div className="knowledge-feature-notes" hidden={group !== "notes"}>
      <Suspense fallback={<p role="status">노트를 불러오고 있습니다…</p>}><Notes active={group === "notes"} onActivate={activateNotes}/></Suspense>
    </div>}
    {(visited.has("activity") || group === "activity") && <div className="knowledge-feature-activity" hidden={group !== "activity"}>
      <Suspense fallback={<p role="status">활동을 불러오고 있습니다…</p>}><Activity active={group === "activity"}/></Suspense>
    </div>}
    {(visited.has("search") || group === "search") && <div className="knowledge-feature-search" hidden={group !== "search"}>
      <Suspense fallback={<p role="status">검색을 불러오고 있습니다…</p>}><Search/></Suspense>
    </div>}
  </>;
}
export default function Knowledge() {
  return <ProductShell product="knowledge" renderContent={props => <Startup><Content {...props}/></Startup>}/>;
}
