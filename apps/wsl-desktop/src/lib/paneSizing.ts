/** 팬 크기 비율. 합이 1이 되도록 정규화된 값만 저장·사용한다. */
export const MIN_PANE_FRACTION = 0.1;

function clampFraction(value: number): number {
  if (!Number.isFinite(value) || value <= 0) return MIN_PANE_FRACTION;
  return value;
}

export function evenFractions(count: number): number[] {
  if (count <= 0) return [];
  return Array.from({ length: count }, () => 1 / count);
}

/**
 * 저장된 비율을 현재 팬 수에 맞춘다. 길이가 다르거나 값이 손상됐으면 균등 분할로
 * 되돌린다 — 잘못된 비율이 팬 하나를 0에 가깝게 만들어 셸 화면을 망가뜨리지 않게 한다.
 */
export function normalizeFractions(fractions: readonly number[] | undefined, count: number): number[] {
  if (count <= 0) return [];
  if (!fractions || fractions.length !== count) return evenFractions(count);
  const clamped = fractions.map(clampFraction);
  const total = clamped.reduce((sum, value) => sum + value, 0);
  if (!Number.isFinite(total) || total <= 0) return evenFractions(count);
  const scaled = clamped.map((value) => value / total);
  return scaled.some((value) => value < MIN_PANE_FRACTION) ? evenFractions(count) : scaled;
}

/**
 * 두 이웃 팬 사이의 구분선을 끌었을 때의 새 비율. 두 팬의 합은 유지하고 각각은
 * 최소 비율 아래로 내려가지 않는다.
 */
export function resizeAdjacent(
  fractions: readonly number[],
  index: number,
  deltaFraction: number,
): number[] {
  if (index < 0 || index + 1 >= fractions.length) return [...fractions];
  const pair = fractions[index] + fractions[index + 1];
  const minimum = Math.min(MIN_PANE_FRACTION, pair / 2);
  const first = Math.min(pair - minimum, Math.max(minimum, fractions[index] + deltaFraction));
  const next = [...fractions];
  next[index] = first;
  next[index + 1] = pair - first;
  return next;
}

/** CSS grid template 문자열. 0 또는 1개 팬에서는 단일 트랙이다. */
export function toGridTemplate(fractions: readonly number[]): string {
  if (fractions.length === 0) return "1fr";
  return fractions.map((value) => `${(value * 1000).toFixed(0)}fr`).join(" ");
}
