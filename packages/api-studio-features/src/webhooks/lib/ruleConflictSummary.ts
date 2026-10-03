import type { ResponseRule } from "../api";

function compactRulePart(value: string, maxChars = 120): string {
  return value.length <= maxChars ? value : `${value.slice(0, maxChars - 1)}…`;
}

function formatRuleLabel(rule: ResponseRule | undefined, fallbackId: string): string {
  const method = compactRulePart(rule?.method ?? "*");
  const path = compactRulePart(rule?.path ?? "(경로 미상)");
  const id = compactRulePart(rule?.id || fallbackId);
  return `${method} ${path} [${id}]`;
}

export function buildRuleConflictSummary(
  candidate: ResponseRule,
  conflicts: readonly {
    existingRuleId: string;
    winnerRuleId: string;
  }[],
  knownRules: readonly ResponseRule[],
): string {
  const candidateLabel = formatRuleLabel(candidate, candidate.id);
  const lines = conflicts.slice(0, 5).map((conflict) => {
    const existing = knownRules.find((knownRule) => knownRule.id === conflict.existingRuleId);
    const winner =
      conflict.winnerRuleId === candidate.id
        ? candidate
        : knownRules.find((knownRule) => knownRule.id === conflict.winnerRuleId);
    return `${candidateLabel} ↔ ${formatRuleLabel(existing, conflict.existingRuleId)} · 적용: ${formatRuleLabel(winner, conflict.winnerRuleId)}`;
  });
  if (conflicts.length > lines.length) lines.push(`외 ${conflicts.length - lines.length}개 충돌`);
  return [`겹치는 응답 규칙 ${conflicts.length}개가 있습니다.`, ...lines, "우선순위를 확인하고 저장할까요?"].join("\n");
}
