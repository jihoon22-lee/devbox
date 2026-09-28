import { useEffect, useRef, useState } from "react";
import { useOperation } from "@devbox/hooks";
import type { CollectionEntry } from "./lib/collections";
import type { EnvVariable } from "./lib/environments";
import { runCollection, type RunDeps, type RunStep, type RunSummary, type SessionVariables } from "./lib/runner";
interface Props {
  entries: CollectionEntry[];
  environment: EnvVariable[];
  session: SessionVariables;
  deps: RunDeps;
  onSelect: (id: string) => void;
  onClose: () => void;
  onSessionChange: () => void;
  onBusyChange?: (busy: boolean) => void;
}
const statusLabels = { passed: "통과", failed: "실패", error: "오류", skipped: "건너뜀" };
/** Diagnostic sharing excludes response data and assertion expected/actual text. */
export function runnerSummaryJson(summary: RunSummary): string {
  return JSON.stringify(
    {
      ...summary,
      steps: summary.steps.map((step) => ({
        ...step,
        assertions: step.assertions.map((assertion) => ({ id: assertion.id, passed: assertion.passed })),
      })),
    },
    null,
    2,
  );
}
export function RunnerPanel({
  entries,
  environment,
  session,
  deps,
  onSelect,
  onClose,
  onSessionChange,
  onBusyChange,
}: Props) {
  const [folder, setFolder] = useState("");
  const [stopOnFailure, setStopOnFailure] = useState(true);
  const [delayMs, setDelayMs] = useState(0);
  const [steps, setSteps] = useState<RunStep[]>([]);
  const [summary, setSummary] = useState<RunSummary | null>(null);
  const [copyError, setCopyError] = useState("");
  const controller = useRef<AbortController | null>(null);
  const { busy, issue, run } = useOperation();
  useEffect(
    () => () => {
      controller.current?.abort();
      onBusyChange?.(false);
    },
    [onBusyChange],
  );
  const start = () => {
    if (controller.current) return;
    const current = new AbortController();
    controller.current = current;
    onBusyChange?.(true);
    setSummary(null);
    setSteps([]);
    setCopyError("");
    void run(async (lifetime) => {
      const stop = () => current.abort();
      lifetime.addEventListener("abort", stop, { once: true });
      try {
        const result = await runCollection(
          entries.filter((entry) => !folder || entry.folder === folder),
          environment,
          session,
          { stopOnFailure, delayMs },
          deps,
          current.signal,
          (step) => {
            if (!lifetime.aborted) {
              setSteps((previous) => [...previous, step]);
              onSessionChange();
            }
          },
        );
        if (!lifetime.aborted) setSummary(result);
      } finally {
        lifetime.removeEventListener("abort", stop);
        controller.current = null;
        onBusyChange?.(false);
      }
    });
  };
  return (
    <section className="import-panel" aria-label="컬렉션 실행">
      <h2>컬렉션 실행</h2>
      <button type="button" disabled={busy} onClick={onClose}>
        닫기
      </button>
      <select aria-label="실행 범위" value={folder} disabled={busy} onChange={(e) => setFolder(e.currentTarget.value)}>
        <option value="">컬렉션 전체</option>
        {[...new Set(entries.map((entry) => entry.folder).filter(Boolean))].map((folder) => (
          <option key={folder} value={folder}>
            {folder}
          </option>
        ))}
      </select>
      <label>
        <input
          type="checkbox"
          checked={stopOnFailure}
          disabled={busy}
          onChange={(e) => setStopOnFailure(e.currentTarget.checked)}
        />
        실패하면 멈춤
      </label>
      <label>
        대기(ms)
        <input
          type="number"
          min={0}
          max={5000}
          value={delayMs}
          disabled={busy}
          onChange={(e) => setDelayMs(Number(e.currentTarget.value))}
        />
      </label>
      <button type="button" disabled={busy || !entries.length} onClick={start}>
        실행
      </button>
      <button type="button" disabled={!busy} onClick={() => controller.current?.abort()}>
        중지
      </button>
      {issue && <p role="alert">{issue}</p>}
      {steps.map((step, index) => (
        <div key={`${index}:${step.entryId}`}>
          <button
            type="button"
            disabled={busy || !["failed", "error"].includes(step.status)}
            onClick={() => onSelect(step.entryId)}
          >
            {step.name} {statusLabels[step.status]}
          </button>{" "}
          {step.httpStatus ?? ""} {step.durationMs !== null ? `${step.durationMs}ms` : ""} {step.message}
        </div>
      ))}
      {summary && (
        <>
          <p role="status">
            통과 {summary.passed} · 실패 {summary.failed} · 오류 {summary.errors} · 건너뜀 {summary.skipped}
          </p>
          {summary.cancelled && <p>실행이 중지되었습니다.</p>}
          <button
            type="button"
            onClick={() => {
              void navigator.clipboard
                .writeText(runnerSummaryJson(summary))
                .catch(() => setCopyError("결과를 복사하지 못했습니다."));
            }}
          >
            결과 복사
          </button>
        </>
      )}
      {copyError && <p role="alert">{copyError}</p>}
    </section>
  );
}
