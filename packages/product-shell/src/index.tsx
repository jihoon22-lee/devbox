import { Component, lazy, Suspense, useEffect, useRef, useState, useCallback, type ReactNode } from "react";
import { isImeComposing } from "@devbox/a11y";
import { describe, nativeMode, type Description, type ProductId } from "./api";
import {IncomingReviewContext,type IncomingReview} from "./incoming";
import { navigate, traverse, type Navigation } from "./navigation";

const Operations = lazy(() => import("./Operations"));
const IncomingCommands = lazy(() => import("./IncomingCommands"));
const SuiteConnection = lazy(() => import("./SuiteConnection"));
const RouteView = lazy(() => import("./RouteView"));
class RouteBoundary extends Component<{ children: ReactNode }, { error: boolean }> {
  state = { error: false };
  static getDerivedStateFromError() { return { error: true }; }
  render() { return this.state.error ? <p role="alert">화면을 불러오지 못했습니다. 앱을 다시 열어 주세요.</p> : this.props.children; }
}

export interface ShellContentProps { description: Description; route: string; navigate: (route: string) => void; refreshContext: () => Promise<void> }
export type ShellContent = (props: ShellContentProps) => ReactNode;
function ReadyShell({ description, renderContent, refreshContext }: { description: Description; renderContent?: ShellContent; refreshContext: () => Promise<void> }) {
  const queryRoute = new URLSearchParams(location.search).get("route");
  const initial = description.features.some((f) => f.route === queryRoute) ? queryRoute! : description.product.defaultRoute;
  const [history, setHistory] = useState<Navigation>({ entries: [initial], cursor: 0 });
  const [visited, setVisited] = useState<Set<string>>(new Set([initial]));
  const [incomingReview,setIncomingReview]=useState<IncomingReview|null>(null);
  const clearIncomingReview=useCallback(()=>setIncomingReview(null),[]);
  const [connectionOpen, setConnectionOpen] = useState(false);
  const [operationsOpen,setOperationsOpen]=useState(false);
  const [online, setOnline] = useState(navigator.onLine);
  const content = useRef<HTMLElement>(null);
  const current = history.entries[history.cursor];
  useEffect(() => {
    const update = () => setOnline(navigator.onLine);
    window.addEventListener("online", update); window.addEventListener("offline", update);
    return () => { window.removeEventListener("online", update); window.removeEventListener("offline", update); };
  }, []);
  const open = useCallback((route: string) => {
    if (!description.features.some((feature) => feature.route === route)) return;
    setVisited((v) => new Set([...v, route])); setHistory((h) => navigate(h, route));
  }, [description]);
  return <IncomingReviewContext.Provider value={{review:incomingReview,clear:clearIncomingReview}}><div className="product-shell" onKeyDown={(event) => {
    if (isImeComposing(event)) return;
    if (event.altKey && (event.key === "ArrowLeft" || event.key === "ArrowRight")) {
      event.preventDefault(); setHistory((h) => traverse(h, event.key === "ArrowLeft" ? -1 : 1));
    }
  }}>
    <a className="shell-skip" href="#product-content" onClick={() => content.current?.focus()}>본문으로 이동</a>
    <header><strong>{description.product.label}</strong><span className="shell-badge">{nativeMode ? "개발 빌드" : "브라우저 미리보기 · 모의 데이터"}</span></header>
    <aside><nav aria-label="제품 화면">{description.features.map((feature) => <button key={feature.id} aria-current={current === feature.route ? "page" : undefined} onClick={() => open(feature.route)}>{feature.label}</button>)}</nav></aside>
    <main id="product-content" ref={content} tabIndex={-1}>
      <div className="shell-toolbar"><button aria-label="뒤로" disabled={history.cursor === 0} onClick={() => setHistory((h) => traverse(h, -1))}>←</button><button aria-label="앞으로" disabled={history.cursor === history.entries.length - 1} onClick={() => setHistory((h) => traverse(h, 1))}>→</button><span>{description.context ? "프로젝트 연결됨" : "프로젝트 선택 없이 사용"}</span><button aria-expanded={connectionOpen} onClick={() => setConnectionOpen(value => !value)}>제품 연결</button><button aria-expanded={operationsOpen} onClick={()=>setOperationsOpen(value=>!value)}>작업 상태</button></div>
      {nativeMode && <Suspense fallback={null}><IncomingCommands description={description} route={current} navigate={open} onReview={setIncomingReview}/></Suspense>}
      {operationsOpen && <Suspense fallback={<p role="status">작업 상태를 불러오고 있습니다…</p>}><Operations description={description} route={current}/></Suspense>}
      {connectionOpen && <Suspense fallback={<p role="status">연결 설정을 불러오고 있습니다…</p>}><SuiteConnection description={description} route={current}/></Suspense>}
      {!online && <p role="status">오프라인입니다. 로컬 화면은 계속 사용할 수 있습니다.</p>}
      {queryRoute && !description.features.some((f) => f.route === queryRoute) && <p role="status">요청한 화면이 없어 기본 화면을 열었습니다.</p>}
      {renderContent ? renderContent({ description, route: current, navigate: open, refreshContext }) : description.features.filter((f) => visited.has(f.route)).map((feature) => <div key={feature.id} hidden={feature.route !== current}><RouteBoundary><Suspense fallback={<p role="status">화면을 불러오고 있습니다…</p>}><RouteView description={description} feature={feature}/></Suspense></RouteBoundary></div>)}
    </main>
  </div></IncomingReviewContext.Provider>;
}

export function ProductShell({ product, renderContent }: { product: ProductId; renderContent?: ShellContent }) {
  const [description, setDescription] = useState<Description | null>(null);
  const [error, setError] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const loadId = useRef(0);
  useEffect(() => {
    let active = true;
    const id = ++loadId.current;
    setDescription(null); setError(false);
    void describe(product).then((value) => { if (active && id === loadId.current) setDescription(value); }, () => { if (active && id === loadId.current) setError(true); });
    return () => { active = false; loadId.current += 1; };
  }, [product, attempt]);
  const refreshContext = useCallback(async () => {
    const id = ++loadId.current;
    const next = await describe(product);
    if (id !== loadId.current) return;
    if (!description || next.handshake.sessionId !== description.handshake.sessionId
      || next.handshake.installationId !== description.handshake.installationId) throw new Error("제품 연결이 변경되었습니다. 앱을 다시 열어 주세요.");
    // Keep the mounted feature and its dirty buffers while refreshing only
    // native-owned description/context metadata.
    setDescription(next);
  }, [product, description]);
  if (error) return <main role="alert"><h1>제품 연결을 확인할 수 없습니다</h1><button onClick={() => setAttempt((v) => v + 1)}>다시 시도</button></main>;
  if (!description || description.product.id !== product) return <main><p role="status">제품을 열고 있습니다…</p></main>;
  return <ReadyShell key={product} description={description} renderContent={renderContent} refreshContext={refreshContext}/>;
}
