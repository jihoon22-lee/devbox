import { mkdir, writeFile, readdir, readFile, lstat } from "node:fs/promises";
import path from "node:path";

// Only the fixed-schema native log inside this synthetic installation is read.
// Arguments, paths, free-form messages and other products never enter evidence.
export async function readInstallerOperations(localData, installationKey) {
  if (!/^[a-f0-9]{64}$/.test(installationKey ?? "")) throw new Error("Invalid installation key");
  const namespace = path.join(localData, `com.devbox.v08.controlcenter.i${installationKey}`);
  const directory = path.join(namespace, "logs");
  for (const entry of [namespace, directory]) {
    const info = await lstat(entry);
    if (!info.isDirectory() || info.isSymbolicLink()) throw new Error("Linked operation log directory");
  }
  const names = (await readdir(directory))
    .filter((name) => /^operations-\d{4}-\d{2}-\d{2}\.jsonl$/.test(name))
    .sort()
    .slice(-2);
  const rows = [];
  for (const name of names) {
    const file = path.join(directory, name),
      info = await lstat(file);
    if (!info.isFile() || info.isSymbolicLink() || info.size > 4 * 1024 * 1024)
      throw new Error("Invalid operation log file");
    for (const line of (await readFile(file, "utf8")).split("\n")) {
      if (!line.trim()) continue;
      let row;
      try {
        row = JSON.parse(line);
      } catch {
        continue;
      }
      if (
        row.product !== "control-center" ||
        row.component !== "control-center.delivery" ||
        ![
          "open_setup_product",
          "restore_action",
          "restore_inventory",
          "record_suite_health",
          "suite_health",
          "suite_recovery",
        ].includes(row.method) ||
        !["succeeded", "failed", "cancelled", "rejected", "panicked", "limit"].includes(row.outcome) ||
        !Number.isSafeInteger(row.tsMs) ||
        !Number.isSafeInteger(row.durationMs)
      )
        continue;
      rows.push({
        method: row.method,
        outcome: row.outcome,
        code: typeof row.code === "string" && /^[a-zA-Z0-9_.:-]{1,64}$/.test(row.code) ? row.code : null,
        tsMs: row.tsMs,
        durationMs: row.durationMs,
      });
    }
  }
  return rows.slice(-20);
}

export async function writeInstallerFailure(
  { stage, error, identity, results, observation },
  evidenceRoot = "product-foundation-evidence",
) {
  await mkdir(evidenceRoot, { recursive: true });
  await writeFile(
    path.join(evidenceRoot, "interactive-installer-failure.json"),
    JSON.stringify(
      {
        schemaVersion: 1,
        stage,
        status: "FAIL",
        ...identity,
        runnerSourceSha: process.env.GITHUB_SHA,
        runnerRunId: process.env.GITHUB_RUN_ID,
        error: {
          name: error.name ?? "Error",
          message: String(error.message ?? error).slice(0, 2000),
          stack: String(error.stack ?? "").slice(0, 6000),
        },
        results,
        observation,
      },
      null,
      2,
    ),
    { flag: "wx" },
  );
}
export async function reportInstallerResults(results, report) {
  try {
    await report(results);
  } catch (reportError) {
    const failure = results.find((result) => result.status === "FAIL");
    if (!failure) throw reportError;
    throw new Error(
      `Interactive installer failed: ${failure.assertions[0]}; evidence reporting also failed: ${reportError.message}`,
      { cause: reportError },
    );
  }
}
