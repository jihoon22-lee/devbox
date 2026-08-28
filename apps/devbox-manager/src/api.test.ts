import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, isTauriMock, openUrlMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  isTauriMock: vi.fn(),
  openUrlMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: openUrlMock }));
vi.mock("./lib/isTauri", () => ({ isTauri: isTauriMock }));

import {
  installRelatedTool,
  launchRelatedTool,
  openRelatedToolUrl,
  relatedTools,
} from "./api";

beforeEach(() => {
  invokeMock.mockReset();
  openUrlMock.mockReset();
  isTauriMock.mockReset().mockReturnValue(false);
});

describe("Related Tools API boundary", () => {
  it("rejects tampered catalog metadata and impossible platform state", async () => {
    const valid = await relatedTools();
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce(valid.map((tool, index) => index === 0
      ? { ...tool, officialUrl: "https://evil.example/download" }
      : tool));
    await expect(relatedTools()).rejects.toThrow("관련 도구 감지 응답이 올바르지 않습니다.");

    invokeMock.mockResolvedValueOnce(valid.map((tool, index) => index === 0
      ? { ...tool, platformSupported: false, installed: true, detection: "path" }
      : tool));
    await expect(relatedTools()).rejects.toThrow("관련 도구 감지 응답이 올바르지 않습니다.");
  });

  it("accepts only the exact requested action and replaces native message text", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce({
      toolId: "vs-code",
      status: "installed",
      message: "C:\\Users\\developer\\secret-token",
    });
    await expect(installRelatedTool("vs-code", true)).resolves.toEqual({
      toolId: "vs-code",
      status: "installed",
      message: "WinGet 설치가 완료되었습니다.",
    });
    expect(invokeMock).toHaveBeenCalledWith("install_related_tool", {
      request: { toolId: "vs-code", confirmed: true },
    });

    invokeMock.mockResolvedValueOnce({ toolId: "another-tool", status: "launched" });
    await expect(launchRelatedTool("vs-code"))
      .rejects.toThrow("관련 도구 작업 결과가 올바르지 않습니다.");
  });

  it("rejects arbitrary ids and external URLs before a native call", async () => {
    isTauriMock.mockReturnValue(true);
    await expect(installRelatedTool("vs-code --silent", true))
      .rejects.toThrow("관련 도구 식별자가 올바르지 않습니다.");
    await expect(openRelatedToolUrl("https://evil.example/tool"))
      .rejects.toThrow("공식 링크가 올바르지 않습니다.");
    expect(invokeMock).not.toHaveBeenCalled();
    expect(openUrlMock).not.toHaveBeenCalled();
  });
});
