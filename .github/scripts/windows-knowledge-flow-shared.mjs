export async function scenarios(context, specifications) {
  const { sourceSha, fixtureSha, artifactDigests, ui, knowledgeFixture: fixture } = context;
  const results = [];
  for (const [id, exercise] of specifications) {
    const screenshots = [],
      assertions = [];
    const record = (status, failureCode = null) => ({
      id,
      status,
      sourceSha,
      fixtureSha,
      artifactDigests,
      evidenceKind: "packaged-ui",
      assertions,
      screenshotPaths: screenshots,
      failureCode,
    });
    if (
      process.platform !== "win32" ||
      process.env.GITHUB_ACTIONS !== "true" ||
      process.env.RUNNER_ENVIRONMENT !== "github-hosted" ||
      !ui ||
      fixture?.kind !== "owned-installed-windows" ||
      !fixture.ownerVerified
    ) {
      assertions.push("Owned committed installation, real input driver and native observations required");
      results.push(record("NOT_RUN", "knowledge-owned-fixture-required"));
      continue;
    }
    try {
      await exercise({ ...context, fixture, screenshots, assertions });
      results.push(record("PASS"));
    } catch (error) {
      assertions.push(`Failure: ${String(error?.message ?? "unknown").slice(0, 300)}`);
      assertions.push("Scenario did not satisfy its real UI/native assertions; first failure retained");
      try {
        screenshots.push(await ui.screenshot(`${id}-failure`));
      } catch {}
      results.push(record("FAIL", `${id.toLowerCase()}-assertion-failed`));
    }
  }
  return results;
}
export const editor = { role: "textbox", name: "Markdown 본문" };
