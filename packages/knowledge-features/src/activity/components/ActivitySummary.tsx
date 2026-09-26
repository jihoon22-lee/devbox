import {
  fmtDuration,
  shortApp,
  toDateStr,
  fmtDay,
  formatNullableTimestamp,
  formatRunSummary,
  formatKnowledgeSummary,
  formatDailyActivity,
  digestSourceDetails,
  digestSourceId,
  digestSourceScope,
  sourceFreshnessState,
  sourceFreshnessLabel,
  digestSourceExplanation,
  digestActivitySourceNotice,
} from "../lib/activityPresentation";
import { projectAssociationLabel } from "../types";
import { isTauri } from "../lib/isTauri";
import type * as React from "react";

interface Props {
  summary:
    | import("../../generated/ActivityDaySummary").ActivityDaySummary
    | import("../../generated/ActivityRangeSummary").ActivityRangeSummary
    | null;
  topApp: import("../../generated/AppTotal").AppTotal | undefined;
  view: "day" | "week" | "month";
  loading: boolean;
  contextActionBusy: boolean;
  cancelCurrentLoad: () => Promise<void>;
  copyDigest: () => Promise<void>;
  digest: import("../../generated/ActivityDigestResponse").ActivityDigestResponse | null;
  downloadDigest: () => Promise<void>;
  sendDigestDraft: () => Promise<void>;
  digestAppFilter: string | null;
  selectDigestFilter: (next: string | null) => void;
  digestAppOptions: string[];
  range: import("../../generated/ActivityRangeSummary").ActivityRangeSummary | null;
  dateStr: string;
  dailyChartRefs: React.RefObject<(HTMLButtonElement | null)[]>;
  onDailyChartKeyDown: (
    event: React.KeyboardEvent<HTMLButtonElement>,
    index: number,
    points: import("../../generated/DayPoint").DayPoint[],
  ) => void;
  selectDate: (next: Date) => void;
  dateContextMenu: import("../../../../context-menu/src/useContextMenu").ContextMenuController;
  maxDaily: number;
  maxSummaryDuration: number;
  attribution: import("../../generated/ActivityAttributionResult").ActivityAttributionResult | null;
}

export function ActivitySummary({
  summary,
  topApp,
  view,
  loading,
  contextActionBusy,
  cancelCurrentLoad,
  copyDigest,
  digest,
  downloadDigest,
  sendDigestDraft,
  digestAppFilter,
  selectDigestFilter,
  digestAppOptions,
  range,
  dateStr,
  dailyChartRefs,
  onDailyChartKeyDown,
  selectDate,
  dateContextMenu,
  maxDaily,
  maxSummaryDuration,
  attribution,
}: Props) {
  return (
    <div className="day">
      {summary && (
        <>
          <div className="cards">
            <div className="card">
              <div className="card-label">PC 사용</div>
              <div className="card-value">{fmtDuration(summary.pc_usage_ms)}</div>
            </div>
            <div className="card">
              <div className="card-label">Git 커밋 · 기간 전체</div>
              <div className="card-value">{summary.git.total_commits}</div>
            </div>
            <div className="card">
              <div className="card-label">가장 활발한 앱</div>
              <div className="card-value">
                {topApp ? shortApp(topApp.app) : summary.app_totals[0] ? shortApp(summary.app_totals[0].app) : "-"}
              </div>
            </div>
          </div>

          {(view === "day" || view === "week" || view === "month") && (
            <section className="panel digest-panel" aria-busy={loading || contextActionBusy}>
              <div className="digest-heading">
                <div>
                  <h2>{view === "day" ? "일간 로컬 요약" : view === "month" ? "월간 로컬 요약" : "주간 로컬 요약"}</h2>
                  <p className="dim">결정론적 규칙으로만 계산하며 네트워크·AI·외부 전송을 사용하지 않습니다.</p>
                </div>
                <div className="digest-actions">
                  {loading && (
                    <button
                      type="button"
                      className="btn"
                      onClick={() => void cancelCurrentLoad()}
                      aria-label="digest 불러오기 취소"
                    >
                      취소
                    </button>
                  )}
                  <button
                    type="button"
                    className="btn"
                    onClick={() => void copyDigest()}
                    disabled={!digest || contextActionBusy || loading}
                  >
                    요약 복사
                  </button>
                  <button
                    type="button"
                    className="btn active"
                    onClick={() => void downloadDigest()}
                    disabled={!digest || contextActionBusy || loading}
                  >
                    {isTauri() ? "요약 저장" : "미리보기 다운로드"}
                  </button>
                  <button
                    type="button"
                    className="btn"
                    onClick={() => void sendDigestDraft()}
                    disabled={!isTauri() || !digest || contextActionBusy || loading}
                    title={isTauri() ? undefined : "Knowledge handoff는 native 데스크톱에서만 사용할 수 있습니다"}
                  >
                    Knowledge로 보내기
                  </button>
                </div>
              </div>
              {digest ? (
                <>
                  <div className="digest-toolbar">
                    <label htmlFor="life-log-digest-app-filter">
                      애플리케이션 필터
                      <select
                        id="life-log-digest-app-filter"
                        value={digestAppFilter ?? ""}
                        onChange={(event) => selectDigestFilter(event.currentTarget.value || null)}
                        disabled={contextActionBusy || loading}
                      >
                        <option value="">모든 애플리케이션</option>
                        {digestAppOptions.map((app) => (
                          <option key={app} value={app}>
                            {shortApp(app)}
                          </option>
                        ))}
                      </select>
                    </label>
                    <span className="dim scope-note" role="status" aria-live="polite">
                      {digest.origin === "browser-preview"
                        ? "브라우저 미리보기만 사용 · 네이티브 로컬 데이터 사용 불가 · "
                        : "네이티브 로컬 요약 · "}
                      {digest.document.range.startDate} ~ {digest.document.range.endDate} ·{" "}
                      {digest.document.range.timezone} · Git 커밋은 요청한 전체 기간을 사용하며 이 앱 필터를 무시합니다.
                    </span>
                  </div>
                  {digestActivitySourceNotice(digest.document) && (
                    <p className="activity-source-notice" role="note">
                      {digestActivitySourceNotice(digest.document)}
                    </p>
                  )}
                  <div className="digest-cards">
                    <div className="card">
                      <div className="card-label">PC 사용</div>
                      <div className="card-value">{fmtDuration(digest.document.summary.pcUsageMs)}</div>
                    </div>
                    <div className="card">
                      <div className="card-label">활동일</div>
                      <div className="card-value">
                        {digest.document.summary.activeDays}/{digest.document.summary.totalDays}
                      </div>
                    </div>
                    <div className="card">
                      <div className="card-label">세션</div>
                      <div className="card-value">{digest.document.summary.sessionCount}</div>
                    </div>
                    <div className="card">
                      <div className="card-label">Git 커밋 · 기간 전체</div>
                      <div className="card-value">{digest.document.summary.gitCommits}</div>
                    </div>
                    <div className="card activity-card" data-testid="run-summary">
                      <div className="card-label">Run Manager · 기간 전체</div>
                      <div className="card-value">{formatRunSummary(digest.document.summary.run)}</div>
                      <div className="dim">
                        마지막 실행 {formatNullableTimestamp(digest.document.summary.run?.lastRunAtMs ?? null)}
                      </div>
                    </div>
                    <div className="card activity-card" data-testid="knowledge-summary">
                      <div className="card-label">Knowledge 노트 · 기간 전체</div>
                      <div className="card-value">{formatKnowledgeSummary(digest.document.summary.knowledge)}</div>
                      <div className="dim">
                        마지막 수정{" "}
                        {formatNullableTimestamp(digest.document.summary.knowledge?.lastModifiedAtMs ?? null)}
                      </div>
                    </div>
                  </div>
                  <p className="digest-headline">{digest.document.headline}</p>
                  {digest.document.summary.activeDays === 0 && digest.document.summary.gitCommits === 0 && (
                    <div className="empty">선택한 기간과 필터에 기록된 활동이 없습니다.</div>
                  )}
                  <div className="digest-days" aria-label="일별 digest">
                    {digest.document.daily.map((day) => (
                      <div key={day.date} className={`digest-day ${day.hasActivity ? "" : "empty-day"}`}>
                        <span className="mono">{day.date}</span>
                        <span>{fmtDuration(day.pcUsageMs)}</span>
                        <span className="dim">
                          세션 {day.sessionCount}개 · 커밋 {day.gitCommits}개
                        </span>
                        <span className="dim">{day.topApp ? shortApp(day.topApp) : "-"}</span>
                        <span className="dim daily-activity">{formatDailyActivity(day)}</span>
                      </div>
                    ))}
                  </div>
                  <details className="digest-details">
                    <summary>소스와 집계 규칙</summary>
                    <div className="digest-source-list">
                      {digest.document.sources.map((source) => (
                        <div
                          key={`${digestSourceId(source.id)}:${digestSourceScope(source.scope)}`}
                          className="git-row"
                        >
                          <span className="mono">{digestSourceId(source.id)}</span>
                          <span className="source-details">
                            <span className={source.available ? "source-ok" : "source-error"}>
                              {source.available
                                ? "사용 가능"
                                : source.errorCode === "browser_preview_only"
                                  ? "브라우저 미리보기만"
                                  : "사용 불가"}
                              {` · ${digestSourceScope(source.scope)}`}
                            </span>
                            <span
                              className={`freshness-badge freshness-${sourceFreshnessState(source.freshnessMs, source.available, source.errorCode)}`}
                            >
                              {sourceFreshnessLabel(
                                sourceFreshnessState(source.freshnessMs, source.available, source.errorCode),
                              )}
                            </span>
                            <span className="source-explanation">{digestSourceExplanation(source)}</span>
                            {source.errorCode && <span className="source-error">오류 코드: {source.errorCode}</span>}
                            {digestSourceDetails(source) && <span className="dim">{digestSourceDetails(source)}</span>}
                          </span>
                        </div>
                      ))}
                    </div>
                    <div className="digest-rules">
                      {Object.entries(digest.document.rules).map(([name, rule]) => (
                        <div key={name} className="digest-rule">
                          <span>{name}</span>
                          <span className="dim">{rule}</span>
                        </div>
                      ))}
                    </div>
                  </details>
                </>
              ) : (
                <div className="empty">Digest를 준비하는 중입니다…</div>
              )}
            </section>
          )}

          {view !== "day" && range && (
            <section className="panel">
              <h2>{range.label} — 일별 사용량</h2>
              <div className="daily-chart">
                {range.daily.map((p, index) => {
                  const pointDate = new Date(p.day_ms);
                  const pointDateStr = toDateStr(pointDate);
                  return (
                    <button
                      key={p.day_ms}
                      type="button"
                      className="daily-col"
                      title={`${fmtDay(p.day_ms)}: ${fmtDuration(p.pc_usage_ms)}`}
                      data-date={pointDateStr}
                      aria-label={`${pointDateStr} 날짜`}
                      aria-current={pointDateStr === dateStr ? "date" : undefined}
                      aria-roledescription="일별 사용량"
                      tabIndex={pointDateStr === dateStr ? 0 : -1}
                      ref={(element) => {
                        dailyChartRefs.current[index] = element;
                      }}
                      onKeyDown={(event) => onDailyChartKeyDown(event, index, range.daily)}
                      onClick={() => selectDate(pointDate)}
                      {...dateContextMenu.triggerProps}
                    >
                      <div
                        className="daily-bar"
                        style={{ height: `${Math.max(2, (p.pc_usage_ms / maxDaily) * 100)}%` }}
                      />
                      <div className="daily-label">{fmtDay(p.day_ms)}</div>
                    </button>
                  );
                })}
                {range.daily.length === 0 && <div className="empty">이 기간에 활동이 없습니다</div>}
              </div>
            </section>
          )}

          {summary.app_totals.length > 0 && (
            <section className="panel">
              <h2>앱 사용량</h2>
              {summary.app_totals.map((a) => (
                <div key={a.app} className="stat-row">
                  <span className="stat-app">{shortApp(a.app)}</span>
                  <div className="stat-bar">
                    <div
                      className="stat-fill"
                      style={{ width: `${Math.min(100, (a.duration_ms / maxSummaryDuration) * 100)}%` }}
                    />
                  </div>
                  <span className="stat-dur">{fmtDuration(a.duration_ms)}</span>
                </div>
              ))}
            </section>
          )}

          {attribution && attribution.profileCount > 0 && (
            <section className="panel">
              <h2>프로젝트 귀속 · 모든 애플리케이션</h2>
              {attribution.attributed.map((a) => (
                <div key={a.projectId} className="git-row">
                  <span className="mono dim">
                    {a.projectId}
                    {a.projectAssociation && <small> · {projectAssociationLabel(a.projectAssociation)}</small>}
                  </span>
                  <span className="git-count">
                    세션 {a.sessions}개 · {fmtDuration(a.durationMs)}
                  </span>
                </div>
              ))}
              {attribution.unattributed.sessions > 0 && (
                <div className="git-row">
                  <span className="dim">미귀속</span>
                  <span className="git-count">
                    세션 {attribution.unattributed.sessions}개 · {fmtDuration(attribution.unattributed.durationMs)}
                  </span>
                </div>
              )}
              <div className="dim">
                귀속은 창 제목의 프로젝트 이름 매치 기준이며 애플리케이션 필터와 독립적입니다(가장 긴 이름 우선, 중복
                집계 없음).
              </div>
            </section>
          )}

          {summary.git.projects.length > 0 && (
            <section className="panel">
              <h2>Git</h2>
              <p className="dim">
                같은 저장소의 동일 커밋은 한 번만 집계하며, 경로 순서상 첫 프로젝트에 귀속합니다. 조회하지 못한
                프로젝트는 합계에서 제외합니다.
              </p>
              {summary.git.projects.map((p) => (
                <div key={p.path} className="git-row">
                  <span className="mono dim">
                    {p.path}
                    {p.projectAssociation && <small> · {projectAssociationLabel(p.projectAssociation)}</small>}
                  </span>
                  <span className="git-count">
                    {p.error_code ? "조회할 수 없음 · 경로와 Git 연결 확인" : `커밋 ${p.commits}개`}
                  </span>
                </div>
              ))}
            </section>
          )}
        </>
      )}
    </div>
  );
}
