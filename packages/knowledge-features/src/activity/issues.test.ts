import { describe, expect, it } from "vitest";
import { activityMessages } from "./issues";

describe("activity issue catalog", () => {
  it("gives every declared code a non-empty Korean message", () => {
    for (const [code, message] of Object.entries(activityMessages))
      expect(message.trim().length, code).toBeGreaterThan(0);
    expect(activityMessages.unavailable).toBeDefined();
  });
});
