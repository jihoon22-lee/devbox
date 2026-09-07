import { useEffect, useState } from "react";
import { routeStatus, type Description, type Feature, type RouteStatus } from "./api";

export default function RouteView({ description, feature }: { description: Description; feature: Feature }) {
  const [status, setStatus] = useState<RouteStatus | null>(null);
  const [error, setError] = useState(false);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let active = true;
    setError(false);
    setStatus(null);
    void routeStatus(description, feature.route).then((value) => { if (active) setStatus(value); }, () => { if (active) setError(true); });
    return () => { active = false; };
  }, [description, feature.route, attempt]);
  return <section aria-label={feature.label}>
    <h1>{feature.label}</h1>
    {error ? <div role="alert"><p>화면 상태를 확인할 수 없습니다.</p><button onClick={() => setAttempt((a) => a + 1)}>다시 시도</button></div>
      : !status ? <p role="status">화면을 확인하고 있습니다…</p>
        : <div className="shell-empty"><h2>기능 이전을 준비하고 있습니다</h2><p>이 개발 빌드에는 제품 탐색 기능이 포함되어 있습니다. 기존 작업과 데이터는 기존 앱에서 계속 사용할 수 있습니다.</p></div>}
  </section>;
}
