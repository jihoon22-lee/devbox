import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { copyFileSync, lstatSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const directory = fileURLToPath(new URL("../../apps/devbox-workspace/src-tauri/resources/wsl/", import.meta.url));
const name = "devbox-workspace-wsl";
const limit = 64 * 1024 * 1024;
export function binary(file) {
  const stat = lstatSync(file);
  assert.ok(stat.isFile() && !stat.isSymbolicLink() && stat.size >= 64 && stat.size <= limit, "invalid helper file");
  const bytes = readFileSync(file);
  assert.equal(bytes.subarray(0, 4).toString("hex"), "7f454c46", "helper must be ELF");
  assert.equal(bytes[4], 2, "helper must be 64-bit");
  assert.equal(bytes[5], 1, "helper must be little endian");
  assert.equal(bytes.readUInt16LE(18), 62, "helper must be x86-64");
  const offset = Number(bytes.readBigUInt64LE(32));
  const size = bytes.readUInt16LE(54), count = bytes.readUInt16LE(56);
  assert.ok(Number.isSafeInteger(offset) && size >= 56 && count > 0 && count < 1024 && offset + size * count <= bytes.length, "invalid ELF program headers");
  for (let index = 0; index < count; index++) assert.notEqual(bytes.readUInt32LE(offset + size * index), 3, "helper must not require a dynamic interpreter");
  return { bytes: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") };
}
export function run(mode, args, destination = directory) {
if (mode === "prepare") {
  assert.equal(args.length, 2);
  const [source, sourceSha] = args;
  assert.match(sourceSha, /^[0-9a-f]{40}$/);
  const info = binary(source);
  copyFileSync(source, path.join(destination, name));
  writeFileSync(path.join(destination, "manifest.json"), JSON.stringify({ schemaVersion: 1, protocol: 1, target: "x86_64-unknown-linux-musl", sourceSha, ...info }, null, 2) + "\n");
} else {
  assert.equal(mode, "verify", "expected prepare or verify");
  assert.ok(args.length <= 1);
  const file = path.join(destination, "manifest.json");
  const stat = lstatSync(file);
  assert.ok(stat.isFile() && !stat.isSymbolicLink() && stat.size <= 4096);
  const manifest = JSON.parse(readFileSync(file, "utf8"));
  assert.deepEqual(Object.keys(manifest).sort(), ["schemaVersion", "protocol", "target", "sourceSha", "bytes", "sha256"].sort());
  assert.equal(manifest.schemaVersion, 1);
  assert.equal(manifest.protocol, 1);
  assert.equal(manifest.target, "x86_64-unknown-linux-musl");
  assert.match(manifest.sourceSha, /^[0-9a-f]{40}$/);
  if (args[0]) assert.equal(manifest.sourceSha, args[0], "helper source differs from product source");
  assert.deepEqual(binary(path.join(destination, name)), { bytes: manifest.bytes, sha256: manifest.sha256 });
}
console.log(`Workspace WSL artifact ${mode}: PASS`);

}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) run(process.argv[2], process.argv.slice(3));
