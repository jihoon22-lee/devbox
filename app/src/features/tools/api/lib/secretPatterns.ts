export function looksLikeSecret(value: string): boolean {
  return (
    /(?:sk[_-]|ghp_|github_pat_|glpat-|xox[bprsa]-)[A-Za-z0-9_.-]{12,}/u.test(value) ||
    /^AKIA[A-Z0-9]{16}$/u.test(value)
  );
}

export function isSensitiveName(name: string): boolean {
  return /(authorization|proxy-authorization|cookie|set-cookie|api[-_]?key|access[-_]?token|refresh[-_]?token|token|secret|password|passwd|private[-_]?key)/i.test(
    name,
  );
}
