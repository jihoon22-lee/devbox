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
