import type { Run, RunState, ScheduleOutcome } from "../../rpc/gen/rpc";
export const liveRun = (run?: Run) => !!run && ["starting", "running", "restarting"].includes(run.state);
export const runStateLabel: Record<RunState, string> = {
  starting: "시작 중",
  running: "실행 중",
  restarting: "재시작 중",
  exited: "종료",
  failed: "실패",
  stopped: "중지됨",
};
export function resultLabel(run?: Run) {
  if (!run) return "실행 기록 없음";
  if (run.exit?.code != null) return `종료 코드 ${run.exit.code}`;
  if (run.exit?.signal) return `신호 ${run.exit.signal}`;
  return runStateLabel[run.state];
}
export function runTone(run?: Run): "idle" | "progress" | "ok" | "danger" {
  if (!run) return "idle";
  return liveRun(run)
    ? run.ready
      ? "ok"
      : "progress"
    : run.state === "failed" || (run.exit?.code != null && run.exit.code !== 0)
      ? "danger"
      : "idle";
}

export const scheduleResultLabel: Record<ScheduleOutcome, string> = {
  claimed: "시작 준비",
  running: "실행 중",
  succeeded: "성공",
  failed: "실패",
  stopped: "중지됨",
  skippedMissed: "지난 예약 건너뜀",
  skippedTrust: "승인 필요로 건너뜀",
};
