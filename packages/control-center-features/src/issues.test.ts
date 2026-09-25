import { expect, it } from "vitest";
import { deliveryMessages, toolsMessages } from "./issues";
it.each([
  ["tools", toolsMessages],
  ["delivery", deliveryMessages],
] as const)("%s translates declared native codes", (_, catalog) => {
  for (const [code, message] of Object.entries(catalog)) expect(message.trim(), code).not.toBe("");
  expect(catalog.unavailable).toBeTruthy();
});
