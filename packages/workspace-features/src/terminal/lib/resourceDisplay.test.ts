import { describe, expect, it } from "vitest";
import { formatResourceBytes, formatResourcePair, resourceSummaryLabel } from "./resourceDisplay";

describe("resource display", () => {
  it("formats bounded byte values without exposing raw command output", () => {
    expect(formatResourceBytes(1024)).toBe("1.0 KiB");
    expect(formatResourceBytes(1024 * 1024 * 2)).toBe("2.0 MiB");
    expect(formatResourceBytes(-1)).toBe("—");
    expect(formatResourceBytes(Number.MAX_SAFE_INTEGER + 1)).toBe("—");
  });

  it("rejects invalid used/total pairs", () => {
    expect(formatResourcePair(11, 10)).toBe("—");
    expect(formatResourcePair(1, 0)).toBe("—");
    expect(formatResourcePair(1, 2)).toContain("/ 2 B");
  });

  it("labels a missing resource snapshot explicitly", () => {
    expect(resourceSummaryLabel(null)).toBe("리소스 조회 안 함");
  });

  it("shows first-sample CPU as unavailable instead of a false percentage", () => {
    expect(resourceSummaryLabel({
      cpuPercent: null,
      memoryUsedBytes: 1,
      memoryTotalBytes: 2,
      diskUsedBytes: 1,
      diskTotalBytes: 2,
    })).toContain("CPU —");
  });
});
