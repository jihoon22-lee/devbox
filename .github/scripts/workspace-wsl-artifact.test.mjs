import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { binary, run } from "./workspace-wsl-artifact.mjs";

test("helper staging rejects a foreign source, changed bytes and dynamic ELF", () => {
  const root = mkdtempSync(path.join(tmpdir(), "workspace-helper-artifact-"));
  try {
    const source = path.join(root, "source");
    const destination = path.join(root, "resources"); mkdirSync(destination);
    const elf = Buffer.alloc(120);
    elf.write("7f454c460201", "hex"); elf.writeUInt16LE(62, 18);
    elf.writeBigUInt64LE(64n, 32); elf.writeUInt16LE(56, 54); elf.writeUInt16LE(1, 56);
    writeFileSync(source, elf);
    run("prepare", [source, "a".repeat(40)], destination);
    run("verify", ["a".repeat(40)], destination);
    assert.throws(() => run("verify", ["b".repeat(40)], destination), /source differs/);
    const file = path.join(destination, "devbox-workspace-wsl");
    const changed = readFileSync(file); changed[119] = 1; writeFileSync(file, changed);
    assert.throws(() => run("verify", [], destination));
    elf.writeUInt32LE(3, 64); writeFileSync(source, elf);
    assert.throws(() => binary(source), /dynamic interpreter/);
    elf.writeUInt32LE(0, 64); elf.writeBigUInt64LE(10000n, 32); writeFileSync(source, elf);
    assert.throws(() => binary(source), /program headers/);
  } finally { rmSync(root, { recursive: true }); }
});
