import assert from "node:assert/strict";
import { readFile, writeFile, lstat } from "node:fs/promises";
import path from "node:path";
import { allWindowsProcesses, windowsLocalAppData } from "./windows-packaged-smoke.mjs";
import { captureWindowOwner, nativeWindowAction } from "./windows-user-flow-window.mjs";
import { readInstallerOperations } from "./windows-suite-installer-evidence.mjs";
import { boundedFailure } from "./user-flow-failure-evidence.mjs";

// A GUI-launched helper inherits stderr from its owned parent. Keep only whole
// fixed-format issue tokens; discard arbitrary output and oversized lines.
export function nativeIssueCollector() {
  const codes = [];
  let line = "";
  let oversized = false;
  return {
    codes,
    write(chunk) {
      for (const character of String(chunk)) {
        if (character === "\n") {
          const code = line.replace(/\r$/u, "");
          if (!oversized && /^(?:bootstrap|suite|update|restore|checkpoint|data)_[a-z0-9_]{1,56}$/u.test(code)) {
            if (codes.length < 16 && !codes.includes(code)) codes.push(code);
          }
          line = "";
          oversized = false;
        } else if (line.length < 80 && !oversized) line += character;
        else oversized = true;
      }
    },
  };
}

export function reviewedHelperCodes(observation) {
  if (!["Devbox 데이터 복구", "Devbox 작업 시작 실패"].includes(observation?.name)) return [];
  return [
    ...new Set(
      (observation.controls ?? []).slice(0, 64).flatMap((control) => {
        const match =
          /^(?:작업을 완료하지 못했습니다|검토한 작업을 시작하지 못했습니다) \(((?:suite|update|bootstrap|checkpoint|restore|data)_[a-z0-9_]{1,56})\)\./mu.exec(
            control.name ?? "",
          );
        return match ? [match[1]] : [];
      }),
    ),
  ];
}

// Read only the retained helper and fixed-schema logs for this owned namespace.
export async function preserveReviewedCommitFailure(center, error, observationId) {
  assert.match(observationId, /^[a-f0-9-]{36}$/u);
  const owner = JSON.parse(await readFile(path.join(center.root, "suite-owner.json"), "utf8"));
  assert.match(owner.payloadRevision, /^[a-f0-9]{64}$/u);
  const image = path.join(center.root, "setup", owner.payloadRevision, "devbox-suite-bootstrap.exe");
  const helpers = allWindowsProcesses().filter((item) => item.Path.toLowerCase() === image.toLowerCase());
  assert.ok(helpers.length <= 4, "Unexpected retained helper count");
  const issues = [];
  for (const helper of helpers) {
    try {
      issues.push({
        codes: reviewedHelperCodes(
          nativeWindowAction(captureWindowOwner(helper, path.dirname(center.root)), "Inspect"),
        ),
      });
    } catch {
      issues.push({ observationUnavailable: true });
    }
  }
  let operations;
  try {
    operations = await readInstallerOperations(windowsLocalAppData(), center.installationKey);
  } catch {
    operations = null;
  }
  let activationPhase = null;
  try {
    const activation = JSON.parse(await readFile(path.join(center.root, "devbox-activation.json"), "utf8"));
    if (["recover", "import", "health", "committed"].includes(activation.phase)) activationPhase = activation.phase;
  } catch {}
  const updateBlocked = await lstat(path.join(center.root, "suite-update.block"))
    .then(() => true)
    .catch((issue) => (issue.code === "ENOENT" ? false : null));
  const restoreBlocked = await lstat(path.join(center.root, "suite-data-restore.block"))
    .then(() => true)
    .catch((issue) => (issue.code === "ENOENT" ? false : null));
  await writeFile(
    `product-foundation-evidence/reviewed-commit-${observationId}.json`,
    JSON.stringify(
      {
        schemaVersion: 1,
        sourceSha: center.sourceSha,
        runnerSourceSha: process.env.GITHUB_SHA,
        installationKey: center.installationKey,
        status: "FAIL",
        error: boundedFailure(error),
        helpers: issues,
        ownedProcessIssues: center.nativeIssueCodes ?? [],
        operations,
        activationPhase,
        updateBlocked,
        restoreBlocked,
      },
      null,
      2,
    ),
    { flag: "wx" },
  );
}
