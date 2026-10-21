import { expect, test } from "vitest";
import { claimed, clean, d1, diskRev, drops, key, opened, run, types } from "./machine-test-kit";

// Two windows on one document: leases move with [take here], journals belong to the holder.

test("read_only_close_never_touches_the_holders_journals", () => {
  const lost = run(
    clean(),
    { type: "edit", text: "mine" },
    { type: "journalTick" },
    { type: "leaseLost", reason: "taken" },
  ).doc;
  expect(lost.state).toBe("ReadOnly");
  // Unsaved edits cannot be kept or saved while another window edits: only [take here] or [discard].
  for (const choice of [undefined, "keep", "save"] as const) {
    const asked = run(lost, { type: "close", choice });
    expect(asked.doc).toMatchObject({ state: "ReadOnly", text: "mine", closeRequested: true });
    expect(asked.effects).toEqual([]);
  }
  const discarded = run(lost, { type: "close", choice: "discard" });
  expect(discarded.doc.state).toBe("Closed");
  expect(types(discarded.effects)).not.toContain("journalDrop");
  expect(types(discarded.effects)).not.toContain("offlineJournal");
  const binary = opened({ leaseId: null, readOnly: { reason: "binary" } }).doc;
  expect(run(binary, { type: "close" })).toEqual({ doc: expect.objectContaining({ state: "Closed" }), effects: [] });
});

test("take_offers_the_holders_journal_as_a_recovery_candidate", () => {
  // Opened read-only while B edits; B journals seq 3. [take here] must not hide B's draft.
  const readOnly = opened({ leaseId: null, text: "disk", readOnly: { reason: "leased_elsewhere" } }).doc;
  const journal = { seq: 3, baseRev: d1, text: "B123", deferred: false };
  const took = run(
    readOnly,
    { type: "take" },
    { type: "claimed", claimed: claimed({ leaseId: "L2", text: "disk", journal }) },
  );
  expect(took.doc).toMatchObject({ state: "Recovering", text: "disk", recovery: { seq: 3, text: "B123" } });
  const restored = run(took.doc, { type: "restore" }, { type: "journalTick" });
  expect(restored.effects).toContainEqual(
    expect.objectContaining({ type: "journal", leaseId: "L2", seq: 4, text: "B123" }),
  );
  const deferred = run(
    readOnly,
    { type: "take" },
    {
      type: "claimed",
      claimed: claimed({ leaseId: "L2", journal: { ...journal, text: null, deferred: true } }),
    },
  );
  expect(deferred.doc.state).toBe("Recovering");
  expect(deferred.effects).toContainEqual({ type: "journalRead", leaseId: "L2" });
});

test("take_with_unsaved_edits_compares_before_overwriting_either_draft", () => {
  const lost = run(
    clean(),
    { type: "edit", text: "mine" },
    { type: "journalTick" },
    { type: "leaseLost", reason: "taken" },
  ).doc;
  const journal = { seq: 5, baseRev: d1, text: "theirs", deferred: false };
  const took = run(lost, { type: "take" }, { type: "claimed", claimed: claimed({ leaseId: "L3", journal }) });
  expect(took.doc).toMatchObject({ state: "Recovering", text: "mine", recovery: { seq: 5, text: "theirs" } });
  expect(types(took.effects)).not.toContain("applyText");
  expect(types(took.effects)).not.toContain("journal");
  expect(types(took.effects)).not.toContain("save");
  // Keep mine: their journal goes (with our lease) and ours is written above it.
  const mine = run(took.doc, { type: "discard" }, { type: "journalTick" });
  expect(mine.doc).toMatchObject({ state: "Dirty", text: "mine" });
  expect(drops(mine.effects)).toEqual([{ type: "journalDrop", key, uptoSeq: 5, leaseId: "L3" }]);
  expect(mine.effects).toContainEqual(expect.objectContaining({ type: "journal", leaseId: "L3", text: "mine" }));
  // Take theirs: an undoable replacement of the buffer.
  const theirs = run(took.doc, { type: "restore" });
  expect(theirs.doc).toMatchObject({ state: "Dirty", text: "theirs" });
  expect(theirs.effects).toContainEqual({ type: "applyText", text: "theirs", undoable: true });
});

/** Dirty "mine" loses the lease; [take here] finds the other window's seq 5 "theirs". */
function takenOverWithMine() {
  const lost = run(
    clean(),
    { type: "edit", text: "mine" },
    { type: "journalTick" },
    { type: "leaseLost", reason: "taken" },
  );
  const journal = { seq: 5, baseRev: d1, text: "theirs", deferred: false };
  return run(lost.doc, { type: "take" }, { type: "claimed", claimed: claimed({ leaseId: "L3", journal }) });
}

test("recovering_with_unsaved_edits_keeps_an_offline_copy_of_the_buffer", () => {
  const took = takenOverWithMine();
  expect(took.doc).toMatchObject({ state: "Recovering", text: "mine" });
  // The daemon journal now holds the other window's draft, so ours must survive a crash here.
  expect(took.effects).toContainEqual({
    type: "offlineJournal",
    key,
    seq: took.doc.editSeq,
    baseRev: d1,
    text: "mine",
  });
  expect(took.doc.journalSeq).toBe(took.doc.editSeq);
});

test("recovering_with_unsaved_edits_asks_before_closing_and_never_drops_one_side_silently", () => {
  const took = takenOverWithMine().doc;
  for (const choice of [undefined, "keep", "save", "discard"] as const) {
    const asked = run(took, { type: "close", choice });
    expect(asked.doc).toMatchObject({ state: "Recovering", text: "mine", closeRequested: true });
    expect(asked.effects).toEqual([]);
  }
  // Only the explicit "discard both drafts" choice closes, dropping theirs and our offline copy.
  const all = run(took, { type: "close", choice: "discardAll" });
  expect(all.doc.state).toBe("Closed");
  expect(all.effects).toEqual([
    { type: "journalDrop", key, uptoSeq: took.editSeq, leaseId: "L3" },
    { type: "release", leaseId: "L3" },
  ]);
  // Keeping ours drops only their journal (seq 5); our offline copy stays until a save covers it.
  const mine = run(took, { type: "discard" }, { type: "journalTick" });
  expect(drops(mine.effects)).toEqual([{ type: "journalDrop", key, uptoSeq: 5, leaseId: "L3" }]);
  expect(mine.effects).toContainEqual(
    expect.objectContaining({ type: "journal", leaseId: "L3", seq: took.editSeq + 1, text: "mine" }),
  );
});

test("conflict_reclaim_offers_a_newer_foreign_journal_instead_of_writing_over_it", () => {
  const conflict = run(clean(), { type: "edit", text: "mine" }, { type: "external", rev: diskRev("d2") }).doc;
  expect(conflict.state).toBe("Conflict");
  const journal = { seq: 7, baseRev: d1, text: "B kept", deferred: false };
  const back = run(
    conflict,
    { type: "reconnected" },
    { type: "claimed", claimed: claimed({ leaseId: "L9", journal }) },
  );
  expect(back.doc).toMatchObject({ state: "Recovering", text: "mine", recovery: { seq: 7, text: "B kept" } });
  expect(types(run(back.doc, { type: "journalTick" }).effects)).not.toContain("journal");
  // After keeping ours the disk conflict is still known: the next save compares again.
  const mine = run(back.doc, { type: "discard" }, { type: "save" });
  expect(mine.effects).toContainEqual(expect.objectContaining({ type: "save", baseRev: d1, text: "mine" }));
});
