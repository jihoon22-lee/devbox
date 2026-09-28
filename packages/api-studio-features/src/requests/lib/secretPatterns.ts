export function looksLikeSecret(value: string): boolean {
  return (
    /(?:sk[_-]|ghp_|github_pat_|glpat-|xox[bprsa]-)[A-Za-z0-9_.-]{12,}/u.test(value) ||
    /^AKIA[A-Z0-9]{16}$/u.test(value)
  );
}
