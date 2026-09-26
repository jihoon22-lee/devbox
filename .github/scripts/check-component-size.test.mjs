import { execFileSync, spawnSync } from "node:child_process";
import { copyFileSync, mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { strict as assert } from "node:assert";
import { test } from "node:test";

test("component exceptions can shrink but cannot grow or cover new views", () => {
  const root = mkdtempSync(join(tmpdir(), "devbox-component-size-"));
  const git = (...args) => execFileSync("git", args, { cwd: root, stdio: "pipe" });
  const write = (file, value) => writeFileSync(join(root, file), value);
  const allow = (value) => write(".github/scripts/component-size-allowlist.json", JSON.stringify(value));
  const check = () =>
    spawnSync(process.execPath, [".github/scripts/check-component-size.mjs"], {
      cwd: root,
      encoding: "utf8",
      env: { ...process.env, GITHUB_BASE_REF: "main" },
    });
  try {
    mkdirSync(join(root, ".github/scripts"), { recursive: true });
    copyFileSync(
      new URL("./check-component-size.mjs", import.meta.url),
      join(root, ".github/scripts/check-component-size.mjs"),
    );
    git("init", "-b", "main");
    git("config", "user.name", "Fixture");
    git("config", "user.email", "fixture@example.invalid");
    write("Existing.tsx", "\n".repeat(1200));
    git("add", ".");
    git("commit", "-m", "fixture");
    git("update-ref", "refs/remotes/origin/main", "HEAD");
    allow({ "Existing.tsx": 1201 });
    assert.equal(check().status, 0, "first adoption allows existing size");
    allow({ "Existing.tsx": 1202 });
    assert.match(check().stderr, /allowance grew/);
    allow({ "Existing.tsx": 1201 });
    git("add", ".");
    git("commit", "-m", "adopt limit");
    git("update-ref", "refs/remotes/origin/main", "HEAD");
    write("Existing.tsx", "\n".repeat(1100));
    allow({ "Existing.tsx": 1101 });
    assert.equal(check().status, 0, "existing exception can shrink");
    write("New.tsx", "\n".repeat(1100));
    assert.match(check().stderr, /New.tsx: 1101 lines/);
    allow({ "Existing.tsx": 1101, "New.tsx": 1101 });
    assert.match(check().stderr, /New.tsx: allowance grew/);
    rmSync(join(root, "New.tsx"));
    allow({ "Existing.tsx": 1101 });
    git("update-ref", "-d", "refs/remotes/origin/main");
    assert.match(check().stderr, /Missing size baseline/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
