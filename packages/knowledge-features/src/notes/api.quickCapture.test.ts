import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ readText: vi.fn() }));
vi.mock("./lib/isTauri", () => ({ isTauri: () => true }));

import { saveQuickCapture } from "./api";

describe("quick capture IPC error boundary", () => {
  beforeEach(() => invokeMock.mockReset());

  it("preserves an allowlisted string rejection from the Tauri runtime", async () => {
    invokeMock.mockRejectedValueOnce("빠른 캡처 미리보기가 오래되어 다시 확인하세요");

    await expect(saveQuickCapture("qc-1")).rejects.toThrow(
      "빠른 캡처 미리보기가 오래되어 다시 확인하세요",
    );
  });

  it("redacts an unexpected native string instead of reflecting it", async () => {
    invokeMock.mockRejectedValueOnce("C:/private/token-value.md");

    await expect(saveQuickCapture("qc-1")).rejects.toThrow("빠른 캡처를 저장하지 못했습니다");
  });
});
