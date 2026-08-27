import type { OpenRequest, ProjectProfile } from "../api";

export type WorkbenchOpenAction =
  | { kind: "selectProfile"; profileId: string }
  | { kind: "draftProfile"; path: string; looksWindows: boolean }
  | { kind: "noop"; reason: string };

/**
 * Path comparison key. Windows paths are case-insensitive and use
 * backslashes; WSL/Linux paths are case-sensitive. Lower-casing both loses
 * that distinction, but this match answers "is this the same repo a caller
 * already knows about", not a filesystem operation — leniency here trades a
 * theoretical false positive between two real, differently-cased paths for
 * fewer missed matches from drive-letter casing (`C:` vs `c:`).
 */
function normalizedForMatch(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/u, "").toLowerCase();
}

function looksLikeWindowsPath(path: string): boolean {
  return /^[a-zA-Z]:[\\/]/.test(path) || path.includes("\\");
}

function profileMatchesPath(profile: ProjectProfile, path: string): boolean {
  const target = normalizedForMatch(path);
  if (profile.windowsPath && normalizedForMatch(profile.windowsPath) === target) return true;
  if (profile.wsl?.path && normalizedForMatch(profile.wsl.path) === target) return true;
  return false;
}

/**
 * Pure routing decision for an inbound open request
 * (`docs/superpowers/specs/2026-08-17-app-interop-design.md` §1.4). workbench
 * accepts `path` and its own opaque `profile` id. A path selects the matching
 * ProjectProfile or creates a bounded draft; a profile id is resolved only
 * against the current in-memory list. Other target kinds explicitly no-op.
 */
export function routeOpenRequest(request: OpenRequest, profiles: ProjectProfile[]): WorkbenchOpenAction {
  const target = request.target;
  switch (target.kind) {
    case "path": {
      const match = profiles.find((profile) => profileMatchesPath(profile, target.path));
      return match
        ? { kind: "selectProfile", profileId: match.id }
        : { kind: "draftProfile", path: target.path, looksWindows: looksLikeWindowsPath(target.path) };
    }
    case "profile": {
      const match = profiles.find((profile) => profile.id === target.id);
      return match
        ? { kind: "selectProfile", profileId: match.id }
        : { kind: "noop", reason: "workbench profile을 현재 저장소에서 찾을 수 없다" };
    }
    case "workspace":
    case "query":
    case "task":
    case "install":
      return { kind: "noop", reason: `workbench는 "${target.kind}" 타깃을 받지 않는다 (설계 §1.4)` };
  }
}
