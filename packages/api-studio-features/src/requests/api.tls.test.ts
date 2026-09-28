import { beforeEach, expect, it, vi } from "vitest";
import { sendRequest, startSseStream, startWebSocket } from "./api";
import { emptyRequest } from "./lib/importers";
import { safeRequestError, safeWebSocketUiError } from "./lib/requestPresentation";
const mocks = vi.hoisted(() => ({ call: vi.fn(), native: vi.fn(() => true) }));
vi.mock("../calls", () => ({ apiCall: mocks.call }));
vi.mock("./lib/isTauri", () => ({ isTauri: mocks.native }));
beforeEach(() => {
  mocks.call.mockReset();
  mocks.native.mockReturnValue(true);
});
it("rejects custom TLS in browser preview before fetch", async () => {
  mocks.native.mockReturnValue(false);
  const req = { ...emptyRequest(), tls: { credentialId: null, verify: false } };
  await expect(sendRequest(req, [])).rejects.toMatchObject({ name: "tls_native_required" });
  expect(mocks.call).not.toHaveBeenCalled();
});
it("rejects HTTP-only TLS settings before starting either stream transport", async () => {
  const req = { ...emptyRequest(), tls: { credentialId: "a".repeat(32), verify: true } };
  for (const native of [true, false]) {
    mocks.native.mockReturnValue(native);
    await expect(
      startSseStream(
        req,
        [],
        { connectTimeoutMs: 1000, idleTimeoutMs: 1000, totalTimeoutMs: 5000, reconnect: false },
        vi.fn(),
      ),
    ).rejects.toMatchObject({ name: "tls_http_only" });
    await expect(startWebSocket(req, [], vi.fn())).rejects.toMatchObject({ name: "tls_http_only" });
  }
  expect(mocks.call).not.toHaveBeenCalled();
});
it("projects fixed TLS codes without exposing raw native errors", () => {
  const error = Object.assign(new Error("private-path"), { name: "tls_credential_missing" });
  expect(safeRequestError(error)).toContain("TLS 자격 증명");
  expect(safeRequestError(error)).not.toContain("private");
  expect(safeWebSocketUiError(Object.assign(new Error("private"), { name: "tls_http_only" }))).toContain("HTTP");
});
