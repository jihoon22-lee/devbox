// Hosted-runner-only actual WSL1 UNC exercise. The setup script owns the entire
// disposable distro. Root unavailability here is an owned directory rename,
// not a claim about WSL2 VM suspend or a stopped distribution.
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync, readFileSync, renameSync, unlinkSync, lstatSync } from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

export async function exerciseKnowledgeWsl({ item, executable, profile, command, sourceQuery, product, stop, wait, click, delay, evidence, progress }) {
  const distro = process.env.DEVBOX_KNOWLEDGE_WSL_DISTRO;
  assert.match(distro ?? "", /^DevboxKnowledgeFixture-[0-9]+-[a-f0-9]{12}$/);
  assert.equal(process.env.GITHUB_ACTIONS, "true");
  assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  const root = `\\\\wsl.localhost\\${distro}\\home\\devbox-fixture\\한글 project`;
  const alternate = `\\\\wsl$\\${distro}\\home\\devbox-fixture\\한글 project`;
  const notes = path.join(root, "Notes");
  // A distinct Search root can become unavailable without asking Windows
  // to rename the vault directory while native Notes watchers hold it open.
  const corpus = `${root}-indexed`;
  const alternateCorpus = `${alternate}-indexed`;
  const posixCorpus = "/home/devbox-fixture/한글 project-indexed";
  const posixMoved = `${posixCorpus}-temporarily-unavailable`;
  const moveCorpus = (from, to) => {
    const moved = spawnSync("wsl.exe", ["--distribution", distro, "--user", "root", "--exec", "/bin/mv", "-T", "--", from, to], { encoding: "utf8", windowsHide: true, timeout: 30_000 });
    if (moved.status !== 0) evidence.wsl.moveFailure = { status: moved.status, signal: moved.signal,
      error: moved.error?.code, stderr: (moved.stderr ?? "").slice(0, 2048), stdout: (moved.stdout ?? "").slice(0, 512) };
    assert.equal(moved.status, 0, `Owned WSL corpus move failed: ${JSON.stringify(evidence.wsl.moveFailure)}`);
  };
  mkdirSync(root); mkdirSync(notes); mkdirSync(corpus);
  writeFileSync(path.join(notes, "Case.md"), "# Upper case\nWSL preserved content\n", { flag: "wx" });
  writeFileSync(path.join(notes, "case.md"), "# Lower case\nSeparate Linux file\n", { flag: "wx" });
  for (let i = 0; i < 500; i++) writeFileSync(path.join(corpus, `wslfixture${String(i).padStart(4, "0")}.txt`), `wslnativecontent wslgroup${Math.floor(i / 100)} fixture\n`, { flag: "wx" });
  const identity = p => { const s = lstatSync(p, { bigint: true }); return `${s.dev}:${s.ino}`; };
  assert.equal(identity(root), identity(alternate));
  assert.notEqual(identity(path.join(notes, "Case.md")), identity(path.join(notes, "case.md")));
  evidence.wsl = { version: 1, boundary: "Actual WSL1 UNC; native Linux move of independent Search directory models root unavailability, not vault/VM suspension", result: "running" };
  const succeeded = response => { assert.equal(response.operation.outcome.state, "succeeded", JSON.stringify(response.operation.outcome)); return response.value; };
  const query = async (text, mode = "name", limit = 2000) => sourceQuery(item, "files", text, { sourceRootId: rootId }, limit, mode);
  const cancel = async result => command(item, "knowledge.search", "source_cancel", { generation: result.generation });
  const eventually = async (callback, label, timeout = 90_000) => {
    const end = Date.now() + timeout;
    while (Date.now() < end) { if (await callback()) return; await delay(250); }
    throw new Error(label);
  };
  progress("wsl-vault-approval-and-atomic-edit");
  succeeded(await command(item, "knowledge.migration", "schedule_vault_change", { path: alternate }));
  await stop(item); item = await product(executable, profile);
  await wait(item.cdp, '!!document.querySelector("#vault-setup-title")', "WSL vault review missing");
  await click(item.cdp, "폴더 연결 미리보기");
  await wait(item.cdp, '!!document.querySelector("#vault-preview-title")', "WSL vault preview missing");
  assert.equal(readFileSync(path.join(notes, "Case.md"), "utf8"), "# Upper case\nWSL preserved content\n");
  await click(item.cdp, "이 폴더로 변경하고 시작");
  await wait(item.cdp, '!!document.querySelector(".knowledge-feature-notes .app")', "WSL vault activation missing");
  const currentRoot = succeeded(await command(item, "knowledge.notes", "get_root"));
  assert.equal(identity(currentRoot), identity(root));
  const content = "# 한글 WSL note\r\nExplicit native save\r\n";
  succeeded(await command(item, "knowledge.notes", "write_file", { rel: "Notes/Case.md", content }));
  assert.equal(readFileSync(path.join(notes, "Case.md"), "utf8"), content);
  assert.equal(succeeded(await command(item, "knowledge.notes", "read_file", { rel: "Notes/case.md" })), "# Lower case\nSeparate Linux file\n");
  assert.equal(succeeded(await command(item, "knowledge.notes", "list_templates"))[0].content, "new product edit");
  evidence.wsl.vaultExplicitApprovalAndCaseDistinctAtomicEdit = true;

  progress("wsl-polling-and-source-query");
  const before = new Set(succeeded(await command(item, "knowledge.search", "list_roots")).map(r => r.id));
  succeeded(await command(item, "knowledge.search-settings", "add_root", { path: corpus, indexContent: true }));
  const added = succeeded(await command(item, "knowledge.search", "list_roots")).filter(r => !before.has(r.id));
  assert.equal(added.length, 1); const rootId = added[0].id;
  const statuses = succeeded(await command(item, "knowledge.search", "watcher_statuses"));
  assert.ok(statuses.some(s => s.sourceKind === "wsl" && s.watchMode === "polling"));
  await eventually(async () => {
    const result = await query("wslfixture");
    evidence.wsl.largeFilenameQuery = { state: result.state, partial: result.partial, rows: result.rows.length,
      verifiedRows: result.rows.filter(row => row.availability === "available").length };
    evidence.wsl.indexStatus = succeeded(await command(item, "knowledge.search", "index_status"));
    // The 1.5 s source deadline may leave a large remote result partly verified.
    // Cached row publication proves index coverage; exact queries below prove
    // usable native references without widening that deadline.
    const indexed = result.rows.length === 500 && !evidence.wsl.indexStatus.indexing;
    await cancel(result); return indexed;
  }, "WSL index did not publish the complete 500-file corpus");
  for (const index of [0, 99, 199, 299, 399, 499]) {
    const exact = await query(`wslfixture${String(index).padStart(4, "0")}`);
    evidence.wsl.lastExactQuery = { state: exact.state, rows: exact.rows.length, availability: exact.rows[0]?.availability };
    assert.equal(exact.rows.length, 1); assert.equal(exact.rows[0].availability, "available");
    assert.ok(exact.rows[0].reference); await cancel(exact);
  }
  const body = await query("wslnativecontent", "content"); assert.equal(body.rows.length, 200); await cancel(body);
  // Preserve the content API's existing 200-row ceiling while checking that
  // all five disjoint 100-file groups were indexed, without truncation claims.
  for (let group = 0; group < 5; group++) {
    const batch = await query(`wslgroup${group}`, "content");
    assert.equal(batch.rows.length, 100);
    await cancel(batch);
  }
  // Alias registration must keep the native root ID; spelling is not identity.
  succeeded(await command(item, "knowledge.search-settings", "add_root", { path: alternateCorpus, indexContent: true }));
  assert.equal(succeeded(await command(item, "knowledge.search", "list_roots")).filter(r => !before.has(r.id)).length, 1);
  evidence.wsl.aliasRegistrationKeptRootId = true;
  evidence.wsl.pollingAnd500FileBodySearch = true;

  const held = await query("wslfixture0000"); assert.equal(held.rows.length, 1); assert.ok(held.rows[0].reference);
  const target = path.join(corpus, "wslfixture0000.txt");
  renameSync(target, path.join(corpus, "previous-object.txt")); writeFileSync(target, "replacement fixture\n", { flag: "wx" });
  assert.equal((await command(item, "knowledge.opener", "reveal_file", { reference: held.rows[0].reference })).operation.outcome.state, "failed");
  await cancel(held);
  evidence.wsl.replacedObjectRejected = true;

  // A Linux symlink created inside this owned tree must not become a route to
  // a note outside the vault, even when the Windows UNC spelling looks local.
  const linked = spawnSync("wsl.exe", ["--distribution", distro, "--user", "root", "--exec", "/bin/ln", "-s", "/etc/hostname", "/home/devbox-fixture/한글 project/Notes/outside.md"], { encoding: "utf8", windowsHide: true, timeout: 30_000 });
  assert.equal(linked.status, 0);
  assert.equal((await command(item, "knowledge.notes", "read_file", { rel: "Notes/outside.md" })).operation.outcome.state, "failed");
  unlinkSync(path.join(notes, "outside.md"));
  evidence.wsl.outsideSymlinkRejected = true;

  progress("wsl-unavailable-last-good-and-reconnect");
  // Cancel acknowledges revocation immediately, while remote handles close on
  // the bounded retirement worker. Establish that our own file-source work has
  // actually drained before preparing an external directory move.
  await eventually(async () => {
    const state = succeeded(await command(item, "knowledge.search", "source_poll", { generation: held.generation }));
    const index = succeeded(await command(item, "knowledge.search", "index_status"));
    evidence.wsl.beforeMoveResources = { state: state.state, retainedObjects: state.bounds.retainedObjects, runningWorkers: state.bounds.runningWorkers, indexing: index.indexing };
    return state.state === "cancelled" && state.bounds.retainedObjects === 0 && state.bounds.runningWorkers === 0 && !index.indexing;
  }, "Cancelled WSL source work did not release its native objects", 30_000);
  // Move inside the owned distro: Windows UNC directory rename can fail with
  // EPERM while previously verified remote objects are being retired. A Linux
  // move models the source changing independently of the Windows client.
  moveCorpus(posixCorpus, posixMoved);
  try {
    succeeded(await command(item, "knowledge.search-settings", "index_now"));
    await eventually(async () => {
      const status = succeeded(await command(item, "knowledge.search", "index_status"));
      return !status.indexing && !!status.last_error;
    }, "Unavailable WSL root was not diagnosed");
    const offline = await query("wslfixture");
    evidence.wsl.offlineQuery = { state: offline.state, partial: offline.partial, rows: offline.rows.length,
      bounds: offline.bounds, indexStatus: succeeded(await command(item, "knowledge.search", "index_status")),
      watcherStatuses: succeeded(await command(item, "knowledge.search", "watcher_statuses")) };
    assert.equal(offline.rows.length, 500); assert.ok(offline.rows.every(r => r.availability !== "available" && !r.reference));
    await cancel(offline);
    // The unrelated active Notes vault and local indexed source remain usable.
    // Notes unavailable/save refusal is separately executed by the native WSL2
    // vault fixture, where the owned vault root can actually be moved.
    succeeded(await command(item, "knowledge.notes", "write_file", { rel: "Notes/Case.md", content }));
    assert.equal(readFileSync(path.join(notes, "Case.md"), "utf8"), content);
    // An unrelated local root continues serving results during WSL failure.
    const local = await sourceQuery(item, "files", "fixturesearch0001");
    assert.ok(local.rows.some(r => r.availability === "available")); await cancel(local);
    evidence.wsl.unavailableRootKeptLastGoodAndLocalSource = true;
  } finally { moveCorpus(posixMoved, posixCorpus); }
  unlinkSync(path.join(corpus, "wslfixture0001.txt"));
  writeFileSync(path.join(corpus, "wslfixture0500.txt"), "reconnected fixture\n", { flag: "wx" });
  // No explicit index_now: the existing WSL polling owner must reconcile the
  // restored complete snapshot and remove only the now-confirmed deletion.
  await eventually(async () => {
    const gone = await query("wslfixture0001"), fresh = await query("wslfixture0500");
    const ready = gone.rows.length === 0 && fresh.rows.length === 1 && fresh.rows[0].availability === "available";
    await cancel(gone); await cancel(fresh); return ready;
  }, "WSL polling did not converge after reconnect", 120_000);
  assert.equal(succeeded(await command(item, "knowledge.notes", "read_file", { rel: "Notes/Case.md" })), content);
  evidence.wsl.reconnectPollingConverged = true;
  evidence.wsl.result = "pass";
  return item;
}
