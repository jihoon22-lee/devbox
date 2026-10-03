import { expect, it, vi } from "vitest";
vi.mock("@devbox/product-shell/api", () => ({ nativeMode: false }));
import { collectorMessage } from "./collectorStatus";
it("describes actual ownership and never interprets unavailable as portable", () => {
  expect(collectorMessage({ owner: "installedAgent", tracking: true, consent: true })).toContain("수집이 계속");
  expect(collectorMessage({ owner: "installedAgent", tracking: false, consent: false })).toContain("일시중지");
  expect(collectorMessage({ owner: "portableLocal", tracking: true, consent: true })).toContain("중지됩니다");
  expect(collectorMessage({ owner: "installedAgent", tracking: null, consent: null })).toContain("확인하지 못했습니다");
  expect(collectorMessage(null)).not.toContain("portable");
});
