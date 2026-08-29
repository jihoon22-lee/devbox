import { useEffect, useMemo, useRef, useState } from "react";
import {
  dependencyEnrichmentExecute,
  dependencyEnrichmentPreview,
  dependencyInventory,
  DEPENDENCY_ENRICHMENT_BUSY,
  DEPENDENCY_ENRICHMENT_ERROR,
  DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED,
  DEPENDENCY_LENS_ERROR,
  type DependencyEcosystem,
  type DependencyEnrichmentEntry,
  type DependencyEnrichmentPreview,
  type DependencyEnrichmentReport,
  type DependencyReport,
  type DependencySourceStatus,
  type EnrichmentSelection,
  type EnrichmentService,
  type EnrichmentValueState,
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

const SERVICE_LABEL: Record<EnrichmentService, string> = {
  osv: "OSV",
  depsDev: "deps.dev",
};

const VALUE_STATE_LABEL: Record<EnrichmentValueState, string> = {
  fresh: "방금 조회",
  cached: "캐시",
  stale: "오래된 캐시",
  failed: "조회 실패",
  notRequested: "선택 안 함",
};

const DEFAULT_SELECTION: EnrichmentSelection = { osv: true, depsDev: true };
const MAX_VISIBLE_PACKAGES = 300;
const MAX_VISIBLE_DUPLICATES = 300;
const MAX_VISIBLE_EDGES = 300;
const MAX_QUERY_LENGTH = 256;

type RemoteActivity = "preview" | "execute" | null;

function fixedRemoteError(error: unknown): string {
  const message = typeof error === "string" ? error : error instanceof Error ? error.message : "";
  if (message === DEPENDENCY_ENRICHMENT_BUSY) return DEPENDENCY_ENRICHMENT_BUSY;
  if (message === DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED) return DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED;
  return DEPENDENCY_ENRICHMENT_ERROR;
}

function remoteStateClass(state: EnrichmentValueState): string {
  return `state-${state}`;
}

function formatAge(ageMs: number | null): string | null {
  if (ageMs === null) return null;
  const minutes = Math.floor(ageMs / 60_000);
  if (minutes < 1) return "1분 미만";
  if (minutes < 60) return `${minutes}분 전`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}시간 전`;
  return `${Math.floor(hours / 24)}일 전`;
}

function RemotePackageMetadata({ entry }: { entry: DependencyEnrichmentEntry }) {
  const osvAge = formatAge(entry.osv.ageMs);
  const depsAge = formatAge(entry.depsDev.ageMs);
  return (
    <div className="dependency-remote-package" aria-label="원격 보강 정보">
      {entry.osv.state !== "notRequested" && (
        <div className="dependency-remote-value">
          <div>
            <strong>OSV</strong>
            <span className={`dependency-remote-state ${remoteStateClass(entry.osv.state)}`}>
              {VALUE_STATE_LABEL[entry.osv.state]}{osvAge ? ` · ${osvAge}` : ""}
            </span>
          </div>
          {entry.osv.state !== "failed" && (
            entry.osv.advisoryIds.length > 0
              ? <p>Advisory: <span className="mono">{entry.osv.advisoryIds.join(" · ")}</span></p>
              : <p className="dim">조회 시점에 반환된 advisory가 없습니다.</p>
          )}
          {entry.osv.truncated && <p className="dependency-lens-warning">OSV 결과에 다음 페이지가 있어 일부만 표시합니다.</p>}
        </div>
      )}
      {entry.depsDev.state !== "notRequested" && (
        <div className="dependency-remote-value">
          <div>
            <strong>deps.dev</strong>
            <span className={`dependency-remote-state ${remoteStateClass(entry.depsDev.state)}`}>
              {VALUE_STATE_LABEL[entry.depsDev.state]}{depsAge ? ` · ${depsAge}` : ""}
            </span>
          </div>
          {entry.depsDev.state !== "failed" && (
            <>
              <p>라이선스(참고용): {entry.depsDev.licenses.length ? entry.depsDev.licenses.join(" · ") : "확인되지 않음"}</p>
              <p>서비스 기본 버전: <span className="mono">{entry.depsDev.defaultVersion ?? "확인되지 않음"}</span> <span className="dim">(안전한 업데이트 보장 아님)</span></p>
              {entry.depsDev.deprecated && <p className="dependency-remote-deprecated">deps.dev에서 deprecated로 표시했습니다.</p>}
              {entry.depsDev.advisoryIds.length > 0 && <p>Advisory: <span className="mono">{entry.depsDev.advisoryIds.join(" · ")}</span></p>}
              {(!entry.depsDev.versionFound || !entry.depsDev.packageFound) && (
                <p className="dim">서비스에 {!entry.depsDev.versionFound ? "해당 버전" : "패키지 기본 버전 정보"}가 없습니다.</p>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}

export default function DependencyLensPanel({ repo }: { repo: RepoEntry | null }) {
  const [report, setReport] = useState<DependencyReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [selection, setSelection] = useState<EnrichmentSelection>(DEFAULT_SELECTION);
  const [forceRefresh, setForceRefresh] = useState(false);
  const [preview, setPreview] = useState<DependencyEnrichmentPreview | null>(null);
  const [enrichment, setEnrichment] = useState<DependencyEnrichmentReport | null>(null);
  const [remoteActivity, setRemoteActivity] = useState<RemoteActivity>(null);
  const [remoteError, setRemoteError] = useState<string | null>(null);
  const requestSequence = useRef(0);
  const busy = loading || remoteActivity !== null;

  useEffect(() => {
    requestSequence.current += 1;
    setReport(null);
    setError(null);
    setLoading(false);
    setQuery("");
    setForceRefresh(false);
    setPreview(null);
    setEnrichment(null);
    setRemoteActivity(null);
    setRemoteError(null);
    return () => {
      requestSequence.current += 1;
    };
  }, [repo?.canonicalKey]);

  const resetRemote = () => {
    requestSequence.current += 1;
    setPreview(null);
    setEnrichment(null);
    setRemoteError(null);
  };

  const analyze = async () => {
    if (!repo || busy) return;
    const sequence = ++requestSequence.current;
    setLoading(true);
    setError(null);
    setPreview(null);
    setEnrichment(null);
    setRemoteError(null);
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

  const reviewTransmission = async () => {
    if (!repo || !report || busy || (!selection.osv && !selection.depsDev)) return;
    const sequence = ++requestSequence.current;
    setRemoteActivity("preview");
    setPreview(null);
    setEnrichment(null);
    setRemoteError(null);
    try {
      const result = await dependencyEnrichmentPreview(repo.path, selection, forceRefresh);
      if (sequence !== requestSequence.current) return;
      if (result.revision !== report.revision) {
        setRemoteError(DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED);
        return;
      }
      setPreview(result);
    } catch (caught) {
      if (sequence === requestSequence.current) setRemoteError(fixedRemoteError(caught));
    } finally {
      if (sequence === requestSequence.current) setRemoteActivity(null);
    }
  };

  const executeEnrichment = async () => {
    if (!repo || !report || !preview || busy) return;
    const reviewed = preview;
    const sequence = ++requestSequence.current;
    setRemoteActivity("execute");
    setRemoteError(null);
    try {
      const result = await dependencyEnrichmentExecute(repo.path, reviewed.token);
      if (sequence !== requestSequence.current) return;
      if (result.revision !== report.revision || result.revision !== reviewed.revision) {
        setRemoteError(DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED);
        return;
      }
      setEnrichment(result);
    } catch (caught) {
      if (sequence === requestSequence.current) setRemoteError(fixedRemoteError(caught));
    } finally {
      if (sequence === requestSequence.current) {
        setPreview(null);
        setRemoteActivity(null);
      }
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

  const enrichmentByPackage = useMemo(() => {
    const byPackage = new Map<string, DependencyEnrichmentEntry>();
    if (!enrichment || !report || enrichment.revision !== report.revision) return byPackage;
    for (const entry of enrichment.entries) {
      for (const packageId of entry.packageIds) byPackage.set(packageId, entry);
    }
    return byPackage;
  }, [enrichment, report?.revision]);

  const serviceTransmissionCount = preview?.services.reduce(
    (total, service) => total + service.transmitted.length,
    0,
  ) ?? 0;

  if (!repo) return null;

  return (
    <section className="dependency-lens-panel" aria-busy={busy}>
      <div className="dependency-lens-head">
        <div>
          <h2>Dependency Lens</h2>
          <p className="dim">기본 분석은 로컬 lockfile만 읽습니다. 원격 보강은 전송 내용을 검토하고 승인한 경우에만 실행됩니다.</p>
        </div>
        <button type="button" className="btn primary" disabled={busy} onClick={() => void analyze()}>
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

          <div className="dependency-lens-section dependency-enrichment-section">
            <div className="dependency-enrichment-head">
              <div>
                <h3>선택적 원격 보강</h3>
                <p className="dim">자동 전송하지 않습니다. 서비스와 정확한 package 좌표를 먼저 검토합니다.</p>
              </div>
              <button
                type="button"
                className="btn"
                disabled={busy || (!selection.osv && !selection.depsDev)}
                onClick={() => void reviewTransmission()}
              >
                {remoteActivity === "preview" ? "검토 준비 중…" : "전송 내용 검토"}
              </button>
            </div>
            <fieldset className="dependency-enrichment-controls" disabled={busy}>
              <legend className="sr-only">원격 metadata 서비스 선택</legend>
              <label>
                <input
                  type="checkbox"
                  checked={selection.osv}
                  onChange={(event) => {
                    const checked = event.currentTarget.checked;
                    setSelection((current) => ({ ...current, osv: checked }));
                    resetRemote();
                  }}
                />
                OSV vulnerability
              </label>
              <label>
                <input
                  type="checkbox"
                  checked={selection.depsDev}
                  onChange={(event) => {
                    const checked = event.currentTarget.checked;
                    setSelection((current) => ({ ...current, depsDev: checked }));
                    resetRemote();
                  }}
                />
                deps.dev 라이선스·상태
              </label>
              <label title="24시간 이내 캐시도 다시 조회합니다.">
                <input
                  type="checkbox"
                  checked={forceRefresh}
                  onChange={(event) => {
                    setForceRefresh(event.currentTarget.checked);
                    resetRemote();
                  }}
                />
                캐시 무시하고 새로 조회
              </label>
            </fieldset>
            {remoteActivity === "execute" && <div className="dependency-lens-status" role="status">검토한 좌표의 원격 정보를 불러오고 있습니다…</div>}
            {remoteError && <div className="error dependency-lens-error" role="alert">{remoteError}</div>}

            {preview && (
              <div className="dependency-enrichment-preview" aria-label="원격 전송 검토">
                <div className="dependency-enrichment-disclosure">
                  <strong>실제 전송 예정</strong>
                  <span>서비스별 전송 좌표 합계 {serviceTransmissionCount}개 · 로컬 package {preview.localPackageCount}개 분석 기준</span>
                  <span className="dim">repository 경로, lockfile 내용·경로, graph edge, registry·Git URL, checksum, credential, 환경·사용자 식별 정보는 보내지 않습니다.</span>
                </div>
                {preview.services.map((service) => (
                  <div className="dependency-enrichment-service" key={service.service}>
                    <div className="dependency-enrichment-service-head">
                      <strong>{SERVICE_LABEL[service.service]}</strong>
                      <span className="mono">https://{service.host}</span>
                      <span className="dim">요청 {service.requestCount} · 캐시 {service.cachedCount} · stale fallback {service.staleFallbackCount} · 상한 생략 {service.omittedCount}</span>
                    </div>
                    {service.transmitted.length > 0 ? (
                      <ul className="dependency-enrichment-coordinates">
                        {service.transmitted.map((coordinate) => (
                          <li key={`${coordinate.ecosystem}:${coordinate.name}@${coordinate.version}`}>
                            <span>{coordinate.direct ? "직접" : "전이"}</span>
                            <span className="mono">{coordinate.ecosystem}</span>
                            <strong>{coordinate.name}</strong>
                            <span className="mono">{coordinate.version}</span>
                            {coordinate.localPackageCount > 1 && <span className="dim">로컬 {coordinate.localPackageCount}개와 매핑</span>}
                          </li>
                        ))}
                      </ul>
                    ) : <p className="dim">전송할 좌표가 없습니다. 유효한 캐시만 적용합니다.</p>}
                  </div>
                ))}
                <div className="dependency-enrichment-confirm">
                  <span className="dim">이 검토는 5분 동안 한 번만 사용할 수 있으며 repository가 바뀌면 무효입니다.</span>
                  <button type="button" className="btn primary" disabled={busy} onClick={() => void executeEnrichment()}>
                    {serviceTransmissionCount > 0 ? "검토한 정보 보내기" : "캐시 결과 적용"}
                  </button>
                </div>
              </div>
            )}

            {enrichment && (
              <div className="dependency-enrichment-result" role="status">
                <div className="dependency-enrichment-result-head">
                  <strong>원격 보강 완료</strong>
                  <span className="dim">로컬 lock graph가 기준이며 원격 metadata는 참고 정보입니다.</span>
                </div>
                <div className="dependency-enrichment-summaries">
                  {enrichment.services.map((service) => (
                    <span key={service.service}>
                      <strong>{SERVICE_LABEL[service.service]}</strong> 대상 {service.targetCount} · 전송 {service.transmittedCount} · 캐시 {service.cachedCount} · stale {service.staleCount} · 실패 {service.failedCount} · 생략 {service.omittedCount}
                    </span>
                  ))}
                </div>
                {!enrichment.cachePersisted && <span className="dependency-lens-warning">이번 결과의 로컬 캐시를 저장하지 못했습니다.</span>}
              </div>
            )}
          </div>

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
              {filteredPackages.visible.map((dependency) => {
                const remote = enrichmentByPackage.get(dependency.id);
                return (
                  <details className="dependency-package" key={dependency.id}>
                    <summary>
                      <span className="dependency-scope">{dependency.direct ? "직접" : "전이"}</span>
                      <strong>{dependency.name}</strong>
                      <span className="mono">{dependency.version}</span>
                      {remote && remote.osv.advisoryIds.length > 0 && <span className="dependency-remote-badge danger">OSV {remote.osv.advisoryIds.length}</span>}
                      {remote?.depsDev.deprecated && <span className="dependency-remote-badge warn">deprecated</span>}
                      <span className="dim">{ECOSYSTEM_LABEL[dependency.ecosystem]} · edge {dependency.dependencies.length}</span>
                    </summary>
                    {remote && <RemotePackageMetadata entry={remote} />}
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
                );
              })}
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
