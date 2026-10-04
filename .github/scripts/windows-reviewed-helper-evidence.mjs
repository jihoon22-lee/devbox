import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { allWindowsProcesses, windowsLocalAppData } from "./windows-packaged-smoke.mjs";
import { captureWindowOwner, nativeWindowAction } from "./windows-user-flow-window.mjs";
import { readInstallerOperations } from "./windows-suite-installer-evidence.mjs";
import { boundedFailure } from "./user-flow-failure-evidence.mjs";

export function reviewedHelperCodes(observation) {
  if (observation?.name !== "Devbox 데이터 복구") return [];
  return [
    ...new Set(
      (observation.controls ?? []).slice(0, 64).flatMap((control) => {
        const match =
          /^작업을 완료하지 못했습니다 \(((?:suite|update|bootstrap|checkpoint|restore|data)_[a-z0-9_]{1,56})\)\./mu.exec(
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
        operations,
      },
      null,
      2,
    ),
    { flag: "wx" },
  );
}
