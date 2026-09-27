import { expect, it } from "vitest";
import { formatBytes, formatCpu, formatTokens } from "./format";
it("formats binary memory, core percentages and Korean token totals", () => {
  expect(formatBytes(384 * 4096)).toBe("1.5 MB");
  expect(formatBytes(3 * 1024 ** 3)).toBe("3.0 GB");
  expect(formatCpu(null)).toBe("측정 중");
  expect(formatCpu(150)).toBe("150%");
  expect(formatTokens(1_234_567)).toBe("123만");
  expect(formatTokens(999)).toBe("999");
});
