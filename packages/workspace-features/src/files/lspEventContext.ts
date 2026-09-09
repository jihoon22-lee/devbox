type Context = { projectId: string; worktreeId: string; revision: number; target: { kind: "windows" } | { kind: "wsl"; distroId: string } };

function context(value: unknown): Context | null {
  if (!value || typeof value !== "object") return null;
  const candidate = value as Partial<Context>;
  if (typeof candidate.projectId !== "string" || typeof candidate.worktreeId !== "string"
    || !Number.isSafeInteger(candidate.revision) || !candidate.target) return null;
  if (candidate.target.kind !== "windows"
    && !(candidate.target.kind === "wsl" && typeof candidate.target.distroId === "string")) return null;
  return candidate as Context;
}

/** Native events carry context identity; JSON property ordering is immaterial. */
export function matchesLspEventContext(contextKey: string, nativeContext: unknown): boolean {
  let selected: Context | null;
  try { selected = context(JSON.parse(contextKey)); } catch { return false; }
  const event = context(nativeContext);
  return Boolean(selected && event && selected.projectId === event.projectId
    && selected.worktreeId === event.worktreeId && selected.revision === event.revision
    && selected.target.kind === event.target.kind
    && (selected.target.kind !== "wsl" || (event.target.kind === "wsl" && selected.target.distroId === event.target.distroId)));
}
