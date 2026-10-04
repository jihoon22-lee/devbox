import assert from "node:assert/strict";
import { mkdtemp, writeFile } from "node:fs/promises";
import path from "node:path";
import { scenarios } from "./windows-knowledge-flow-shared.mjs";
export const scenarioIds = ["SEARCH-01", "SEARCH-02"];
export async function navigateSearch({ knowledgeFixture, ui }, target) {
  await knowledgeFixture.navigate("search");
  await ui.waitForTarget(target);
}
export async function searchRootRemovalTarget(roots, value) {
  // Native roots use normalized forward slashes, also after renderer reopen.
  const root = roots.find((row) => path.win32.resolve(row.path) === path.win32.resolve(value));
  assert.ok(root, "Owned search root missing before removal");
  return { role: "button", name: `${root.path} 루트 제거` };
}
export async function run(context) {
  return scenarios(context, [
    [
      "SEARCH-01",
      async ({ ui, fixture, fixtureRoot, assertions, screenshots }) => {
        const root = await mkdtemp(path.join(fixtureRoot, "knowledge-search-"));
        for (const name of ["foo.txt", "bar.md", "a---b.log", "한글.md"])
          await writeFile(path.join(root, name), "synthetic name-only fixture\n", { flag: "wx" });
        for (let index = 0; index < 496; index++)
          await writeFile(path.join(root, `perf-${String(index).padStart(3, "0")}.txt`), "synthetic indexed file\n", {
            flag: "wx",
          });
        await navigateSearch({ knowledgeFixture: fixture, ui }, { role: "textbox", name: "검색 루트 경로" });
        const beforeContent = (await fixture.contentStats()).indexed_files;
        await ui.fill({ role: "textbox", name: "검색 루트 경로" }, root);
        const indexStarted = performance.now();
        await ui.click({ role: "button", name: "추가" });
        await fixture.waitRoot(root);
        await fixture.wait(async () => {
          const status = await fixture.indexStats();
          return !status.indexing && status.indexed_files >= 500;
        }, "owned 500 files metadata index completed");
        const indexMs = performance.now() - indexStarted,
          searchMs = [];
        if (await fixture.regexEnabled()) await ui.click({ role: "checkbox", name: "regex" });
        for (let index = 0; index < 10; index++) {
          const query = `perf-${String(index).padStart(3, "0")}.txt`,
            started = performance.now();
          await ui.fill({ role: "textbox", name: "파일 이름 검색" }, query);
          await fixture.waitBody(query);
          searchMs.push(performance.now() - started);
        }
        fixture.performanceSearch = { fileCount: 500, indexMs, searchMs };
        await ui.click({ role: "checkbox", name: "regex" });
        await ui.fill({ role: "textbox", name: "파일 이름 검색" }, "foo|bar");
        await fixture.waitBody("foo.txt");
        await fixture.waitBody("bar.md");
        const body = await fixture.body();
        assert.ok(!body.includes("a---b.log"));
        await ui.fill({ role: "textbox", name: "파일 이름 검색" }, "a.*b");
        await fixture.waitBody("a---b.log");
        await ui.fill({ role: "textbox", name: "파일 이름 검색" }, "한글");
        await fixture.waitBody("한글.md");
        const owned = (await fixture.roots()).find((r) => path.resolve(r.path) === path.resolve(root));
        assert.equal(owned.content, false);
        assert.equal((await fixture.contentStats()).indexed_files, beforeContent);
        screenshots.push(await ui.screenshot("SEARCH-01-regex-candidates"));
        assertions.push(
          "Actual regex union, separated literals and Korean match metadata candidates without content extraction; root added through real input",
        );
      },
    ],
    [
      "SEARCH-02",
      async ({ ui, fixture, fixtureRoot, assertions, screenshots }) => {
        const root = await mkdtemp(path.join(fixtureRoot, "knowledge-search-watch-"));
        await writeFile(path.join(root, "watch-before.txt"), "fixture\n", { flag: "wx" });
        await navigateSearch({ knowledgeFixture: fixture, ui }, { role: "textbox", name: "검색 루트 경로" });
        await ui.fill({ role: "textbox", name: "검색 루트 경로" }, root);
        await ui.click({ role: "button", name: "추가" });
        await fixture.waitRoot(root);
        if (await fixture.regexEnabled()) await ui.click({ role: "checkbox", name: "regex" });
        await ui.fill({ role: "textbox", name: "파일 이름 검색" }, "watch-");
        await fixture.waitBody("watch-before.txt");
        await writeFile(path.join(root, "watch-after.txt"), "fixture\n", { flag: "wx" });
        await fixture.waitBody("watch-after.txt");
        await fixture.navigate("notes");
        const agent = fixture.agentIdentity();
        await fixture.closeAndReopen();
        fixture.assertSameAgent(agent);
        await navigateSearch({ knowledgeFixture: fixture, ui }, { role: "textbox", name: "파일 이름 검색" });
        await ui.fill({ role: "textbox", name: "파일 이름 검색" }, "watch-after");
        await fixture.waitBody("watch-after.txt");
        // Native root removal remains acknowledged before the renderer forgets it.
        await fixture.removeRootViaUi(root);
        assert.ok(!(await fixture.roots()).some((r) => path.resolve(r.path) === path.resolve(root)));
        screenshots.push(await ui.screenshot("SEARCH-02-watch-close-remove"));
        assertions.push(
          "Visible metadata polling converges after a real fixture event; installation Agent survives Knowledge close; root removal is native-acknowledged",
        );
      },
    ],
  ]);
}
