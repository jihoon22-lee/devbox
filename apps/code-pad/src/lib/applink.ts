import type { OpenRequest } from "../types";

/** Emitted by an already-running instance when it is relaunched with argv
 * (`docs/superpowers/specs/2026-08-17-app-interop-design.md` §3). Cold start
 * uses the `take_pending_open` pull instead, but both converge on
 * `routeOpenRequest` below. */
export const APPLINK_OPEN_EVENT = "devbox://open";

export type CodePadOpenAction =
  | { kind: "openFile"; path: string; line: number | null; column: number | null }
  | { kind: "openWorkspace"; path: string }
  | { kind: "noop"; reason: string };

/**
 * Pure routing decision for an inbound open request. Code Pad accepts `path`
 * (+ line/column) and `workspace` (§1.4 of the design doc). `profile` and
 * `query` are targets other apps accept, not Code Pad — an explicit no-op
 * with a reason, never a silent drop, so a newer sender talking to this app
 * degrades to "app opens normally" (§1.3) instead of doing nothing
 * unexplained.
 */
export function routeOpenRequest(request: OpenRequest): CodePadOpenAction {
  const target = request.target;
  switch (target.kind) {
    case "path":
      return { kind: "openFile", path: target.path, line: target.line, column: target.column };
    case "workspace":
      return { kind: "openWorkspace", path: target.path };
    case "profile":
    case "query":
    case "task":
    case "install":
      return { kind: "noop", reason: `code-pad는 "${target.kind}" 타깃을 받지 않는다 (설계 §1.4)` };
  }
}
