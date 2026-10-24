import { expect, test } from "vitest";
import { closedDoc, reduce } from "./machine";
import {
  claimed,
  clean,
  code,
  d1,
  diskRev,
  drops,
  key,
  note,
  opened,
  run,
  types,
  withOffline,
} from "./machine-test-kit";

test("open_claims_then_loads_clean_without_a_journal", () => {
  const open = reduce(closedDoc("1", key, note), { type: "open", key, policy: note });
  expect(open.doc.state).toBe("Loading");
  expect(open.effects).toEqual([{ type: "claim", key, policy: note, take: false }]);
  const loaded = reduce(open.doc, { type: "claimed", claimed: claimed() });
  expect(loaded.doc).toMatchObject({ state: "Clean", leaseId: "L1", diskRev: d1, text: "a" });
  expect(loaded.effects).toEqual([]);
});

test("binary_or_leased_elsewhere_opens_read_only", () => {
  expect(opened({ leaseId: null, text: null, readOnly: { reason: "binary" } }).doc.state).toBe("ReadOnly");
  const elsewhere = opened({ leaseId: null, readOnly: { reason: "leased_elsewhere" } }).doc;
  expect(elsewhere).toMatchObject({ state: "ReadOnly", text: "a", readOnly: { reason: "leased_elsewhere" } });
  const taken = run(elsewhere, { type: "take" });
  expect(taken.effects).toContainEqual({ type: "claim", key, policy: note, take: true });
  expect(run(taken.doc, { type: "claimed", claimed: claimed({ leaseId: "L2" }) }).doc).toMatchObject({
    state: "Clean",
    leaseId: "L2",
  });
});

test("server_journal_goes_recovering_and_restore_applies_it_over_the_disk_rev", () => {
  const journal = { seq: 7, baseRev: diskRev("d0"), text: "draft", deferred: false };
  const recovering = opened({ journal }).doc;
  expect(recovering).toMatchObject({ state: "Recovering", text: "a", recovery: { seq: 7, text: "draft" } });
  const restored = run(recovering, { type: "restore" });
  expect(restored.doc).toMatchObject({ state: "Dirty", text: "draft", diskRev: d1 });
  expect(restored.doc.editSeq).toBeGreaterThan(7);
  expect(restored.effects).toContainEqual({ type: "applyText", text: "draft", undoable: true });
  expect(types(restored.effects)).toEqual(expect.arrayContaining(["scheduleJournal", "scheduleSave"]));
  const ticked = run(restored.doc, { type: "journalTick" });
  expect(ticked.effects).toContainEqual(expect.objectContaining({ type: "journal", seq: restored.doc.editSeq }));
});

test("deferred_server_journal_is_read_by_lease_before_restore", () => {
  const journal = { seq: 3, baseRev: d1, text: null, deferred: true };
  const step = opened({ journal });
  expect(step.doc.state).toBe("Recovering");
  expect(step.effects).toContainEqual({ type: "journalRead", leaseId: "L1" });
  expect(run(step.doc, { type: "restore" }).doc.state).toBe("Recovering");
  const loaded = run(step.doc, { type: "journalLoaded", seq: 3, baseRev: d1, text: "big draft" });
  expect(loaded.doc.recovery?.text).toBe("big draft");
  expect(run(loaded.doc, { type: "restore" }).doc).toMatchObject({ state: "Dirty", text: "big draft" });
});

test("offline_draft_that_differs_from_disk_is_a_recovery_candidate", () => {
  const draft = { kind: "draft" as const, seq: 4, baseRev: d1, text: "offline" };
  expect(opened({}, note, draft).doc).toMatchObject({ state: "Recovering", recovery: { seq: 4, text: "offline" } });
  const same = opened({}, note, { ...draft, text: "a" });
  expect(same.doc.state).toBe("Clean");
  expect(drops(same.effects)).toEqual([{ type: "journalDrop", key, uptoSeq: 4, leaseId: "L1" }]);
});

test("tombstone_suppresses_an_older_server_journal_and_reissues_the_drop", () => {
  const journal = { seq: 4, baseRev: d1, text: "dropped", deferred: false };
  const step = opened({ journal }, note, { kind: "tombstone", dropUpto: 4 });
  expect(step.doc.state).toBe("Clean");
  expect(drops(step.effects)).toEqual([{ type: "journalDrop", key, uptoSeq: 4, leaseId: "L1" }]);
  const edited = run(step.doc, { type: "edit", text: "ab" }, { type: "journalTick" });
  expect(edited.effects).toContainEqual(expect.objectContaining({ type: "journal", seq: 5 }));
});

test("clean_edit_schedules_journal_and_only_note_autosave", () => {
  const n = run(clean(), { type: "edit", text: "ab" });
  expect(n.doc).toMatchObject({ state: "Dirty", editSeq: 1, text: "ab" });
  expect(n.effects).toEqual([
    { type: "scheduleJournal", ms: 300 },
    { type: "scheduleSave", ms: 1000 },
  ]);
  const c = run(clean("a", code), { type: "edit", text: "ab" });
  expect(c.effects).toEqual([{ type: "scheduleJournal", ms: 300 }]);
  expect(run(c.doc, { type: "saveTick" }).doc.state).toBe("Dirty");
  expect(run(c.doc, { type: "save" }).doc.state).toBe("Saving");
});

test("journal_tick_writes_the_journal_with_lease_seq_and_base_rev", () => {
  const step = run(clean(), { type: "edit", text: "ab" }, { type: "journalTick" });
  expect(step.effects).toContainEqual({ type: "journal", leaseId: "L1", seq: 1, baseRev: d1, text: "ab" });
  expect(step.doc.state).toBe("Dirty");
  expect(step.doc.journalSeq).toBe(1);
  expect(run(step.doc, { type: "journalTick" }).effects).toEqual([]);
});

test("save_from_dirty_sends_cas_save_and_goes_saving", () => {
  const step = run(clean(), { type: "edit", text: "ab" }, { type: "saveTick" });
  expect(step.doc.state).toBe("Saving");
  expect(step.doc.inflight).toEqual({ seq: 1, baseRev: d1, expect: "present" });
  expect(step.effects).toContainEqual({
    type: "save",
    leaseId: "L1",
    seq: 1,
    baseRev: d1,
    expect: "present",
    text: "ab",
  });
});

test("own_save_echo_is_ignored_while_saving", () => {
  const d2 = diskRev("d2");
  const saving = run(clean(), { type: "edit", text: "ab" }, { type: "saveTick" }).doc;
  const held = run(saving, { type: "external", rev: { ...d2, mtimeNs: "9" } });
  expect(held.doc.state).toBe("Saving");
  expect(held.effects).toEqual([]);
  const done = run(held.doc, { type: "saveOk", rev: d2 });
  expect(done.doc).toMatchObject({ state: "Clean", diskRev: d2, savedSeq: 1, pendingExternal: [] });
  expect(types(done.effects)).not.toContain("readDisk");
  expect(drops(done.effects)).toEqual([{ type: "journalDrop", key, uptoSeq: 1, leaseId: "L1" }]);
});

test("edit_during_save_stays_dirty_with_new_base_rev", () => {
  const d2 = diskRev("d2");
  const saving = run(clean(), { type: "edit", text: "ab" }, { type: "saveTick" }).doc;
  const edited = run(saving, { type: "edit", text: "abc" });
  expect(edited.doc).toMatchObject({ state: "Saving", editSeq: 2, text: "abc" });
  const done = run(edited.doc, { type: "saveOk", rev: d2 });
  expect(done.doc).toMatchObject({ state: "Dirty", diskRev: d2, savedSeq: 1, inflight: undefined });
  expect(drops(done.effects)).toEqual([]);
  const next = run(done.doc, { type: "saveTick" });
  expect(next.effects).toContainEqual(expect.objectContaining({ type: "save", seq: 2, baseRev: d2, text: "abc" }));
});

test("save_conflict_keeps_buffer_and_journal_and_reads_the_disk", () => {
  let s = reduce(clean("a"), { type: "edit", text: "x" }).doc;
  s = reduce(s, { type: "saveTick" }).doc;
  const r = reduce(s, { type: "saveFailed", code: "documents.conflict", rev: diskRev("d2") });
  expect(r.doc.state).toBe("Conflict");
  expect(r.doc).toMatchObject({ text: "x", conflictRev: diskRev("d2"), inflight: undefined });
  expect(r.effects).not.toContainEqual(expect.objectContaining({ type: "journalDrop" }));
  expect(r.effects).toContainEqual({ type: "readDisk", key, policy: note });
  const shown = reduce(r.doc, { type: "diskLoaded", rev: diskRev("d2"), text: "theirs" });
  expect(shown.doc).toMatchObject({ state: "Conflict", text: "x", disk: { rev: diskRev("d2"), text: "theirs" } });
});

test("repeated_conflict_errors_never_overwrite_the_buffer_or_journal", () => {
  const d2 = diskRev("d2");
  const d3 = diskRev("d3");
  const dirty = run(clean(), { type: "edit", text: "mine" }, { type: "journalTick" }).doc;
  const first = run(dirty, { type: "saveTick" }, { type: "saveFailed", code: "documents.conflict", rev: d2 });
  const again = run(
    first.doc,
    { type: "diskLoaded", rev: d2, text: "theirs" },
    { type: "resolve", choice: "mine" },
    { type: "saveFailed", code: "documents.conflict", rev: d3 },
  );
  expect(again.doc).toMatchObject({ state: "Conflict", text: "mine", journalSeq: dirty.journalSeq, conflictRev: d3 });
  expect(drops([...first.effects, ...again.effects])).toEqual([]);
  // A different code is never treated as a conflict, whatever its message would say.
  const other = run(dirty, { type: "saveTick" }, { type: "saveFailed", code: "documents.too_large" });
  expect(other.doc).toMatchObject({ state: "Dirty", text: "mine", error: "documents.too_large" });
  expect(drops(other.effects)).toEqual([]);
});

test("io_error_keeps_dirty_and_retries_same_base_after_reconnect", () => {
  const saving = run(clean(), { type: "edit", text: "ab" }, { type: "saveTick" }).doc;
  const failed = run(saving, { type: "saveFailed", code: "connection_lost" });
  expect(failed.doc).toMatchObject({ state: "Dirty", text: "ab", retry: true, error: "connection_lost" });
  expect(failed.effects).toContainEqual({ type: "offlineJournal", key, seq: 1, baseRev: d1, text: "ab" });
  expect(failed.doc.journalSeq).toBe(1);
  // The lease died with the connection: reclaim first, then save over the same base.
  const back = run(failed.doc, { type: "reconnected" });
  expect(back.effects).toEqual([{ type: "claim", key, policy: note, take: false }]);
  const d9 = diskRev("d9");
  const reclaimed = run(back.doc, { type: "claimed", claimed: claimed({ leaseId: "L2", rev: d9, text: "disk" }) });
  expect(reclaimed.doc.text).toBe("ab");
  expect(reclaimed.effects).toContainEqual({
    type: "save",
    leaseId: "L2",
    seq: 1,
    baseRev: d1,
    expect: "present",
    text: "ab",
  });
  expect(run(saving, { type: "saveFailed", code: "internal" }).doc).toMatchObject({ state: "Dirty", retry: true });
});

test("journal_failure_while_offline_writes_the_offline_journal", () => {
  const dirty = run(clean(), { type: "edit", text: "ab" }).doc;
  const ticked = run(dirty, { type: "journalTick" }, { type: "journalFailed", seq: 1, code: "connection_lost" });
  expect(ticked.effects).toContainEqual({ type: "offlineJournal", key, seq: 1, baseRev: d1, text: "ab" });
  expect(
    run(dirty, { type: "journalTick" }, { type: "journalFailed", seq: 1, code: "documents.too_large" }).effects,
  ).not.toContainEqual(expect.objectContaining({ type: "offlineJournal" }));
});

test("external_change_while_clean_reloads_as_undoable_transaction", () => {
  const d2 = diskRev("d2");
  const step = run(clean(), { type: "external", rev: d2 });
  expect(step.effects).toEqual([{ type: "readDisk", key, policy: note }]);
  const loaded = run(step.doc, { type: "diskLoaded", rev: d2, text: "outside" });
  expect(loaded.doc).toMatchObject({ state: "Clean", diskRev: d2, text: "outside" });
  expect(loaded.effects).toEqual([{ type: "applyText", text: "outside", undoable: true }]);
  // A touch keeps the content hash: nothing to reload.
  expect(run(clean(), { type: "external", rev: { ...d1, mtimeNs: "5" } }).effects).toEqual([]);
});

test("external_change_while_dirty_goes_to_conflict", () => {
  const d2 = diskRev("d2");
  const step = run(clean(), { type: "edit", text: "mine" }, { type: "external", rev: d2 });
  expect(step.doc).toMatchObject({ state: "Conflict", text: "mine", conflictRev: d2 });
  expect(step.effects).toContainEqual({ type: "readDisk", key, policy: note });
  expect(drops(step.effects)).toEqual([]);
  // A Clean reload that arrives after a new edit is a conflict too, never an overwrite.
  const late = run(clean(), { type: "external", rev: d2 }, { type: "edit", text: "mine" });
  const raced = run(late.doc, { type: "diskLoaded", rev: d2, text: "outside" });
  expect(raced.doc).toMatchObject({ state: "Conflict", text: "mine" });
  expect(types(raced.effects)).not.toContain("applyText");
});

test("external_delete_goes_orphaned_and_save_recreates_with_expect_absent", () => {
  const fromClean = run(clean(), { type: "external", rev: null });
  expect(fromClean.doc).toMatchObject({ state: "Orphaned", text: "a", diskRev: null });
  const dirty = run(clean(), { type: "edit", text: "keep" }, { type: "external", rev: null });
  expect(dirty.doc).toMatchObject({ state: "Orphaned", text: "keep" });
  const saved = run(dirty.doc, { type: "save" });
  expect(saved.effects).toContainEqual({
    type: "save",
    leaseId: "L1",
    seq: 1,
    baseRev: null,
    expect: "absent",
    text: "keep",
  });
  const d2 = diskRev("d2");
  expect(run(saved.doc, { type: "saveOk", rev: d2 }).doc).toMatchObject({ state: "Clean", diskRev: d2 });
});

test("conflict_keep_mine_saves_with_the_disk_rev_as_base", () => {
  const d2 = diskRev("d2");
  const conflict = run(clean(), { type: "edit", text: "mine" }, { type: "external", rev: d2 }).doc;
  const mine = run(conflict, { type: "diskLoaded", rev: d2, text: "theirs" }, { type: "resolve", choice: "mine" });
  expect(mine.doc.state).toBe("Saving");
  expect(mine.effects).toContainEqual(expect.objectContaining({ type: "save", baseRev: d2, text: "mine" }));
});

test("conflict_take_disk_drops_the_journal", () => {
  const d2 = diskRev("d2");
  const dirty = run(clean(), { type: "edit", text: "mine" }, { type: "journalTick" }).doc;
  const conflict = run(dirty, { type: "external", rev: d2 }, { type: "diskLoaded", rev: d2, text: "theirs" }).doc;
  const disk = run(conflict, { type: "resolve", choice: "disk" });
  expect(disk.doc).toMatchObject({ state: "Clean", text: "theirs", diskRev: d2 });
  expect(disk.effects).toContainEqual({ type: "applyText", text: "theirs", undoable: true });
  expect(drops(disk.effects)).toEqual([{ type: "journalDrop", key, uptoSeq: conflict.journalSeq, leaseId: "L1" }]);
  // Reopen: the daemon reports no journal, so the document opens Clean (AR-F13).
  const closed = run(disk.doc, { type: "close" });
  expect(closed.doc.state).toBe("Closed");
  const reopened = run(closed.doc, { type: "open", key, policy: note }, withOffline(undefined, { rev: d2 }));
  expect(reopened.doc.state).toBe("Clean");
  expect(reopened.doc.state).not.toBe("Recovering");
});

test("conflict_merge_goes_dirty_over_the_disk_rev", () => {
  const d2 = diskRev("d2");
  const conflict = run(clean(), { type: "edit", text: "mine" }, { type: "external", rev: d2 }).doc;
  const merged = run(conflict, { type: "resolve", choice: "merged", text: "both" });
  expect(merged.doc).toMatchObject({ state: "Dirty", text: "both", diskRev: d2 });
  expect(merged.effects).toContainEqual({ type: "applyText", text: "both", undoable: true });
});

test("lease_lost_goes_read_only", () => {
  const dirty = run(clean(), { type: "edit", text: "mine" }).doc;
  const lost = run(dirty, { type: "leaseLost", reason: "taken" });
  expect(lost.doc).toMatchObject({ state: "ReadOnly", text: "mine", leaseId: undefined });
  expect(lost.doc.readOnly).toEqual({ reason: "leased_elsewhere" });
  expect(drops(lost.effects)).toEqual([]);
  const saving = run(dirty, { type: "saveTick" }).doc;
  expect(run(saving, { type: "saveFailed", code: "documents.lease_lost", reason: "taken" }).doc.state).toBe("ReadOnly");
  // Every state: Clean and Conflict lose the lease the same way and keep the buffer.
  expect(run(clean("disk"), { type: "leaseLost", reason: "taken" }).doc).toMatchObject({
    state: "ReadOnly",
    text: "disk",
  });
  const conflict = run(dirty, { type: "external", rev: diskRev("d2") }).doc;
  expect(conflict.state).toBe("Conflict");
  expect(run(conflict, { type: "leaseLost", reason: "taken" }).doc).toMatchObject({ state: "ReadOnly", text: "mine" });
});

test("expired_lease_reclaims_quietly", () => {
  const dirty = run(clean(), { type: "edit", text: "mine" }).doc;
  const expired = run(dirty, { type: "leaseLost", reason: "expired" });
  expect(expired.doc.state).toBe("Dirty");
  expect(expired.effects).toEqual([{ type: "claim", key, policy: note, take: false }]);
  const back = run(expired.doc, { type: "claimed", claimed: claimed({ leaseId: "L2" }) });
  expect(back.doc).toMatchObject({ state: "Dirty", leaseId: "L2", text: "mine" });
  // Another window holds it now: the reclaim answers read-only and the buffer stays.
  const elsewhere = run(expired.doc, {
    type: "claimed",
    claimed: claimed({ leaseId: null, readOnly: { reason: "leased_elsewhere" } }),
  });
  expect(elsewhere.doc).toMatchObject({ state: "ReadOnly", text: "mine" });
});

test("moved_event_rekeys_the_document", () => {
  const to = { root: "vault", path: "b.md" };
  const dirty = run(clean(), { type: "edit", text: "mine" }).doc;
  const moved = run(dirty, { type: "moved", to });
  expect(moved.doc).toMatchObject({ key: to, state: "Dirty", text: "mine", editSeq: 1 });
  expect(moved.effects).toEqual([]);
  const fromClean = run(clean(), { type: "moved", to });
  expect(fromClean.doc).toMatchObject({ key: to, state: "Clean", text: "a" });
  expect(fromClean.effects).toEqual([]);
  const saved = run(moved.doc, { type: "journalTick" }, { type: "close", choice: "discard" });
  expect(drops(saved.effects)).toEqual([{ type: "journalDrop", key: to, uptoSeq: 1, leaseId: "L1" }]);
});

test("close_dirty_requires_a_choice", () => {
  const dirty = run(clean(), { type: "edit", text: "mine" }, { type: "journalTick" }).doc;
  const asked = run(dirty, { type: "close" });
  expect(asked.doc).toMatchObject({ state: "Dirty", text: "mine", closeRequested: true });
  expect(asked.effects).toEqual([]);
  const discard = run(dirty, { type: "close", choice: "discard" });
  expect(discard.doc.state).toBe("Closed");
  expect(discard.effects).toEqual([
    { type: "journalDrop", key, uptoSeq: dirty.journalSeq, leaseId: "L1" },
    { type: "release", leaseId: "L1" },
  ]);
  const keep = run(dirty, { type: "close", choice: "keep" });
  expect(keep.doc.state).toBe("Closed");
  expect(drops(keep.effects)).toEqual([]);
  const save = run(dirty, { type: "close", choice: "save" });
  expect(save.doc.state).toBe("Saving");
  const done = run(save.doc, { type: "saveOk", rev: diskRev("d2") });
  expect(done.doc.state).toBe("Closed");
  expect(done.effects).toContainEqual({ type: "release", leaseId: "L1" });
  expect(run(clean(), { type: "close" }).doc.state).toBe("Closed");
});

test("recovering_discard_and_close_discard_drop_the_journal", () => {
  const journal = { seq: 6, baseRev: d1, text: "draft", deferred: false };
  const discarded = run(opened({ journal }).doc, { type: "discard" });
  expect(discarded.doc).toMatchObject({ state: "Clean", text: "a" });
  expect(drops(discarded.effects)).toEqual([{ type: "journalDrop", key, uptoSeq: 6, leaseId: "L1" }]);
  const dirty = run(clean(), { type: "edit", text: "x" }, { type: "journalTick" }).doc;
  expect(drops(run(dirty, { type: "close", choice: "discard" }).effects)).toEqual([
    { type: "journalDrop", key, uptoSeq: 1, leaseId: "L1" },
  ]);
});

test("offline_draft_seq_counts_toward_the_drop", () => {
  // Journal written at seq 1, then two more edits fail to save offline at seq 3.
  const journaled = run(clean(), { type: "edit", text: "x" }, { type: "journalTick" }).doc;
  const offline = run(
    journaled,
    { type: "edit", text: "xy" },
    { type: "edit", text: "xyz" },
    { type: "saveTick" },
    { type: "saveFailed", code: "connection_lost" },
  ).doc;
  expect(offline.journalSeq).toBe(3);
  expect(drops(run(offline, { type: "close", choice: "discard" }).effects)).toEqual([
    { type: "journalDrop", key, uptoSeq: 3, leaseId: "L1" },
  ]);
});

test("claimed_seeds_sequences_above_every_existing_journal", () => {
  const journal = { seq: 7, baseRev: d1, text: "a", deferred: false };
  const step = opened({ journal }, note, { kind: "draft", seq: 9, baseRev: d1, text: "a" });
  expect(step.doc.state).toBe("Clean");
  expect(step.doc).toMatchObject({ editSeq: 9, savedSeq: 9, journalSeq: 9 });
  expect(drops(step.effects)).toEqual([{ type: "journalDrop", key, uptoSeq: 9, leaseId: "L1" }]);
});

test("reclaim_continues_above_the_holders_journal_seq", () => {
  // A journals seq 1, B takes and journals the same text up to seq 5, A takes back and edits.
  const a = run(clean(), { type: "edit", text: "a1" }, { type: "journalTick" }, { type: "leaseLost", reason: "taken" });
  const journal = { seq: 5, baseRev: d1, text: "a1", deferred: false };
  const back = run(a.doc, { type: "take" }, { type: "claimed", claimed: claimed({ leaseId: "L3", journal }) });
  expect(back.doc).toMatchObject({ state: "Dirty", leaseId: "L3", text: "a1" });
  expect(back.doc.editSeq).toBeGreaterThan(5);
  const typed = run(back.doc, { type: "edit", text: "A-new" }, { type: "journalTick" });
  expect(typed.effects).toContainEqual(
    expect.objectContaining({ type: "journal", leaseId: "L3", seq: back.doc.editSeq + 1, text: "A-new" }),
  );
  // A clean reclaim moves every sequence past the daemon journal too.
  const cleanBack = run(
    clean(),
    { type: "reconnected" },
    { type: "claimed", claimed: claimed({ leaseId: "L2", journal: { ...journal, text: "a" } }) },
  );
  expect(cleanBack.doc).toMatchObject({ state: "Clean", editSeq: 5, savedSeq: 5, journalSeq: 5 });
});

test("stale_journal_keeps_the_edit_offline_and_reseeds_by_claim", () => {
  const dirty = run(clean(), { type: "edit", text: "ab" }, { type: "journalTick" }).doc;
  const stale = run(dirty, { type: "journalFailed", seq: 1, code: "documents.stale_journal" });
  expect(stale.effects).toContainEqual({ type: "offlineJournal", key, seq: 1, baseRev: d1, text: "ab" });
  expect(stale.effects).toContainEqual({ type: "claim", key, policy: note, take: false });
  const journal = { seq: 9, baseRev: d1, text: "ab", deferred: false };
  const reseeded = run(stale.doc, { type: "claimed", claimed: claimed({ journal }) });
  expect(reseeded.doc).toMatchObject({ state: "Dirty", text: "ab", editSeq: 10 });
  expect(run(reseeded.doc, { type: "journalTick" }).effects).toContainEqual(
    expect.objectContaining({ type: "journal", seq: 10, text: "ab" }),
  );
});

test("recovering_reclaim_rereads_a_deferred_journal", () => {
  const journal = { seq: 3, baseRev: d1, text: null, deferred: true };
  const recovering = opened({ journal }).doc;
  const back = run(
    recovering,
    { type: "reconnected" },
    { type: "claimed", claimed: claimed({ leaseId: "L2", journal }) },
  );
  expect(back.doc.state).toBe("Recovering");
  expect(back.effects).toContainEqual({ type: "journalRead", leaseId: "L2" });
});

test("saving_with_a_foreign_rev_reads_the_disk_after_save_ok", () => {
  const saving = run(clean(), { type: "edit", text: "ab" }, { type: "saveTick" }).doc;
  const step = run(saving, { type: "external", rev: diskRev("other") }, { type: "saveOk", rev: diskRev("d2") });
  expect(step.doc.state).toBe("Clean");
  expect(step.effects).toContainEqual({ type: "readDisk", key, policy: note });
});

test("orphaned_never_autosaves_and_loses_the_lease_like_any_state", () => {
  const orphaned = run(clean(), { type: "edit", text: "keep" }, { type: "external", rev: null }).doc;
  const ticked = run(orphaned, { type: "saveTick" });
  expect(ticked.doc.state).toBe("Orphaned");
  expect(types(ticked.effects)).not.toContain("save");
  expect(run(orphaned, { type: "leaseLost", reason: "taken" }).doc).toMatchObject({ state: "ReadOnly", text: "keep" });
  const recovering = opened({ journal: { seq: 2, baseRev: d1, text: "draft", deferred: false } }).doc;
  expect(run(recovering, { type: "leaseLost", reason: "taken" }).doc.state).toBe("ReadOnly");
});

test("orphaned_without_edits_closes_without_a_choice", () => {
  const orphaned = run(clean(), { type: "external", rev: null }).doc;
  const closed = run(orphaned, { type: "close" });
  expect(closed.doc.state).toBe("Closed");
  expect(closed.effects).toEqual([{ type: "release", leaseId: "L1" }]);
});

test("conflict_whose_disk_equals_the_buffer_resolves_clean", () => {
  // A save whose response was lost is retried over the old base: the disk already has our text.
  const d2 = diskRev("d2");
  const saving = run(clean(), { type: "edit", text: "mine" }, { type: "journalTick" }, { type: "saveTick" }).doc;
  const conflict = run(saving, { type: "saveFailed", code: "documents.conflict", rev: d2 }).doc;
  const same = run(conflict, { type: "diskLoaded", rev: d2, text: "mine" });
  expect(same.doc).toMatchObject({ state: "Clean", diskRev: d2, text: "mine" });
  expect(drops(same.effects)).toEqual([{ type: "journalDrop", key, uptoSeq: 1, leaseId: "L1" }]);
});
