import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

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
