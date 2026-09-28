import { beforeEach, expect, it, vi } from "vitest";
import { sendRequest, revealCapture, discardCaptures, restoreCaptures } from "./api";
import { emptyRequest } from "./lib/importers";
const mocks = vi.hoisted(() => ({ call: vi.fn(), native: vi.fn(() => true) }));
vi.mock("../calls", () => ({ apiCall: mocks.call }));
vi.mock("./lib/isTauri", () => ({ isTauri: mocks.native }));
beforeEach(() => {
  mocks.call.mockReset().mockResolvedValue({});
  mocks.native.mockReturnValue(true);
});
it("sends capture definitions with the existing request and scoped reference commands", async () => {
  const captures = [{ id: "c", enabled: true, variable: "token", source: "jsonPath" as const, target: "$.token" }];
  const request = emptyRequest();
  await sendRequest(request, [], undefined, captures);
  expect(mocks.call).toHaveBeenCalledWith("send_request", {
    req: request,
    environment: [],
    requestId: expect.any(String),
    captures,
  });
  mocks.call.mockResolvedValueOnce("explicit-value");
  await expect(revealCapture("ref")).resolves.toBe("explicit-value");
  await discardCaptures(["ref"]);
  await restoreCaptures(["ref"]);
  expect(mocks.call.mock.calls.slice(-3)).toEqual([
    ["reveal_capture", { reference: "ref" }],
    ["discard_captures", { references: ["ref"] }],
    ["restore_captures", { references: ["ref"] }],
  ]);
});
it("does not send reference commands from browser preview", async () => {
  mocks.native.mockReturnValue(false);
  await expect(revealCapture("ref")).rejects.toThrow();
  await discardCaptures(["ref"]);
  await restoreCaptures(["ref"]);
  expect(mocks.call).not.toHaveBeenCalled();
});
