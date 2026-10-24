import { expect, test } from "vitest";
import type { Doc } from "./machine";
import { claimed, clean, d1, diskRev, opened, run, types } from "./machine-test-kit";

// Every state that holds unsaved edits: a disk change is recorded for comparison, never applied.
const d2 = diskRev("d2");

/** Conflict "mine" reclaimed next to another window's seq 7: Recovering over our unsaved buffer. */
function recoveringMine(): Doc {
  const conflict = run(clean(), { type: "edit", text: "mine" }, { type: "external", rev: d2 }).doc;
  const journal = { seq: 7, baseRev: d1, text: "B kept", deferred: false };
  const back = run(
    conflict,
    { type: "reconnected" },
    { type: "claimed", claimed: claimed({ leaseId: "L9", rev: d2, text: "outside", journal }) },
  ).doc;
  expect(back).toMatchObject({ state: "Recovering", text: "mine" });
  return back;
}

test("recovering_with_unsaved_edits_records_a_disk_change_without_touching_the_buffer", () => {
  const back = recoveringMine();
  const resynced = run(back, { type: "resync" }, { type: "diskLoaded", rev: d2, text: "outside" });
  expect(resynced.doc).toMatchObject({ state: "Recovering", text: "mine", diskRev: d1 });
  expect(resynced.doc.disk).toEqual({ rev: d2, text: "outside" });
  expect(types(resynced.effects)).toEqual(["readDisk"]);
  // Keep mine → the save still uses the old base, so the daemon answers conflict → compare again.
  const kept = run(resynced.doc, { type: "discard" }, { type: "save" });
  expect(kept.doc).toMatchObject({ state: "Saving", text: "mine" });
  expect(kept.effects).toContainEqual(expect.objectContaining({ type: "save", baseRev: d1, text: "mine" }));
  const compared = run(
    kept.doc,
    { type: "saveFailed", code: "documents.conflict", rev: d2 },
    { type: "diskLoaded", rev: d2, text: "outside" },
  );
  expect(compared.doc).toMatchObject({ state: "Conflict", text: "mine", disk: { rev: d2, text: "outside" } });
});

const unsavedStates: [string, () => Doc][] = [
  ["Dirty", () => run(clean(), { type: "edit", text: "mine" }).doc],
  ["Saving", () => run(clean(), { type: "edit", text: "mine" }, { type: "saveTick" }).doc],
  ["Conflict", () => run(clean(), { type: "edit", text: "mine" }, { type: "external", rev: diskRev("d3") }).doc],
  ["Orphaned", () => run(clean(), { type: "edit", text: "mine" }, { type: "external", rev: null }).doc],
  ["Recovering", recoveringMine],
  ["ReadOnly", () => run(clean(), { type: "edit", text: "mine" }, { type: "leaseLost", reason: "taken" }).doc],
];

test.each(unsavedStates)("%s_keeps_unsaved_edits_through_external_disk_and_resync", (_state, make) => {
  for (const rev of [d2, null]) {
    const doc = make();
    const step = run(
      doc,
      { type: "external", rev },
      { type: "resync" },
      { type: "diskLoaded", rev, text: rev && "outside" },
    );
    expect(step.doc.text).toBe("mine");
    expect(step.doc.editSeq).toBe(doc.editSeq);
    expect(types(step.effects)).not.toContain("applyText");
    expect(types(step.effects)).not.toContain("journalDrop");
  }
});

test("reopen_with_two_different_drafts_offers_both", () => {
  const journal = { seq: 4, baseRev: d1, text: "server draft", deferred: false };
  const offline = { kind: "draft" as const, seq: 6, baseRev: d1, text: "offline draft" };
  const step = run(opened({ journal }).doc);
  expect(step.doc.recovery?.other).toBeUndefined();
  const both = opened({ journal }, undefined, offline).doc;
  expect(both).toMatchObject({
    state: "Recovering",
    recovery: { seq: 6, text: "offline draft", other: { seq: 4, text: "server draft" } },
  });
  expect(run(both, { type: "restore", which: "other" }).doc).toMatchObject({ state: "Dirty", text: "server draft" });
  expect(run(both, { type: "restore" }).doc).toMatchObject({ state: "Dirty", text: "offline draft" });
});
