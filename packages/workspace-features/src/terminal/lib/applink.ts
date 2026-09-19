import type { OpenRequest } from "../types";

export type WslDesktopOpenAction =
  | { kind: "openTerminal"; path: string }
  | { kind: "openProfile"; id: string }
  | { kind: "noop"; reason: string };

/**
 * Pure routing decision for an inbound open request
 * (`docs/superpowers/specs/2026-08-17-app-interop-design.md` §1.4).
 * wsl-desktop accepts `path` — a new terminal opens with that path as cwd.
 * `profile` resolves a WSL Desktop-owned named terminal workspace. `workspace`/`query`
 * are targets other apps accept, not this one.
 */
export function routeOpenRequest(request: OpenRequest): WslDesktopOpenAction {
  const target = request.target;
  switch (target.kind) {
    case "path":
      return { kind: "openTerminal", path: target.path };
    case "profile":
      return { kind: "openProfile", id: target.id };
    case "workspace":
    case "query":
    case "task":
    case "install":
      return { kind: "noop", reason: `wsl-desktop은 "${target.kind}" 타깃을 받지 않는다 (설계 §1.4)` };
  }
}
