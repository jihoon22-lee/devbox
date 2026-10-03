import { describe, expect, it } from "vitest";
import { shiftActivityDate } from "./activityPresentation";
describe("activity calendar navigation", () => {
  it("moves timeline by days and clamps month end", () => {
    const timeline = shiftActivityDate(new Date(2026, 0, 31), "timeline", 1);
    expect([timeline.getMonth(), timeline.getDate()]).toEqual([1, 1]);
    const month = shiftActivityDate(new Date(2026, 0, 31), "month", 1);
    expect([month.getMonth(), month.getDate()]).toEqual([1, 28]);
    const leap = shiftActivityDate(new Date(2024, 0, 31), "month", 1);
    expect(leap.getDate()).toBe(29);
  });
});
