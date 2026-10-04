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
// These fixtures contain only a few visible lines. AX textbox serialization
// adds a final visual newline; read actual rendered lines without trimming.
export function visibleNoteTextExpression() {
  return '(()=>{const editors=document.querySelectorAll(".cm-content");if(editors.length!==1)throw new Error("Unique synthetic note editor unavailable");const lines=editors[0].querySelectorAll(".cm-line");if(!lines.length)throw new Error("Synthetic note lines unavailable");return Array.from(lines,line=>line.textContent).join("\\n");})()';
}
export function noteReadyExpression(path, content) {
  return `document.querySelector('.editor-head .path')?.textContent===${JSON.stringify(path)} && (${visibleNoteTextExpression()})===${JSON.stringify(content)}`;
}
