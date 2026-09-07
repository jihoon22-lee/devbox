import { Component, lazy, Suspense, useEffect, useRef, useState, type ReactNode } from "react";
import { isImeComposing } from "@devbox/a11y";
import { describe, nativeMode, type Description, type ProductId } from "./api";
import { navigate, traverse, type Navigation } from "./navigation";

const RouteView = lazy(() => import("./RouteView"));
class RouteBoundary extends Component<{ children: ReactNode }, { error: boolean }> {
  state = { error: false };
  static getDerivedStateFromError() { return { error: true }; }
  render() { return this.state.error ? <p role="alert">화면을 불러오지 못했습니다. 앱을 다시 열어 주세요.</p> : this.props.children; }
}

function ReadyShell({ description }: { description: Description }) {
  const queryRoute = new URLSearchParams(location.search).get("route");
  const initial = description.features.some((f) => f.route === queryRoute) ? queryRoute! : description.product.defaultRoute;
  const [history, setHistory] = useState<Navigation>({ entries: [initial], cursor: 0 });
  const [visited, setVisited] = useState<Set<string>>(new Set([initial]));
  const [online, setOnline] = useState(navigator.onLine);
  const content = useRef<HTMLElement>(null);
  const current = history.entries[history.cursor];
  useEffect(() => {
    const update = () => setOnline(navigator.onLine);
    window.addEventListener("online", update); window.addEventListener("offline", update);
    return () => { window.removeEventListener("online", update); window.removeEventListener("offline", update); };
  }, []);
  function open(route: string) {
    setVisited((v) => new Set([...v, route])); setHistory((h) => navigate(h, route));
  }
  return <div className="product-shell" onKeyDown={(event) => {
    if (isImeComposing(event)) return;
    if (event.altKey && (event.key === "ArrowLeft" || event.key === "ArrowRight")) {
      event.preventDefault(); setHistory((h) => traverse(h, event.key === "ArrowLeft" ? -1 : 1));
    }
  }}>
    <a className="shell-skip" href="#product-content" onClick={() => content.current?.focus()}>본문으로 이동</a>
    <header><strong>{description.product.label}</strong><span className="shell-badge">{nativeMode ? "개발 빌드" : "브라우저 미리보기 · 모의 데이터"}</span></header>
    <aside><nav aria-label="제품 화면">{description.features.map((feature) => <button key={feature.id} aria-current={current === feature.route ? "page" : undefined} onClick={() => open(feature.route)}>{feature.label}</button>)}</nav></aside>
    <main id="product-content" ref={content} tabIndex={-1}>
      <div className="shell-toolbar"><button aria-label="뒤로" disabled={history.cursor === 0} onClick={() => setHistory((h) => traverse(h, -1))}>←</button><button aria-label="앞으로" disabled={history.cursor === history.entries.length - 1} onClick={() => setHistory((h) => traverse(h, 1))}>→</button><span>프로젝트 선택 없이 사용</span></div>
      {!online && <p role="status">오프라인입니다. 로컬 화면은 계속 사용할 수 있습니다.</p>}
      {queryRoute && !description.features.some((f) => f.route === queryRoute) && <p role="status">요청한 화면이 없어 기본 화면을 열었습니다.</p>}
      {description.features.filter((f) => visited.has(f.route)).map((feature) => <div key={feature.id} hidden={feature.route !== current}><RouteBoundary><Suspense fallback={<p role="status">화면을 불러오고 있습니다…</p>}><RouteView description={description} feature={feature}/></Suspense></RouteBoundary></div>)}
    </main>
  </div>;
}

export function ProductShell({ product }: { product: ProductId }) {
  const [description, setDescription] = useState<Description | null>(null);
  const [error, setError] = useState(false);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let active = true;
    setDescription(null); setError(false);
    void describe(product).then((value) => { if (active) setDescription(value); }, () => { if (active) setError(true); });
    return () => { active = false; };
  }, [product, attempt]);
  if (error) return <main role="alert"><h1>제품 연결을 확인할 수 없습니다</h1><button onClick={() => setAttempt((v) => v + 1)}>다시 시도</button></main>;
  if (!description || description.product.id !== product) return <main><p role="status">제품을 열고 있습니다…</p></main>;
  return <ReadyShell key={product} description={description}/>;
}
