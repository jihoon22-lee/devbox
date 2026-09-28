import { beforeEach, expect, it, vi } from "vitest";
import { sendRequest, oauth2Status, authorizeOAuth2, cancelOAuth2, fetchOAuth2Token, clearOAuth2Token } from "./api";
import { emptyRequest } from "./lib/importers";
const mocks = vi.hoisted(() => ({ call: vi.fn(), native: vi.fn(() => true) }));
vi.mock("../calls", () => ({ apiCall: mocks.call }));
vi.mock("./lib/isTauri", () => ({ isTauri: mocks.native }));
beforeEach(() => {
  mocks.call.mockReset().mockResolvedValue({ state: "missing", expiresAtMs: null, scope: null });
  mocks.native.mockReturnValue(true);
});
it("keeps OAuth calls typed and cancellation scoped to its request", async () => {
  const auth = { ...emptyRequest().auth!, kind: "oauth2" };
  await oauth2Status(auth, []);
  await authorizeOAuth2("login", auth, []);
  await cancelOAuth2("login");
  await fetchOAuth2Token(auth, []);
  await clearOAuth2Token(auth, []);
  expect(mocks.call.mock.calls).toEqual([
    ["oauth2_status", { auth, environment: [] }],
    ["authorize_oauth2", { requestId: "login", auth, environment: [] }],
    ["cancel_oauth2", { requestId: "login" }],
    ["fetch_oauth2_token", { auth, environment: [] }],
    ["clear_oauth2_token", { auth, environment: [] }],
  ]);
});
it("rejects OAuth in browser preview instead of sending an anonymous request", async () => {
  mocks.native.mockReturnValue(false);
  const req = emptyRequest();
  req.auth = { ...req.auth!, kind: "oauth2" };
  await expect(sendRequest(req, [])).rejects.toThrow("데스크톱");
  await expect(oauth2Status(req.auth, [])).rejects.toThrow("데스크톱");
  expect(mocks.call).not.toHaveBeenCalled();
});
