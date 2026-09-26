import { describe, expect, it } from "vitest";
import { apiMessages, webhookMessages, transformMessages, storeMessages } from "./catalog";
describe("API Studio native issue catalogs", () => {
  it.each([
    ["api", apiMessages],
    ["webhooks", webhookMessages],
    ["transforms", transformMessages],
    ["store", storeMessages],
  ] as const)("%s has a translation for every declared code", (_, catalog) => {
    for (const [code, message] of Object.entries(catalog)) expect(message.trim(), code).not.toBe("");
    expect(catalog.unavailable).toBe("작업을 완료하지 못했습니다.");
  });
});
