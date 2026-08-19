import type { OpenRequest } from "../types";

export type WslDesktopOpenAction =
  | { kind: "openTerminal"; path: string }
  | { kind: "noop"; reason: string };

/**
 * Pure routing decision for an inbound open request
 * (`docs/superpowers/specs/2026-08-17-app-interop-design.md` §1.4).
 * wsl-desktop accepts `path` — a new terminal opens with that path as cwd.
 * `profile` is a real target for this app per §1.4, but the workspace/layout
 * feature it needs does not exist yet (v0.5.0,
 * `docs/superpowers/specs/2026-08-17-wsl-desktop-terminal-design.md` §4.4) —
 * an explicit no-op, not a silent drop. `workspace`/`query` are targets other
 * apps accept, not this one.
 */
export function routeOpenRequest(request: OpenRequest): WslDesktopOpenAction {
  const target = request.target;
  switch (target.kind) {
    case "path":
      return { kind: "openTerminal", path: target.path };
    case "profile":
      return {
        kind: "noop",
        reason:
          "profile 타깃은 프로필 워크스페이스/레이아웃 기능이 필요하다 — v0.5.0 예정 " +
          "(docs/superpowers/specs/2026-08-17-wsl-desktop-terminal-design.md §4.4), 아직 미구현",
      };
    case "workspace":
    case "query":
      return { kind: "noop", reason: `wsl-desktop은 "${target.kind}" 타깃을 받지 않는다 (설계 §1.4)` };
  }
}
