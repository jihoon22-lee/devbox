function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * 탭 제목 v1 규칙: distro 이름을 그대로 쓰고, 이미 같은 이름(또는 "이름 N" 패턴)을
 * 쓰는 탭이 있으면 다음 번호를 붙인다. 탭은 항상 첫 팬과 함께 생성되므로(App.tsx의
 * startInTab), distro는 탭 생성 시점에 이미 확정돼 있다 — 제목은 그때 한 번만 계산한다.
 */
export function nextTabTitle(existingTitles: string[], distro: string): string {
  const pattern = new RegExp(`^${escapeRegExp(distro)}( \\d+)?$`);
  const count = existingTitles.filter((t) => pattern.test(t)).length;
  return count === 0 ? distro : `${distro} ${count + 1}`;
}
