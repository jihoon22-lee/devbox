#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";

const LIMIT = 1000;
const allow = JSON.parse(readFileSync(new URL("./component-size-allowlist.json", import.meta.url), "utf8"));
const git = (...args) => execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
const files = [
  ...new Set(git("ls-files", "--cached", "--others", "--exclude-standard", "--", "*.tsx").split("\n")),
].filter((file) => file && !file.includes(".test.") && !file.includes(".spec."));
const lines = (text) => text.split("\n").length;
const problems = [];
for (const file of files) {
  const count = lines(readFileSync(file, "utf8"));
  const ceiling = allow[file] ?? LIMIT;
  if (count > ceiling) problems.push(`${file}: ${count} lines (limit ${ceiling})`);
}
for (const [file, ceiling] of Object.entries(allow)) {
  if (!Number.isSafeInteger(ceiling) || ceiling <= LIMIT || !files.includes(file))
    problems.push(`${file}: invalid or unnecessary allowlist entry`);
}
// A PR may reduce an old exception, never expand it or exempt a new large view.
const base = process.env.GITHUB_BASE_REF ? `origin/${process.env.GITHUB_BASE_REF}` : "origin/main";
let baseSha;
try {
  baseSha = git("rev-parse", "--verify", base).trim();
} catch {
  problems.push(`Missing size baseline: ${base}`);
}
if (baseSha) {
  let previous = {};
  let firstAdoption = false;
  try {
    previous = JSON.parse(git("show", `${baseSha}:.github/scripts/component-size-allowlist.json`));
  } catch {
    firstAdoption = true;
  }
  for (const [file, ceiling] of Object.entries(allow)) {
    let baseline = previous[file];
    if (baseline === undefined) {
      baseline = LIMIT;
      if (firstAdoption) {
        try {
          baseline = lines(git("show", `${baseSha}:${file}`));
        } catch {
          /* New files have no exception. */
        }
      }
    }
    if (ceiling > Math.max(LIMIT, baseline)) problems.push(`${file}: allowance grew from ${baseline} to ${ceiling}`);
  }
}
if (problems.length) {
  console.error(problems.join("\n"));
  process.exitCode = 1;
} else console.log("Component sizes OK");
