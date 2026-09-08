import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { ProductShell, type ShellContentProps } from "@devbox/product-shell";
import { Startup } from "./Startup";
import { localDateKey } from "./dates";
const LifecycleSettings = lazy(() => import("./LifecycleSettings"));
const MigrationSettings = lazy(() => import("./MigrationSettings"));
const Daily = lazy(() => import("./Daily"));
const Notes = lazy(() => import("@devbox/knowledge-features/notes"));
const Activity = lazy(() => import("@devbox/knowledge-features/activity"));
const Search = lazy(() => import("@devbox/knowledge-features/search"));
function Content({ route, navigate }: ShellContentProps) {
  const [showImport, setShowImport] = useState(false);
  const activateNotes = useCallback(() => { setShowImport(false); navigate("notes"); }, [navigate]);
  const activateDaily = useCallback(() => navigate("daily"), [navigate]);
  const activateActivity = useCallback(() => navigate("activity"), [navigate]);
  const [date, setDate] = useState(() => localDateKey());
  useEffect(() => setShowImport(false), [route]);
  const [openRequest, setOpenRequest] = useState<{ id: number; path: string }>();
  const openNote = useCallback((path: string) => {
    setOpenRequest(previous => ({ id: (previous?.id ?? 0) + 1, path }));
    navigate("notes");
  }, [navigate]);
  const group = ["activity", "search", "daily"].includes(route) ? route : "notes";
  const [visited, setVisited] = useState(() => new Set([group]));
  useEffect(() => { setVisited(previous => previous.has(group) ? previous : new Set([...previous, group])); }, [group]);
  return <>
    {group === "notes" && showImport && <div className="knowledge-settings-page"><button type="button" onClick={() => setShowImport(false)}>노트로 돌아가기</button><Suspense fallback={<p role="status">가져오기 설정을 불러오고 있습니다…</p>}><MigrationSettings/></Suspense></div>}
    {group === "daily" && <Suspense fallback={<p role="status">일일 기록을 불러오고 있습니다…</p>}><Daily date={date} onDateChange={setDate} onOpen={openNote} onActivity={activateActivity}/></Suspense>}
    {(visited.has("notes") || group === "notes") && <div className="knowledge-feature-notes" hidden={group !== "notes" || showImport}>
      <Suspense fallback={<p role="status">노트를 불러오고 있습니다…</p>}><Notes active={group === "notes" && !showImport} onActivate={activateNotes} onDaily={activateDaily} onImport={() => setShowImport(true)} openRequest={openRequest}/></Suspense>
    </div>}
    {(visited.has("activity") || group === "activity") && <div className="knowledge-feature-activity" hidden={group !== "activity"}>
      <Suspense fallback={<p role="status">활동을 불러오고 있습니다…</p>}><Activity active={group === "activity"} selectedDate={date} onDateChange={setDate} onDaily={activateDaily} onDraft={activateNotes} lifecycleSettings={<Suspense fallback={<p role="status">종료 설정을 불러오고 있습니다…</p>}><LifecycleSettings/></Suspense>}/></Suspense>
    </div>}
    {(visited.has("search") || group === "search") && <div className="knowledge-feature-search" hidden={group !== "search"}>
      <Suspense fallback={<p role="status">검색을 불러오고 있습니다…</p>}><Search onNoteOpen={activateNotes}/></Suspense>
    </div>}
  </>;
}
export default function Knowledge() {
  return <ProductShell product="knowledge" renderContent={props => <Startup><Content {...props}/></Startup>}/>;
}
