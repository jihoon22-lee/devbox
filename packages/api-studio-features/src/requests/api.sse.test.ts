import { describe, expect, it, vi } from "vitest";
import { startSseStream } from "./api";
import type { RequestTemplate, SseOptions } from "./types";

vi.mock("./lib/isTauri", () => ({ isTauri: () => false }));

const options: SseOptions = {
  connectTimeoutMs: 1_000,
  idleTimeoutMs: 1_000,
  totalTimeoutMs: 5_000,
  reconnect: false,
};

function request(patch: Partial<RequestTemplate> = {}): RequestTemplate {
  return {
    method: "POST",
    url: "https://example.test/stream",
    headers: [],
    cookies: [],
    multipart: [],
    params: [],
    body_kind: "none",
    body: "",
    auth: null,
    timeout_ms: 5_000,
    ...patch,
  };
}

describe("browser SSE request boundary", () => {
  it("rejects multipart content on GET instead of silently dropping the body", async () => {
    await expect(startSseStream(request({
      method: "GET",
      body_kind: "multipart",
      multipart: [{
        kind: "text",
        name: "field",
        value: "value",
        file_path: "",
        file_name: "",
        content_type: "",
        enabled: true,
      }],
    }), [], options, vi.fn())).rejects.toThrow("GET SSE 요청에는 본문을 사용할 수 없습니다.");
  });

  it("rejects browser-only multipart features before starting a background fetch", async () => {
    await expect(startSseStream(request({
      body_kind: "multipart",
      multipart: [{
        kind: "file",
        name: "upload",
        value: "",
        file_path: "/private/fixture.txt",
        file_name: "fixture.txt",
        content_type: "",
        enabled: true,
      }],
    }), [], options, vi.fn())).rejects.toThrow(
      "SSE multipart 파일 전송은 데스크톱 앱에서만 사용할 수 있습니다.",
    );

    await expect(startSseStream(request({
      body_kind: "multipart",
      multipart: [{
        kind: "text",
        name: "field",
        value: "value",
        file_path: "",
        file_name: "",
        content_type: "text/plain",
        enabled: true,
      }],
    }), [], options, vi.fn())).rejects.toThrow(
      "SSE multipart part별 Content-Type은 데스크톱 앱에서만 사용할 수 있습니다.",
    );
  });
});
