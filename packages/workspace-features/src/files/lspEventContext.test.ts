import { expect, it } from "vitest";
import { matchesLspEventContext } from "./lspEventContext";

it("accepts reordered native fields and rejects late, missing and cross-target events", () => {
  const selected = { projectId: "project", worktreeId: "worktree", revision: 2, target: { kind: "wsl", distroId: "Ubuntu" } };
  const key = JSON.stringify(selected);
  expect(matchesLspEventContext(key, { target: selected.target, revision: 2, worktreeId: "worktree", projectId: "project" })).toBe(true);
  for (const event of [undefined, null, {}, { ...selected, revision: 1 }, { ...selected, worktreeId: "other" },
    { ...selected, projectId: "other" }, { ...selected, target: { kind: "windows" } },
    { ...selected, target: { kind: "wsl", distroId: "Debian" } }]) expect(matchesLspEventContext(key, event)).toBe(false);
  expect(matchesLspEventContext("standalone", selected)).toBe(false);
});
