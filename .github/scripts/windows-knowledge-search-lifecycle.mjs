import assert from "node:assert/strict";
import { mkdtemp, writeFile } from "node:fs/promises";
import path from "node:path";
import { scenarios } from "./windows-knowledge-flow-shared.mjs";
export const scenarioIds = ["SEARCH-01", "SEARCH-02"];
export async function run(context) {
  return scenarios(context, [
    [
      "SEARCH-01",
      async ({ ui, fixture, fixtureRoot, assertions, screenshots }) => {
        const root = await mkdtemp(path.join(fixtureRoot, "knowledge-search-"));
        for (const name of ["foo.txt", "bar.md", "a---b.log", "한글.md"])
          await writeFile(path.join(root, name), "synthetic name-only fixture\n", { flag: "wx" });
        await fixture.navigate("search");
        const beforeContent = (await fixture.contentStats()).indexed_files;
        await ui.fill({ role: "textbox", name: "검색 루트 경로" }, root);
        await ui.click({ role: "button", name: "추가" });
        await fixture.waitRoot(root);
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
        await fixture.navigate("search");
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
        await fixture.navigate("search");
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
