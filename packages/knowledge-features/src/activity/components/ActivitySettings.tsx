import { useEffect, useRef, useState } from "react";
import { DataSourceRow } from "./DataSourceRow";
import { fmtDuration, digestSourceScope } from "../lib/activityPresentation";
import PrivacyRulesPanel from "../PrivacyRulesPanel";
import { isImeComposing } from "@devbox/a11y";
import { setAutostart, setIdleThreshold, getIdleThreshold } from "../api";
import { isTauri } from "../lib/isTauri";
import type * as React from "react";

interface Props {
  lifecycleSettings: React.ReactNode;
  sources: import("../../generated/SourceStatus").SourceStatus[];
  refreshDraftHistory: () => Promise<void>;
  contextActionBusy: boolean;
  draftHistory: import("../../generated/DraftHistoryEntry").DraftHistoryEntry[];
  regenerateDraft: (entry: import("../../generated/DraftHistoryEntry").DraftHistoryEntry) => Promise<void>;
  loading: boolean;
  projects: string[];
  projectProbes: Record<string, import("../../generated/ProjectProbe").ProjectProbe>;
  checkProject: (path: string) => Promise<void>;
  projectProbePath: string | null;
  projectSaving: boolean;
  removeProject: (p: string) => Promise<void>;
  projectInput: string;
  setProjectInput: React.Dispatch<React.SetStateAction<string>>;
  addProject: () => Promise<void>;
  idleThreshold: number;
  setIdleThresholdState: React.Dispatch<React.SetStateAction<number>>;
  autoStart: import("../../generated/AgentAutostartStatus").AgentAutostartStatus | null;
  setError: React.Dispatch<React.SetStateAction<string | null>>;
  setNotice: React.Dispatch<React.SetStateAction<string | null>>;
  setAutoStart: React.Dispatch<
    React.SetStateAction<import("../../generated/AgentAutostartStatus").AgentAutostartStatus | null>
  >;
  privacy: import("../../generated/PrivacyRules").PrivacyRules;
  privacyHealthy: boolean;
  setPrivacy: React.Dispatch<React.SetStateAction<import("../../generated/PrivacyRules").PrivacyRules>>;
  setPrivacyHealthy: React.Dispatch<React.SetStateAction<boolean>>;
}

export function ActivitySettings({
  lifecycleSettings,
  sources,
  refreshDraftHistory,
  contextActionBusy,
  draftHistory,
  regenerateDraft,
  loading,
  projects,
  projectProbes,
  checkProject,
  projectProbePath,
  projectSaving,
  removeProject,
  projectInput,
  setProjectInput,
  addProject,
  idleThreshold,
  setIdleThresholdState,
  autoStart,
  setError,
  setNotice,
  setAutoStart,
  privacy,
  privacyHealthy,
  setPrivacy,
  setPrivacyHealthy,
}: Props) {
  const [thresholdDraft, setThresholdDraft] = useState(String(Math.round(idleThreshold / 60000)));
  const [thresholdSaving, setThresholdSaving] = useState(false);
  const [thresholdError, setThresholdError] = useState("");
  const thresholdSequence = useRef(0);
  useEffect(() => {
    setThresholdDraft(String(Math.round(idleThreshold / 60000)));
  }, [idleThreshold]);
  useEffect(
    () => () => {
      thresholdSequence.current++;
    },
    [],
  );
  const saveThreshold = async () => {
    const minutes = Number(thresholdDraft);
    if (!Number.isInteger(minutes) || minutes < 1 || minutes > 1440) {
      setThresholdError("1분부터 1440분 사이의 정수를 입력해 주세요.");
      return;
    }
    const sequence = ++thresholdSequence.current;
    setThresholdSaving(true);
    setThresholdError("");
    try {
      await setIdleThreshold(minutes * 60000);
      const acknowledged = await getIdleThreshold();
      if (sequence === thresholdSequence.current) {
        setIdleThresholdState(acknowledged);
        setThresholdDraft(String(Math.round(acknowledged / 60000)));
      }
    } catch {
      if (sequence === thresholdSequence.current)
        setThresholdError("유휴 시간을 저장하지 못했습니다. 이전 설정을 유지했습니다. 다시 시도해 주세요.");
    } finally {
      if (sequence === thresholdSequence.current) setThresholdSaving(false);
    }
  };
  return (
    <div className="settings">
      {lifecycleSettings}
      <section className="panel">
        <h2>데이터 소스</h2>
        {sources.length === 0 && <div className="dim">등록된 소스가 없습니다.</div>}
        {sources.map((s) => (
          <DataSourceRow
            key={`${s.producer}:v${s.schemaVersion ?? "unknown"}:${s.available ? "ok" : "error"}`}
            source={s}
          />
        ))}
        <div className="dim">
          소스는 devbox 공용 루트의 읽기 전용 snapshot을 통해 읽습니다(다른 앱의 DB를 직접 읽지 않음).
        </div>
      </section>

      <section className="panel" aria-label="Knowledge 초안 handoff 기록">
        <div className="panel-heading-row">
          <div>
            <h2>Knowledge handoff 기록</h2>
            <div className="dim">
              상태와 집계 요약/소스 참조만 보존합니다. 활동 원문·경로·자격 증명은 저장하지 않습니다.
            </div>
          </div>
          <button
            className="btn small"
            type="button"
            onClick={() => void refreshDraftHistory()}
            disabled={contextActionBusy}
          >
            새로 고침
          </button>
        </div>
        {draftHistory.length === 0 ? (
          <div className="dim">아직 보낸 초안이 없습니다.</div>
        ) : (
          draftHistory.map((entry) => (
            <div className="handoff-history-row" key={entry.handoffId}>
              <div className="handoff-history-main">
                <span className={`handoff-status handoff-status-${entry.status}`}>{entry.status}</span>
                <strong>
                  {entry.summary.startDate} ~ {entry.summary.endDate}
                </strong>
                <span className="dim">
                  {entry.summary.period} · {entry.summary.timezone}
                </span>
                <span className="dim">
                  세션 {entry.summary.sessionCount}개 · {fmtDuration(entry.summary.pcUsageMs)} · 커밋{" "}
                  {entry.summary.gitCommits}개
                </span>
              </div>
              <div className="handoff-history-sources">
                {entry.sources.map((source) => (
                  <span key={source.id} className={source.available ? "source-ok" : "source-error"}>
                    {source.id} · {digestSourceScope(source.scope)}
                    {source.errorCode ? ` · ${source.errorCode}` : ""}
                  </span>
                ))}
              </div>
              <button
                className="btn small"
                type="button"
                onClick={() => void regenerateDraft(entry)}
                disabled={!isTauri() || contextActionBusy || loading}
              >
                다시 생성
              </button>
            </div>
          ))
        )}
      </section>

      <section className="panel">
        <h2>Git 프로젝트 경로</h2>
        {projects.map((p) => (
          <div key={p} className="git-row">
            <div className="git-project-details">
              <span className="mono">{p}</span>
              {projectProbes[p] && (
                <span className={projectProbes[p].repository ? "project-probe-ok" : "project-probe-error"}>
                  {projectProbes[p].repository
                    ? `Git 저장소 확인됨 · ${projectProbes[p].target === "wsl" ? "WSL" : "Windows"}`
                    : `확인 실패 · ${projectProbes[p].errorCode ?? "git_failed"}`}
                </span>
              )}
            </div>
            <div className="git-project-actions">
              <button
                className="mini"
                onClick={() => void checkProject(p)}
                disabled={projectProbePath !== null || projectSaving}
              >
                {projectProbePath === p ? "확인 중…" : "연결 확인"}
              </button>
              <button
                className="mini"
                onClick={() => void removeProject(p)}
                disabled={projectSaving}
                aria-label={`${p} 제거`}
              >
                ✕
              </button>
            </div>
          </div>
        ))}
        <div className="row">
          <input
            placeholder="C:\projects\devbox 또는 \\wsl$\Ubuntu\home\user\project"
            value={projectInput}
            onChange={(e) => setProjectInput(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (!isImeComposing(e) && e.key === "Enter") void addProject();
            }}
            disabled={projectSaving}
          />
          <button className="btn" onClick={() => void addProject()} disabled={projectSaving || !projectInput.trim()}>
            {projectSaving ? "저장 중…" : "추가"}
          </button>
        </div>
        <div className="dim">
          연결 확인은 중지된 WSL 배포판을 시작할 수 있습니다. 경로 저장만으로는 배포판을 시작하지 않습니다.
        </div>
        <div className="dim">
          {lifecycleSettings
            ? "활동 수집을 켜면 세션이 기록됩니다. 수집 중지는 활동 화면에서 선택할 수 있습니다."
            : "활동 추적은 Life Log에 통합되어 있으며, 세션은 자동으로 기록됩니다."}
        </div>
      </section>

      <section className="panel">
        <h2>유휴 감지</h2>
        <div className="row">
          <span className="dim">자리를 비운 지 (분):</span>
          <input
            type="number"
            min={1}
            aria-label="유휴 시간 (분)"
            max={1440}
            disabled={thresholdSaving}
            value={thresholdDraft}
            onChange={(event) => setThresholdDraft(event.currentTarget.value)}
          />
        </div>
        <button className="btn" disabled={thresholdSaving} onClick={() => void saveThreshold()}>
          {thresholdSaving ? "유휴 시간 저장 중…" : "유휴 시간 저장"}
        </button>
        <p className="dim">적용된 유휴 시간: {Math.round(idleThreshold / 60000)}분</p>
        {thresholdError && <p role="alert">{thresholdError}</p>}
        <div className="dim">이 시간 이상 입력이 없으면 해당 구간을 사용 시간에서 제외합니다.</div>
      </section>

      <section className="panel">
        <h2>자동 시작</h2>
        {autoStart?.supported ? (
          <label className="row">
            <input
              type="checkbox"
              checked={autoStart.enabled}
              onChange={(e) => {
                setError(null);
                setNotice(null);
                void (async () => {
                  try {
                    const next = await setAutostart(e.currentTarget.checked);
                    setAutoStart(next);
                  } catch (err) {
                    setError(err instanceof Error ? err.message : String(err));
                  }
                })();
              }}
            />
            로그인할 때 백그라운드 서비스 시작
          </label>
        ) : (
          <div className="dim">설치된 Windows Suite에서 자동 시작을 설정할 수 있습니다.</div>
        )}
      </section>

      <PrivacyRulesPanel
        initial={privacy}
        healthy={privacyHealthy}
        onSaved={(rules) => {
          setPrivacy(rules);
          setPrivacyHealthy(true);
        }}
      />
    </div>
  );
}
