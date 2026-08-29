import { useCallback, useEffect, useRef, useState } from "react";
import {
  packageDependencySummary,
  type PackageDependencyStatus,
  type PackageDependencySummary,
} from "../api";
import { formatRuntimeFreshness } from "../lib/runtimeSuggestions";

const STATUS_LABEL: Record<PackageDependencyStatus, string> = {
  fresh: "최신 요약",
  stale: "오래된 요약",
  expired: "만료된 요약",
  missing: "요약 없음",
  corrupt: "안전하게 읽을 수 없음",
};

const ECOSYSTEM_LABEL: Record<string, string> = {
  cargo: "Cargo",
  pnpm: "pnpm",
  npm: "npm",
  python: "Python / uv",
  gradle: "Gradle",
};

interface Props {
  profileId: string;
}

export default function PackageDependencySummaryPanel({ profileId }: Props) {
  const [summary, setSummary] = useState<PackageDependencySummary | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestSequence = useRef(0);

  const refresh = useCallback(() => {
    const request = ++requestSequence.current;
    setLoading(true);
    setError(null);
    setSummary(null);
    void packageDependencySummary(profileId)
      .then((result) => {
        if (request === requestSequence.current && result.profileId === profileId) {
          setSummary(result);
        }
      })
      .catch(() => {
        if (request === requestSequence.current) {
          setError("패키지 의존성 요약을 불러올 수 없습니다.");
        }
      })
      .finally(() => {
        if (request === requestSequence.current) setLoading(false);
      });
  }, [profileId]);

  useEffect(() => {
    refresh();
    return () => {
      requestSequence.current += 1;
    };
  }, [refresh]);

  const available = summary && !["missing", "corrupt"].includes(summary.status);
  const sourceDetail = summary?.producerVersion && summary.freshnessMs !== null
    ? `${summary.source} · producer ${summary.producerVersion} · ${formatRuntimeFreshness(summary.freshnessMs)}`
    : summary?.source;

  return (
    <section
      className="package-dependency-panel"
      aria-labelledby="package-dependency-title"
      aria-busy={loading}
    >
      <div className="dependency-health-heading">
        <h4 id="package-dependency-title" className="dependency-subtitle">Packages</h4>
        <button type="button" className="btn" disabled={loading} onClick={refresh}>
          {loading ? "불러오는 중…" : "패키지 요약 새로고침"}
        </button>
      </div>
      <p className="field-help">
        Repo Manager가 로컬 lockfile에서 게시한 집계만 읽습니다. Workbench는 package manager나 build를 실행하지 않습니다.
      </p>

      {loading && <div className="dim" role="status">패키지 의존성 요약을 확인하는 중…</div>}
      {error && <div className="field-error" role="alert">{error}</div>}
      {summary && (
        <>
          <div className={`package-summary-status status-${summary.status}`}>
            <strong>{STATUS_LABEL[summary.status]}</strong>
            {sourceDetail && <span>{sourceDetail}</span>}
          </div>
          {summary.status === "missing" && (
            <p className="dim">Repo Manager에서 이 프로젝트의 ‘의존성 분석’을 실행하면 요약이 표시됩니다.</p>
          )}
          {summary.status === "corrupt" && (
            <p className="field-error">Repo Manager snapshot 계약을 확인한 뒤 다시 분석하세요.</p>
          )}
          {available && (
            <>
              <div className="package-summary-metrics" aria-label="Package dependency 집계">
                <span><strong>{summary.packageCount}</strong> 전체</span>
                <span><strong>{summary.directCount}</strong> 직접</span>
                <span><strong>{summary.transitiveCount}</strong> 전이</span>
                <span><strong>{summary.duplicateCount}</strong> 중복 버전</span>
              </div>
              <div className="package-summary-signals">
                <span>미해결 edge {summary.unresolvedDependencyCount}</span>
                <span>lockfile 없음 {summary.missingLockfileCount}</span>
                <span>lockfile 오래됨 {summary.staleLockfileCount}</span>
                <span>미지원 {summary.unsupportedCount}</span>
                <span>해석 오류 {summary.invalidCount}</span>
              </div>
              {summary.ecosystems.length > 0 && (
                <div className="package-ecosystem-list" aria-label="Ecosystem별 package 집계">
                  {summary.ecosystems.map((ecosystem) => (
                    <div key={ecosystem.ecosystem}>
                      <strong>{ECOSYSTEM_LABEL[ecosystem.ecosystem] ?? ecosystem.ecosystem}</strong>
                      <span>
                        전체 {ecosystem.packageCount} · 직접 {ecosystem.directCount} · 중복 {ecosystem.duplicateCount}
                      </span>
                    </div>
                  ))}
                </div>
              )}
              {summary.truncated && (
                <p className="field-error">안전 한도에 도달해 일부 package 또는 edge가 생략되었습니다.</p>
              )}
            </>
          )}
        </>
      )}
    </section>
  );
}
