import type { ResponseRule, WebhookFixture } from "../../../rpc/gen/rpc";
/** A local draft only: received authorization and body never become response metadata. */
export function fixtureToRule(fixture: WebhookFixture): ResponseRule {
  return {
    id: "",
    priority: 0,
    method: fixture.request.received.method,
    path: fixture.request.received.path,
    status: 200,
    headers: [],
    body: "",
    delayMs: 0,
    sequence: [],
  };
}
