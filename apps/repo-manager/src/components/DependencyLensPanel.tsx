import { useEffect, useMemo, useRef, useState } from "react";
import {
  dependencyInventory,
  DEPENDENCY_LENS_ERROR,
  type DependencyEcosystem,
  type DependencyReport,
  type DependencySourceStatus,
  type RepoEntry,
} from "../api";

const ECOSYSTEM_LABEL: Record<DependencyEcosystem, string> = {
  cargo: "Cargo",
  pnpm: "pnpm",
  npm: "npm",
  python: "Python/uv",
  gradle: "Gradle",
};

const SOURCE_STATUS_LABEL: Record<DependencySourceStatus, string> = {
  ready: "정상",
  missingLockfile: "lockfile 없음",
  staleLockfile: "manifest가 lockfile보다 최신",
  invalid: "안전하게 해석할 수 없음",
  unsupported: "현재 형식 미지원",
};

const MAX_VISIBLE_PACKAGES = 300;
const MAX_VISIBLE_DUPLICATES = 300;
const MAX_VISIBLE_EDGES = 300;
const MAX_QUERY_LENGTH = 256;

export default function DependencyLensPanel({ repo }: { repo: RepoEntry | null }) {
  const [report, setReport] = useState<DependencyReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const requestSequence = useRef(0);

  useEffect(() => {
    requestSequence.current += 1;
    setReport(null);
    setError(null);
    setLoading(false);
    setQuery("");
    return () => {
      requestSequence.current += 1;
    };
  }, [repo?.canonicalKey]);

  const analyze = async () => {
    if (!repo || loading) return;
    const sequence = ++requestSequence.current;
    setLoading(true);
    setError(null);
    try {
      const result = await dependencyInventory(repo.path);
      if (sequence === requestSequence.current) setReport(result);
    } catch {
      if (sequence === requestSequence.current) {
        setReport(null);
        setError(DEPENDENCY_LENS_ERROR);
      }
    } finally {
      if (sequence === requestSequence.current) setLoading(false);
    }
  };

  const filteredPackages = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    const matches = report?.packages.filter((dependency) =>
      !normalized
      || dependency.name.toLowerCase().includes(normalized)
      || dependency.version.toLowerCase().includes(normalized)
      || ECOSYSTEM_LABEL[dependency.ecosystem].toLowerCase().includes(normalized),
    ) ?? [];
    return { total: matches.length, visible: matches.slice(0, MAX_VISIBLE_PACKAGES) };
  }, [query, report]);

  if (!repo) return null;

  return (
    <section className="dependency-lens-panel" aria-busy={loading}>
      <div className="dependency-lens-head">
        <div>
          <h2>Dependency Lens</h2>
          <p className="dim">로컬 lockfile만 읽으며 package manager, build script, 네트워크를 실행하지 않습니다.</p>
        </div>
        <button type="button" className="btn primary" disabled={loading} onClick={() => void analyze()}>
          {loading ? "분석 중…" : report ? "다시 분석" : "의존성 분석"}
        </button>
      </div>
      {loading && <div className="dependency-lens-status" role="status">bounded lock graph를 분석하고 있습니다…</div>}
      {error && <div className="error dependency-lens-error" role="alert">{error}</div>}
      {!loading && !report && !error && (
        <div className="dependency-lens-empty dim">선택한 repository의 Cargo, pnpm, npm, uv lockfile을 분석할 수 있습니다.</div>
      )}
      {report && (
        <>
          <dl className="dependency-lens-metrics">
            <div><dt>전체</dt><dd>{report.packageCount}</dd></div>
            <div><dt>직접</dt><dd>{report.directCount}</dd></div>
            <div><dt>전이</dt><dd>{report.transitiveCount}</dd></div>
            <div><dt>중복 이름</dt><dd>{report.duplicates.length}</dd></div>
            <div><dt>미해결 edge</dt><dd>{report.unresolvedDependencyCount}</dd></div>
          </dl>
          {(report.truncated || !report.summaryPublished) && (
            <div className="dependency-lens-warning" role="status">
              {report.truncated && <span>안전 상한에 도달해 일부 결과를 생략했습니다.</span>}
              {!report.summaryPublished && <span>Workbench용 요약 snapshot을 갱신하지 못했습니다.</span>}
            </div>
          )}

          <div className="dependency-lens-section">
            <h3>감지된 source</h3>
            {report.sources.length ? (
              <div className="dependency-source-list">
                {report.sources.map((source) => (
                  <div className={`dependency-source status-${source.status}`} key={`${source.ecosystem}:${source.path}`}>
                    <strong>{ECOSYSTEM_LABEL[source.ecosystem]}</strong>
                    <span className="mono">{source.path}</span>
                    <span>{SOURCE_STATUS_LABEL[source.status]}</span>
                    <span className="dim">{source.packageCount} packages · {source.directCount} direct</span>
                  </div>
                ))}
              </div>
            ) : <div className="dim">지원하는 manifest 또는 lockfile이 없습니다.</div>}
          </div>

          {report.duplicates.length > 0 && (
            <div className="dependency-lens-section">
              <h3>중복 버전</h3>
              <ul className="dependency-duplicates">
                {report.duplicates.slice(0, MAX_VISIBLE_DUPLICATES).map((duplicate) => (
                  <li key={`${duplicate.ecosystem}:${duplicate.name}`}>
                    <strong>{duplicate.name}</strong>
                    <span>{duplicate.versions.join(" · ")}</span>
                    <span className="dim">{ECOSYSTEM_LABEL[duplicate.ecosystem]}</span>
                  </li>
                ))}
              </ul>
              {report.duplicates.length > MAX_VISIBLE_DUPLICATES && (
                <div className="dim">중복 버전은 렌더링 상한으로 {MAX_VISIBLE_DUPLICATES}개만 표시합니다.</div>
              )}
            </div>
          )}

          <div className="dependency-lens-section">
            <div className="dependency-inventory-head">
              <h3>패키지 graph / inventory</h3>
              <label>
                <span className="sr-only">패키지 필터</span>
                <input
                  value={query}
                  onChange={(event) => setQuery(event.currentTarget.value)}
                  maxLength={MAX_QUERY_LENGTH}
                  placeholder="이름·버전·ecosystem 필터"
                />
              </label>
            </div>
            <div className="dependency-package-list" aria-label="Dependency package inventory">
              {filteredPackages.visible.map((dependency) => (
                <details className="dependency-package" key={dependency.id}>
                  <summary>
                    <span className="dependency-scope">{dependency.direct ? "직접" : "전이"}</span>
                    <strong>{dependency.name}</strong>
                    <span className="mono">{dependency.version}</span>
                    <span className="dim">{ECOSYSTEM_LABEL[dependency.ecosystem]} · edge {dependency.dependencies.length}</span>
                  </summary>
                  {dependency.dependencies.length > 0 ? (
                    <>
                      <ul>
                        {dependency.dependencies.slice(0, MAX_VISIBLE_EDGES).map((target) => (
                          <li className="mono" key={target}>{target}</li>
                        ))}
                      </ul>
                      {dependency.dependencies.length > MAX_VISIBLE_EDGES && (
                        <p className="dim">하위 edge는 렌더링 상한으로 {MAX_VISIBLE_EDGES}개만 표시합니다.</p>
                      )}
                    </>
                  ) : <p className="dim">해석된 하위 dependency가 없습니다.</p>}
                </details>
              ))}
              {filteredPackages.total === 0 && <div className="dim">조건에 맞는 package가 없습니다.</div>}
            </div>
            {filteredPackages.total > filteredPackages.visible.length && (
              <div className="dim">렌더링 상한으로 {filteredPackages.visible.length}개만 표시합니다. 필터를 좁혀 주세요.</div>
            )}
          </div>
        </>
      )}
    </section>
  );
}
