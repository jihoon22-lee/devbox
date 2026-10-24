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

/** Stored token shapes match the daemon raw-body guard; see raw-storage-token-cases.json. */
export function redactKnownTokenPatterns(value: string): string {
  if (/-----BEGIN [A-Z ]*PRIVATE KEY-----/.test(value)) return "[REDACTED]";
  const prefixed = value.replace(
    /(\{\{secret:[A-Za-z_][A-Za-z0-9_]{0,127}\}\})|(?:sk[_-]|ghp_|github_pat_|glpat-|xox[baprs]-)[A-Za-z0-9_.-]{12,}|AKIA[A-Z0-9]{16}/g,
    (match, reference: string | undefined) => (reference ? match : "[REDACTED]"),
  );
  // Scan maximal runs once; an unanchored JWT regex retries quadratically on long plain bodies.
  return prefixed.replace(/[A-Za-z0-9_.-]+/g, (word) =>
    /^[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}$/.test(word) ? "[REDACTED]" : word,
  );
}
