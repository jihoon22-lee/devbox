import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  buildWifiPayload,
  generateQr,
  MAX_PAYLOAD_BYTES,
  MAX_OUTPUT_SIZE,
  QR_ERROR_MESSAGES,
  type GenerateQrRequest,
} from "./qr";

const canvasContext = {
  fillStyle: "",
  fillRect: vi.fn(),
} as unknown as CanvasRenderingContext2D;

function request(overrides: Partial<GenerateQrRequest> = {}): GenerateQrRequest {
  return {
    preset: "text",
    text: "https://example.com/devbox",
    version: 3,
    errorCorrection: "M",
    size: 256,
    quietZone: 4,
    ...overrides,
  };
}

beforeEach(() => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(canvasContext);
  vi.spyOn(HTMLCanvasElement.prototype, "toDataURL").mockReturnValue("data:image/png;base64,cG5n");
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("generateQr", () => {
  it("produces deterministic SVG/PNG metadata without reflecting payload text", async () => {
    const first = await generateQr(request());
    const second = await generateQr(request());

    expect(first).toEqual(second);
    expect(first.version).toBe(3);
    expect(first.width).toBe(222);
    expect(first.svg).toContain("shape-rendering=\"crispEdges\"");
    expect(first.svg).not.toContain("example.com");
    expect(first.pngBase64).toBe("cG5n");
    expect(first.payloadBytes).toBe(new TextEncoder().encode(request().text).length);
  });

  it("formats Wi-Fi fields with the standard escaped payload", () => {
    expect(buildWifiPayload({
      ssid: "dev;box",
      password: String.raw`p\;,:`,
      security: "WPA",
      hidden: true,
    })).toBe(String.raw`WIFI:T:WPA;S:dev\;box;P:p\\\;\,\:;H:true;;`);
    expect(buildWifiPayload({
      ssid: "open",
      password: "unexpected",
      security: "nopass",
      hidden: false,
    })).toBeNull();
  });

  it("rejects invalid Unicode, options, and QR capacity with fixed errors", async () => {
    const invalidUnicode = request({ text: "\ud800" });
    await expect(generateQr(invalidUnicode)).rejects.toMatchObject({
      code: "invalidInput",
      message: QR_ERROR_MESSAGES.invalidInput,
    });

    await expect(generateQr(request({ version: 41 as never }))).rejects.toThrow(
      QR_ERROR_MESSAGES.invalidVersion,
    );
    await expect(generateQr(request({ size: 63 }))).rejects.toThrow(
      QR_ERROR_MESSAGES.invalidSize,
    );
    await expect(generateQr(request({ version: 1, text: "x".repeat(MAX_PAYLOAD_BYTES) }))).rejects.toThrow(
      QR_ERROR_MESSAGES.capacity,
    );
    await expect(generateQr(request({ text: "secret", version: 1, errorCorrection: "H" }))).resolves.toBeTruthy();
    await expect(generateQr(request({ text: "", errorCorrection: "X" as never }))).rejects.toThrow(
      QR_ERROR_MESSAGES.emptyInput,
    );
    await expect(generateQr(request({ preset: "unknown" as never, size: 1 }))).rejects.toThrow(
      QR_ERROR_MESSAGES.invalidInput,
    );
    await expect(generateQr(request({
      preset: "wifi",
      wifi: { ssid: 42 } as never,
    }))).rejects.toThrow(QR_ERROR_MESSAGES.invalidWifi);
  });

  it("keeps the rendered dimension bounded and rejects oversized image output", async () => {
    const result = await generateQr(request({ version: null, text: "bounded", size: MAX_OUTPUT_SIZE }));
    expect(result.width).toBeGreaterThan(0);
    expect(result.width).toBeLessThanOrEqual(MAX_OUTPUT_SIZE);

    vi.spyOn(HTMLCanvasElement.prototype, "toDataURL")
      .mockReturnValue(`data:image/png;base64,${"A".repeat(5_592_409)}`);
    await expect(generateQr(request())).rejects.toMatchObject({
      code: "render",
      message: QR_ERROR_MESSAGES.render,
    });
  });

  it("fails closed when the browser has no canvas renderer", async () => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
    await expect(generateQr(request())).rejects.toMatchObject({
      code: "render",
      message: QR_ERROR_MESSAGES.render,
    });
  });
});
