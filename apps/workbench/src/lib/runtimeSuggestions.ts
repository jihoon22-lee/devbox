import { MAX_EXPECTED_PORTS, parseExpectedPorts } from "./profileEditor";

export interface RuntimePortMergeResult {
  nextText: string | null;
  error?: string;
}

/**
 * Preserves the existing validated order and appends selected published ports
 * in deterministic ascending order. Invalid draft text is never replaced.
 */
export function mergeSuggestedPorts(
  expectedPortsText: string,
  selectedPorts: Iterable<number>,
): RuntimePortMergeResult {
  const existing = parseExpectedPorts(expectedPortsText);
  if (existing.error) {
    return {
      nextText: null,
      error: "현재 예상 포트 입력을 먼저 수정하세요. 기존 입력은 변경하지 않았습니다.",
    };
  }

  const selected = Array.from(new Set(selectedPorts)).sort((left, right) => left - right);
  if (selected.some((port) => !Number.isSafeInteger(port) || port < 1 || port > 65535)) {
    return { nextText: null, error: "선택한 runtime 포트가 올바르지 않습니다." };
  }
  const existingSet = new Set(existing.ports);
  const additions = selected.filter((port) => !existingSet.has(port));
  if (existing.ports.length + additions.length > MAX_EXPECTED_PORTS) {
    return {
      nextText: null,
      error: `예상 포트는 최대 ${MAX_EXPECTED_PORTS}개까지 등록할 수 있습니다.`,
    };
  }
  return { nextText: [...existing.ports, ...additions].join(", ") };
}

export function formatRuntimeFreshness(freshnessMs: number | null): string {
  if (freshnessMs === null || !Number.isFinite(freshnessMs) || freshnessMs < 0) return "시각 정보 없음";
  if (freshnessMs < 60_000) return `${Math.floor(freshnessMs / 1_000)}초 전`;
  if (freshnessMs < 60 * 60_000) return `${Math.floor(freshnessMs / 60_000)}분 전`;
  return `${Math.floor(freshnessMs / (60 * 60_000))}시간 전`;
}
