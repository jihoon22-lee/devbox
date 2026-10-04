import { describe, expect, it } from "vitest";
import { RecoveryWriter } from "./recoveryWriter";
import type { RecoveryEntry } from "./api";
const entry = (content: string, time = 1): RecoveryEntry => ({
  path: "/a",
  content,
  baseHash: null,
  snapshotAtMs: time,
  encoding: { encodingKind: "utf8", bom: true },
  lineEnding: "crlf",
});
describe("RecoveryWriter", () => {
  it("persists an empty dirty snapshot with its encoding and line ending", async () => {
    const writes: RecoveryEntry[][] = [];
    const writer = new RecoveryWriter({
      load: async () => ({ entries: [], nativeRevision: "1" }),
      save: async (entries) => {
        writes.push(entries);
        return "2";
      },
      discard: async () => "3",
    });
    writer.update([entry("")]);
    await writer.flush();
    expect(writes).toEqual([[entry("")]]);
  });
  it("flushes the last generation when edits arrive during a write", async () => {
    let release!: () => void;
    const writes: RecoveryEntry[][] = [];
    const writer = new RecoveryWriter({
      load: async () => ({ entries: [], nativeRevision: "1" }),
      save: async (entries) => {
        writes.push(entries);
        if (writes.length === 1)
          await new Promise<void>((resolve) => {
            release = resolve;
          });
        return String(writes.length + 1);
      },
      discard: async () => "3",
    });
    writer.update([entry("first")]);
    const flush = writer.flush();
    await new Promise((resolve) => setTimeout(resolve, 0));
    writer.update([entry("second", 2)]);
    release();
    await flush;
    expect(writes.map((entries) => entries[0].content)).toEqual(["first", "second"]);
  });
  it("does not overwrite a newer same-document recovery on CAS conflict", async () => {
    let loads = 0;
    const writer = new RecoveryWriter({
      load: async () => ({ entries: ++loads === 1 ? [] : [entry("external", 10)], nativeRevision: String(loads) }),
      save: async () => {
        throw new Error("revision_mismatch");
      },
      discard: async () => "3",
    });
    writer.update([entry("mine", 2)]);
    await expect(writer.flush()).rejects.toThrow("다른");
  });
});

describe("failed saved-document removal", () => {
  it("retries failed removal on a clean close and keeps rejecting while storage fails", async () => {
    let unavailable = true;
    let stored = [entry("old draft")];
    let removals = 0;
    const writer = new RecoveryWriter({
      load: async () => ({ entries: stored, nativeRevision: "1" }),
      save: async () => {
        throw new Error("clean close must not save a draft");
      },
      discard: async (path, revision) => {
        removals++;
        expect(revision).toBe("1");
        if (unavailable) throw new Error("journal unavailable");
        stored = stored.filter((item) => item.path !== path);
        return "2";
      },
    });
    await expect(writer.discard("/a")).rejects.toThrow("journal unavailable");
    writer.update([]);
    await expect(writer.flush()).rejects.toThrow("journal unavailable");
    unavailable = false;
    await writer.flush();
    expect(stored).toEqual([]);
    expect(removals).toBe(3);
    await writer.flush();
    expect(removals).toBe(3);
  });
  it("persists a new edit after retrying the prior saved snapshot removal", async () => {
    let unavailable = true;
    let stored = [entry("old draft")];
    const writer = new RecoveryWriter({
      load: async () => ({ entries: stored, nativeRevision: "1" }),
      save: async (entries, revision) => {
        expect(revision).toBe("2");
        stored = entries;
        return "3";
      },
      discard: async () => {
        if (unavailable) throw new Error("journal unavailable");
        stored = [];
        return "2";
      },
    });
    await expect(writer.discard("/a")).rejects.toThrow();
    writer.update([entry("new edit", 2)]);
    unavailable = false;
    await writer.flush();
    expect(stored).toEqual([entry("new edit", 2)]);
  });
});
