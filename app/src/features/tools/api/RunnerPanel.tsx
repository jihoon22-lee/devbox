/** 컬렉션 실행기: 순서·생산→소비 확인, 실행, 단계별 결과(상태·변수 출처·건너뛴 이유)와 요약. */
import { useEffect, useRef, useState } from "react";
import type { CollectionRoot, Pair } from "../../../rpc/gen/rpc";
import { Button } from "../../../ui";
import s from "./Api.module.css";
import { useApiBackend } from "./backend";
import { RunPlanView } from "./RunPlanView";
import { MAX_RUN_DELAY_MS, type RunEntry, type RunStep, type RunSummary, runCollection } from "./runner/runner";
import type { Session } from "./runner/session";
import { toApiResponse } from "./sendEvents";

const STATUS: Record<RunStep["status"], string> = { passed: "통과", failed: "실패", error: "오류", skipped: "건너뜀" };

function sleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(resolve, ms);
    signal.addEventListener(
      "abort",
      () => {
        clearTimeout(timer);
        reject(new Error("cancelled"));
      },
      { once: true },
    );
  });
}

export function RunnerPanel({
  entries,
  environment,
  session,
  root,
}: {
  entries: RunEntry[];
  environment: Pair[];
  session: Session;
  root?: CollectionRoot;
}) {
  const backend = useApiBackend();
  const [order, setOrder] = useState(entries);
  const [steps, setSteps] = useState<RunStep[]>([]);
  const [summary, setSummary] = useState<RunSummary>();
  const [stopOnFailure, setStopOnFailure] = useState(false);
  const [delayMs, setDelayMs] = useState(0);
  const [error, setError] = useState<string>();
  const [runId, setRunId] = useState<string>();
  const [running, setRunning] = useState(false);
  const controller = useRef<AbortController>(undefined);
  useEffect(() => setOrder(entries), [entries]);
  useEffect(() => () => controller.current?.abort(), []);
  const names = new Map(order.map((entry) => [entry.id, entry.name]));

  async function run() {
    const abort = new AbortController();
    controller.current = abort;
    setSteps([]);
    setSummary(undefined);
    setError(undefined);
    const runId = crypto.randomUUID();
    setRunId(runId);
    setRunning(true);
    try {
      const result = await runCollection(
        order,
        environment,
        session,
        { stopOnFailure, delayMs },
        {
          runId,
          sleep,
          send: async (snapshot, variables, signal) =>
            toApiResponse(
              await backend.send(
                {
                  snapshot,
                  env: { variables },
                  runId,
                  origin: null,
                  cookieJar: root ? (root.kind === "project" ? root.projectId : "personal") : null,
                },
                signal,
              ),
            ),
        },
        abort.signal,
        (step) => setSteps((previous) => [...previous, step]),
      );
      setSummary(result);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "실행하지 못했습니다");
      setSummary(undefined);
    } finally {
      controller.current = undefined;
      setRunning(false);
    }
  }

  const source = (step: RunStep) =>
    step.sources
      .map((item) =>
        item.from === "environment"
          ? `${item.name}: 환경`
          : `${item.name}: ${item.from.runId === runId ? (names.get(item.from.requestId) ?? item.from.requestId) : "이전 실행"}`,
      )
      .join(", ");
  return (
    <div className={s.pane}>
      <RunPlanView entries={order} disabled={running} onReorder={setOrder} />
      <div className={s.bar}>
        <label className={s.bar}>
          <input type="checkbox" checked={stopOnFailure} onChange={(event) => setStopOnFailure(event.target.checked)} />
          실패하면 멈추기
        </label>
        <label className={s.bar}>
          요청 사이 대기(ms)
          <input
            className={s.input}
            type="number"
            min={0}
            max={MAX_RUN_DELAY_MS}
            value={delayMs}
            onChange={(event) => setDelayMs(Number(event.target.value))}
          />
        </label>
        {running ? (
          <Button onClick={() => controller.current?.abort()}>중지</Button>
        ) : (
          <Button variant="primary" disabled={order.length === 0} onClick={() => void run()}>
            실행
          </Button>
        )}
      </div>
      {error && <p role="alert">{error}</p>}
      {(steps.length > 0 || summary) && (
        <table className={s.results} aria-label="실행 결과">
          <thead>
            <tr>
              <th scope="col">요청</th>
              <th scope="col">결과</th>
              <th scope="col">HTTP</th>
              <th scope="col">시간</th>
              <th scope="col">변수 출처</th>
              <th scope="col">메모</th>
            </tr>
          </thead>
          <tbody>
            {steps.map((step) => (
              <tr key={step.entryId} aria-label={step.name}>
                <td>{step.name}</td>
                <td>{STATUS[step.status]}</td>
                <td>{step.httpStatus ?? "—"}</td>
                <td>{step.durationMs === null ? "—" : `${step.durationMs} ms`}</td>
                <td>{source(step)}</td>
                <td>
                  {step.message ||
                    step.assertions
                      .filter((check) => !check.passed)
                      .map((check) => check.message)
                      .join(", ")}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {summary && (
        <p className={s.muted}>
          {`통과 ${summary.passed} · 실패 ${summary.failed} · 오류 ${summary.errors} · 건너뜀 ${summary.skipped}`}
          {summary.cancelled ? " (중지함)" : ""}
        </p>
      )}
    </div>
  );
}
