import { useEffect, useState } from "react";
import { routeStatus, type Description, type Feature, type RouteStatus } from "./api";

export default function RouteView({ description, feature }: { description: Description; feature: Feature }) {
  const [status, setStatus] = useState<RouteStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  // biome-ignore lint/correctness/useExhaustiveDependencies: existing dependency list; review in P1-15
  useEffect(() => {
    let active = true;
    setError(null);
    setStatus(null);
    void routeStatus(description, feature.route).then(
      (value) => {
        if (active) setStatus(value);
      },
      (failure: unknown) => {
        if (active) setError(failure instanceof Error ? failure.message : "화면 상태를 확인할 수 없습니다.");
      },
    );
    return () => {
      active = false;
    };
  }, [description, feature.route, attempt]);
  return (
    <section className="shell-route" aria-label={feature.label}>
      <h1>{feature.label}</h1>
      {error ? (
        <div role="alert">
          <p>{error}</p>
          <button onClick={() => setAttempt((a) => a + 1)}>다시 시도</button>
        </div>
      ) : !status ? (
        <p role="status">화면을 확인하고 있습니다…</p>
      ) : (
        <div className="shell-empty">
          <h2>이 화면은 아직 제공되지 않습니다</h2>
        </div>
      )}
    </section>
  );
}
