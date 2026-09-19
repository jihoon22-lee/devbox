import type { DashboardFreshness } from "./resourceDisplay";

export interface SnapshotAge {
  capturedAtMs: number;
  staleAfterMs: number;
}

/**
 * 마지막 정상 snapshot이 TTL을 넘겼는지. 시계를 신뢰할 수 없거나(미래 시각) TTL이
 * 비정상이면 만료로 본다 — 판단이 서지 않을 때는 항상 닫는 쪽이다.
 */
export function isSnapshotExpired(snapshot: SnapshotAge, nowMs: number): boolean {
  const { capturedAtMs, staleAfterMs } = snapshot;
  if (!Number.isFinite(capturedAtMs) || !Number.isFinite(staleAfterMs) || staleAfterMs <= 0) return true;
  const age = nowMs - capturedAtMs;
  if (age < 0) return true;
  return age > staleAfterMs;
}

/**
 * snapshot을 근거로 잠그는 조작(동시 입력, Docker 상태 변경)을 지금 허용할 수 있는가.
 *
 * 새 collection이 진행 중(`refreshing`)이라는 사실만으로는 잠그지 않는다. dashboard
 * snapshot은 distro별 개수만 담고 대상 세션의 정체성은 담지 않으며
 * (`SessionState::terminal_counts_by_distro`), backend는 보유하지 않은 세션을 지정한
 * broadcast를 스스로 거부한다. in-flight refresh 동안 조작을 막는 것은 안전을 더하지
 * 않고 TTL 주기마다 사용자가 켜 둔 상태만 되돌린다. 수집 실패(`error`), 만료
 * (`stale` 또는 TTL 초과), snapshot 부재에서는 계속 fail-closed다.
 */
export function isSnapshotActionable(
  state: DashboardFreshness,
  hasSnapshot: boolean,
  expired: boolean,
): boolean {
  if (!hasSnapshot || expired) return false;
  return state !== "loading" && state !== "stale" && state !== "error";
}
