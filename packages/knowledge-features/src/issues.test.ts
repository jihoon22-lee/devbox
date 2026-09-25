import { describe, expect, it } from "vitest";
import { notesMessages } from "./notes/issues";
import { searchMessages } from "./search/issues";
import { setupMessages } from "./setup/issues";
import { commandsMessages } from "./commands/issues";
describe("Knowledge issue catalogs", () => {
  it.each([
    ["notes", notesMessages],
    ["search", searchMessages],
    ["setup", setupMessages],
    ["commands", commandsMessages],
  ] as const)("%s has a message for every native code", (_, catalog) => {
    for (const [code, message] of Object.entries(catalog)) expect(message.trim(), code).not.toBe("");
    expect(catalog.unavailable).toBeTruthy();
  });
});
