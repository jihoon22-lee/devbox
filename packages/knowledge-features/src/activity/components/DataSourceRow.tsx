import {
  fmtDuration,
  sourceFreshnessState,
  sourceFreshnessLabel,
  DigestSource,
  fixedSourceExplanation,
  digestSourceExplanation,
} from "../lib/activityPresentation";
import { type SourceStatus } from "../api";

export function DataSourceRow({ source }: { source: SourceStatus }) {
  const diagnostics = [
    source.schemaVersion != null ? `v${source.schemaVersion}` : null,
    source.producerVersion,
    source.freshnessMs != null ? `${fmtDuration(source.freshnessMs)} 전 갱신` : null,
  ].filter((value): value is string => value != null);
  const activity = source.knowledgeActivity;
  const freshness = sourceFreshnessState(source.freshnessMs, source.available, source.errorCode ?? source.error);
  const sourceMetadata: DigestSource = {
    id: source.producer,
    available: source.available,
    schemaVersion: source.schemaVersion,
    snapshotVersion: null,
    producerVersion: source.producerVersion,
    generatedAt: source.generatedAt,
    freshnessMs: source.freshnessMs,
    view: null,
    scope: source.scope ?? "unavailable",
    errorCode: source.errorCode ?? null,
  };
  const fixedExplanation = fixedSourceExplanation({
    ...sourceMetadata,
    freshnessState:
      source.freshnessState === "fresh" ||
      source.freshnessState === "stale" ||
      source.freshnessState === "expired" ||
      source.freshnessState === "error"
        ? source.freshnessState
        : "unknown",
  });

  return (
    <div className="git-row source-row">
      <span className="mono">{source.producer}</span>
      <div className="source-details">
        <span className={`freshness-badge freshness-${freshness}`}>{sourceFreshnessLabel(freshness)}</span>
        {diagnostics.length > 0 && <span className="dim">{diagnostics.join(" · ")}</span>}
        <span className="dim">{source.scope ?? "범위 없음"}</span>
        <span className="source-explanation">
          {fixedExplanation ?? source.explanation ?? digestSourceExplanation(sourceMetadata)}
        </span>
        {source.available && activity && (
          <span className="source-activity">
            오늘 작성·수정 {activity.notesModifiedToday}개
            {activity.lastModifiedAtMs != null &&
              ` · 마지막 수정 ${new Date(activity.lastModifiedAtMs).toLocaleString()}`}
            {activity.legacy_snapshot && " · 구버전 snapshot"}
            {!activity.identifiersComplete &&
              !activity.legacy_snapshot &&
              ` · 식별자 ${activity.identifiedNotes}개만 포함`}
          </span>
        )}
        {!source.available && (
          <span role="alert" className="source-error">
            {source.errorCode ? `${source.errorCode} · ` : ""}
            {fixedExplanation ? "사용할 수 없음" : (source.error ?? "사용할 수 없음")}
          </span>
        )}
      </div>
    </div>
  );
}
