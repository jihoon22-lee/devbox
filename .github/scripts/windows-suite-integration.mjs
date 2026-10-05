import { navigateWorkspaceFiles } from "./windows-workspace-ui-observations.mjs";
// L4 preparations are identified separately; domain transfers/reviews use real UI input.
import assert from "node:assert/strict";
import path from "node:path";
import { readFile, readdir, writeFile } from "node:fs/promises";
import {
  changeKnowledgeExecuteAccess,
  receiverAccessPending,
  withUnavailableReceiver,
} from "./windows-suite-receiver-access.mjs";
import { setTimeout as delay } from "node:timers/promises";
import { pathToFileURL } from "node:url";
import { createApiUserFlowContext, markApiCleanupFailure } from "./windows-api-user-flow-adapter.mjs";
import { createInstalledProductContext } from "./windows-suite-ui-context.mjs";
import { createDirectProductContext } from "./windows-suite-direct-layout.mjs";
import { createUiDriver } from "./suite-user-flow-driver.mjs";
import { workspaceRequestExpression } from "./windows-workspace-registration.mjs";
import { typedComponentBridge } from "./typed-component-fixture.mjs";
import { captureWindowOwner, nativeWindowAction } from "./windows-user-flow-window.mjs";
import { freePort, connect, waitForRenderer } from "./workspace-cdp-fixture.mjs";
import {
  allWindowsProcesses,
  inspectElevatedCdpPolicy,
  installElevatedCdpPolicy,
  releaseCdpSession,
  stopOwnedProcess,
} from "./windows-packaged-smoke.mjs";
import {
  requireApiContext,
  button,
  textbox,
  select,
  scenario,
  until,
  expectText,
} from "./windows-api-user-flow-actions.mjs";
import { writeUserFlowResults } from "./suite-user-flow-results.mjs";
export const SCENARIO_IDS = Object.freeze(["HANDOFF-01", "HANDOFF-02"]);
const pipeline = { role: "region", name: "타입 지정 파이프라인" };
const incoming = { role: "region", name: "다른 제품의 열기 요청" };
const draftDialog = { role: "dialog", name: "API Studio 결과 초안 미리보기" };
async function domain(context, product, component, method, args = {}, route = "notes") {
  const response = await context.cdp.evaluate(
    `(async()=>{const invoke=window.__TAURI_INTERNALS__.invoke;${typedComponentBridge} const d=await invoke('plugin:product-shell|describe');return invokeComponent(${JSON.stringify(product)},{request:{header:{protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+15000,route:${JSON.stringify(route)},context:d.context},component:${JSON.stringify(component)},method:${JSON.stringify(method)},args:${JSON.stringify(args)}}});})()`,
  );
  assert.equal(response.operation.outcome.state, "succeeded");
  return response.value;
}
async function pending(context) {
  const response = await context.cdp.evaluate(
    "(async()=>{const invoke=window.__TAURI_INTERNALS__.invoke;const d=await invoke('plugin:product-shell|describe');return invoke('plugin:suite|connection',{request:{header:{protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+5000,route:d.features[0].route,context:d.context},method:{kind:'pending'}}});})()",
  );
  assert.equal(response.operation.outcome.state, "succeeded");
  return response.value;
}
export async function prepareExpiryPipeline(api) {
  // The fresh expiry offer mounts asynchronously and changes the controls' scrollport.
  await api.ui.waitForTarget(incoming);
  await api.ui.click(button("새 파이프라인"));
  await select(api, "파이프라인 입력 형식", 0);
}
export async function reviewHandoffClose(workspace, ownedFile) {
  const dirtyPaths =
    await workspace.cdp.evaluate(`Array.from(document.querySelectorAll('.document-tab-select[role="tab"]'))
    .filter(tab => tab.querySelector('.document-tab-name')?.textContent.trimStart().startsWith('● '))
    .map(tab => tab.title)`);
  assert.ok(
    dirtyPaths.every((file) => file === ownedFile),
    "Unowned dirty document prevents handoff discard",
  );
  await workspace.ui.click(
    button(dirtyPaths.length ? "파일 변경 폐기 후 종료" : "종료", { role: "dialog", name: "Workspace 종료 검토" }),
  );
}
export async function review(context) {
  await context.ui.waitForTarget(incoming);
  await context.ui.click(button("화면 열기", incoming));
}
export async function applySourceSelection(api, expected) {
  assert.ok(typeof expected === "string" && expected.length > 0 && Buffer.byteLength(expected) <= 1_000_000);
  await api.ui.click(button("적용", { role: "dialog", name: "Toolbox 텍스트 미리보기" }));
  try {
    await until(
      // Read the controlled textarea itself, including when it is below the
      // viewport. An AX name fallback is not evidence of its accepted value.
      () =>
        api.cdp.evaluate(`(()=>{
      const inputs = document.querySelectorAll('textarea[aria-label="스마트 워크플로 입력"]');
      return inputs.length === 1 && inputs[0].value === ${JSON.stringify(expected)};
    })()`),
      "Accepted source selection did not reach Smart input",
    );
  } catch (error) {
    try {
      const observation = await api.cdp.evaluate(`(()=>{
        const inputs = document.querySelectorAll('textarea[aria-label="스마트 워크플로 입력"]');
        const value = inputs.length === 1 ? inputs[0].value : null;
        return { inputCount: Math.min(inputs.length, 2), empty: value === '',
          exact: value === ${JSON.stringify(expected)},
          containsExpected: typeof value === 'string' && value.includes(${JSON.stringify(expected)}),
          previewPresent: !!document.querySelector('.toolbox-handoff-dialog'),
          acceptedActionsPresent: !!document.querySelector('.incoming-diff-actions') };
      })()`);
      await writeFile("product-foundation-evidence/handoff-input-first-failure.json", JSON.stringify(observation), {
        flag: "wx",
      });
    } catch {}
    throw error;
  }
}
export async function awaitKnowledgePreviewClosed(knowledge) {
  await until(
    () => knowledge.cdp.evaluate("document.querySelector('.handoff-dialog') === null"),
    "Knowledge preview decision did not complete",
  );
}
export async function cancelKnowledgePreview(knowledge) {
  await knowledge.ui.waitForTarget(button("취소", draftDialog));
  await knowledge.ui.click(button("취소", draftDialog));
  await awaitKnowledgePreviewClosed(knowledge);
}
export async function saveKnowledgePreview(knowledge) {
  await knowledge.ui.waitForTarget(button("초안 저장", draftDialog));
  await knowledge.ui.click(button("초안 저장", draftDialog));
}
export async function disconnectSuiteConnection(api) {
  await api.ui.click(button("제품 연결"));
  await api.ui.waitForTarget(button("자동 연결 끄기"));
  await api.ui.click(button("자동 연결 끄기"));
}
export async function restoreSuiteConnection(api) {
  await api.ui.click(button("이 설치 확인"));
  await api.ui.waitForTarget(button("연결 켜기"));
  await api.ui.click(button("연결 켜기"));
}
export async function selectSource(workspace) {
  await workspace.ui.click({ role: "textbox", name: "" });
  await workspace.ui.press("Control+a");
  const key = { key: "F10", code: "F10", windowsVirtualKeyCode: 121, modifiers: 8 };
  await workspace.cdp.command("Input.dispatchKeyEvent", { type: "keyDown", ...key });
  await workspace.cdp.command("Input.dispatchKeyEvent", { type: "keyUp", ...key });
  await workspace.ui.waitForTarget({ role: "menuitem", name: "선택 내용을 API Studio에서 변환" });
  await workspace.ui.click({ role: "menuitem", name: "선택 내용을 API Studio에서 변환" });
}
export async function selectBase64Stage(api) {
  const index = await api.cdp.evaluate(
    "Array.from(document.querySelector('[aria-label=\"변환 단계 추가\"]').options).filter(option=>!option.disabled).findIndex(option=>option.value==='base64-encode')",
  );
  assert.ok(index >= 0);
  await select(api, "변환 단계 추가", index);
  await until(
    () => api.cdp.evaluate("document.querySelector('[aria-label=\"변환 단계 추가\"]').value==='base64-encode'"),
    "Base64 stage selection not observed",
  );
}
async function executePipeline(api) {
  const input = await api.ui.text(textbox("스마트 워크플로 입력"));
  const expected = Buffer.from(input, "utf8").toString("base64");
  assert.ok(expected);
  await api.ui.click(button("파이프라인 실행"));
  await until(
    async () =>
      await api.cdp.evaluate(
        `document.querySelector('[aria-label="파이프라인 결과"]')?.textContent.trim()===${JSON.stringify(expected)}`,
      ),
    "Current source Base64 output not observed",
  );
  return expected;
}
async function storeOutput(api) {
  await api.ui.click(button("Knowledge 초안 보관", pipeline));
  await api.ui.click(button("마스킹 사본 보관", { role: "dialog", name: "Knowledge 초안 보관 확인" }));
  await expectText(api, "API Studio에 마스킹한 초안을 보관했습니다.");
}
async function sendStored(api) {
  await api.ui.click(button("Knowledge에서 초안 검토", pipeline));
}
async function snapshotFiles(root) {
  const files = {};
  async function walk(dir) {
    for (const entry of await readdir(dir, { withFileTypes: true })) {
      assert.ok(!entry.isSymbolicLink());
      const file = path.join(dir, entry.name);
      if (entry.isDirectory()) await walk(file);
      else if (entry.name.endsWith(".md")) files[path.relative(root, file)] = (await readFile(file)).toString("base64");
      assert.ok(Object.keys(files).length < 1000);
    }
  }
  await walk(root);
  return files;
}
async function coldReceiver(previous, activate) {
  const executable = previous.executable,
    root = previous.root;
  await previous.close();
  assert.equal(
    allWindowsProcesses().some((p) => p.Path.toLowerCase() === executable.toLowerCase()),
    false,
  );
  const port = await freePort(),
    policy = inspectElevatedCdpPolicy(path.basename(executable), port);
  installElevatedCdpPolicy(policy);
  let identity, cdp;
  try {
    await activate();
    await until(
      () => {
        identity = allWindowsProcesses().find((p) => p.Path.toLowerCase() === executable.toLowerCase());
        return Boolean(identity);
      },
      "Normal cold activation did not launch the exact Knowledge member",
      30000,
    );
    const child = {
      pid: identity.Pid,
      get exitCode() {
        return allWindowsProcesses().some(
          (p) => p.Pid === identity.Pid && p.Created === identity.Created && p.Path === identity.Path,
        )
          ? null
          : 0;
      },
    };
    const owner = captureWindowOwner(identity, path.dirname(root));
    cdp = await connect(port, child);
    await waitForRenderer(
      cdp,
      "Boolean(document.querySelector('.product-shell > main'))",
      "actual cold Knowledge shell",
    );
    let closed = false;
    const close = async () => {
      if (closed) return;
      closed = true;
      try {
        if (child.exitCode === null) {
          nativeWindowAction(owner, "Close");
          await until(() => child.exitCode !== null, "Cold Knowledge did not close normally");
        }
      } finally {
        releaseCdpSession({ cdp, policy });
      }
    };
    const ui = createUiDriver({
      cdp,
      evidenceRoot: "product-foundation-evidence/user-flows/screenshots/knowledge-cold",
      closeOwnedWindow: close,
    });
    return { ...previous, cdp, ui, child, processIdentity: identity, close };
  } catch (error) {
    if (identity) {
      const owner = captureWindowOwner(identity, path.dirname(root));
      nativeWindowAction(owner, "Close");
      await until(
        () => !allWindowsProcesses().some((p) => p.Pid === identity.Pid && p.Created === identity.Created),
        "Failed cold receiver cleanup",
      );
    }
    releaseCdpSession({ cdp, policy });
    throw error;
  }
}
export async function run(api) {
  requireApiContext(api);
  const results = [];
  let workspace, knowledge, foreign, ownedSourceFile;
  let restoreAccess = null;
  try {
    const receipt = JSON.parse(
      (await readFile(path.join(path.dirname(api.root), "workspace-handoff-fixture.json"), "utf8")).replace(
        /^\uFEFF/,
        "",
      ),
    );
    assert.equal(receipt.sourceSha, api.sourceSha);
    assert.equal(receipt.installationKey, api.installationKey);
    assert.ok(path.resolve(receipt.file).startsWith(path.resolve(receipt.root) + path.sep));
    const original = await readFile(receipt.file);
    ownedSourceFile = receipt.file;
    workspace = await createInstalledProductContext("workspace");
    knowledge = await createInstalledProductContext("knowledge");
    const root = await domain(knowledge, "knowledge", "knowledge.notes", "get_root");
    assert.ok(
      root.includes(`.i${api.installationKey}${path.sep}`) || root.startsWith(path.dirname(api.root) + path.sep),
    );
    const readRegistry = async () => {
      const response = await workspace.cdp.evaluate(workspaceRequestExpression("workspace.registry", "snapshot"));
      assert.equal(response.operation.outcome.state, "succeeded");
      return response.value;
    };
    await workspace.ui.click(button("개요"));
    const registry = await readRegistry(),
      project = registry.projects.find((item) =>
        registry.worktrees.some(
          (tree) =>
            tree.projectId === item.id &&
            path.resolve(tree.binding.root).toLowerCase() === path.resolve(receipt.root).toLowerCase(),
        ),
      );
    assert.ok(project, "Retained Windows project not registered");
    await workspace.ui.click(button("프로젝트 선택", { role: "region", name: project.name }));
    await navigateWorkspaceFiles(workspace.ui);
    await workspace.ui.fill(textbox("열 파일 경로"), receipt.file);
    await workspace.ui.click(button("파일 열기"));
    await until(
      async () => await workspace.cdp.evaluate("Boolean(document.querySelector('.cm-content'))"),
      "Source editor missing",
    );
    let expiredAt = 0;
    results.push(
      await scenario(api, "HANDOFF-01", async (record) => {
        record(
          "L4 fixture reuses the retained Workspace Windows project and native validated Knowledge vault in this installed namespace",
        );
        await selectSource(workspace);
        await until(async () => (await pending(api)).length === 1, "Selection review missing");
        await review(api);
        await expectText(api, "Toolbox 텍스트 미리보기");
        await expectText(api, "원본");
        await api.ui.click(button("취소", { role: "dialog", name: "Toolbox 텍스트 미리보기" }));
        assert.deepEqual(await readFile(receipt.file), original);
        record(
          "Actual Unicode editor selection reaches hot API preview; recipient cancel preserves exact original file bytes",
        );
        await selectSource(workspace);
        await workspace.ui.click({ role: "textbox", name: "" });
        await workspace.ui.press("End");
        await workspace.ui.typeText(" source-revision-changed");
        await until(async () => {
          const response = await workspace.cdp.evaluate(workspaceRequestExpression("workspace.files", "load_recovery"));
          assert.equal(response.operation.outcome.state, "succeeded");
          return response.value.entries.some(
            (entry) => entry.path === receipt.file && entry.content.includes("source-revision-changed"),
          );
        }, "Changed source buffer journal not observed");
        await review(api);
        await expectText(api, "원본이 변경·닫힘·만료되었거나");
        assert.equal(await api.cdp.evaluate("Boolean(document.querySelector('.toolbox-handoff-dialog'))"), false);
        await workspace.ui.press("Control+z");
        record(
          "Actual source buffer change before receiver review rejects the captured revision without applying stale selection",
        );
        await selectSource(workspace);
        await review(api);
        await expectText(api, "Toolbox 텍스트 미리보기");
        await applySourceSelection(
          api,
          original
            .toString("utf8")
            .replace(/^\uFEFF/u, "")
            .replace(/\r\n/g, "\n"),
        );
        await selectSource(workspace);
        expiredAt = Date.now() + 122000;
        record(
          "Real native two-minute source selection expiry starts on a fresh UI-issued offer while independent transform/Knowledge journeys execute",
        );
        await prepareExpiryPipeline(api);
        await selectBase64Stage(api);
        await api.ui.click(button("단계 추가"));
        const output = await executePipeline(api);
        assert.ok(output.trim());
        const before = await snapshotFiles(root);
        await storeOutput(api);
        await sendStored(api);
        await review(knowledge);
        await expectText(knowledge, output.trim());
        await cancelKnowledgePreview(knowledge);
        assert.deepEqual(await snapshotFiles(root), before);
        await sendStored(api);
        await review(knowledge);
        await saveKnowledgePreview(knowledge);
        await until(
          async () => Object.keys(await snapshotFiles(root)).length === Object.keys(before).length + 1,
          "Explicit draft save did not create exactly one note",
        );
        const saved = await snapshotFiles(root);
        await awaitKnowledgePreviewClosed(knowledge);
        await sendStored(api);
        await review(knowledge);
        await expectText(knowledge, "이미 저장한 결과 초안입니다.");
        assert.equal(await knowledge.cdp.evaluate("Boolean(document.querySelector('.handoff-dialog'))"), false);
        assert.deepEqual(await snapshotFiles(root), saved);
        record(
          "Actual Text→Base64 output requires producer store and Knowledge review; cancel creates no note, save creates exactly one, repeated same stored artifact creates no duplicate",
        );
        await api.ui.fill(textbox("스마트 워크플로 입력"), "cold owned output");
        await executePipeline(api);
        await storeOutput(api);
        knowledge = await coldReceiver(knowledge, () => sendStored(api));
        await review(knowledge);
        await expectText(knowledge, Buffer.from("cold owned output").toString("base64"));
        await cancelKnowledgePreview(knowledge);
        assert.deepEqual(await snapshotFiles(root), saved);
        record(
          "Normal product activation launches the exact candidate Knowledge member from a closed state; owned registry CDP instrumentation observes actual preview/cancel without relaunch",
        );
        while (Date.now() < expiredAt) await delay(Math.min(60000, expiredAt - Date.now()));
        await review(api);
        await until(
          async () => /만료|이미 처리/.test(await api.cdp.evaluate("document.body.innerText")),
          "Expired selection lacks actual UI rejection",
        );
        assert.equal(await api.cdp.evaluate("Boolean(document.querySelector('.toolbox-handoff-dialog'))"), false);
        assert.deepEqual(await readFile(receipt.file), original);
        record(
          "Actual native two-minute elapsed source selection is rejected by its visible review action; original Workspace bytes survive and stale text is never applied",
        );
      }),
    );
    if (results.at(-1).status !== "PASS") return results;
    results.push(
      await scenario(api, "HANDOFF-02", async (record) => {
        const before = await snapshotFiles(root),
          sourceBytes = await readFile(receipt.file);
        await selectSource(workspace);
        await until(async () => (await pending(api)).length === 1, "Fresh foreign-boundary offer absent");
        foreign = await createDirectProductContext("api-studio");
        const ownDescription = await api.cdp.evaluate(
            "window.__TAURI_INTERNALS__.invoke('plugin:product-shell|describe')",
          ),
          foreignDescription = await foreign.cdp.evaluate(
            "window.__TAURI_INTERNALS__.invoke('plugin:product-shell|describe')",
          );
        assert.notEqual(foreignDescription.handshake.installationId, ownDescription.handshake.installationId);
        const offered = (await pending(api))[0];
        assert.ok(offered);
        await assert.rejects(() =>
          domain(
            foreign,
            "api-studio",
            "api-studio.transforms",
            "open_workspace_selection",
            { id: offered.target.id, operationId: offered.operationId, revision: offered.commandRevision },
            "transforms",
          ),
        );
        assert.equal(await foreign.cdp.evaluate("Boolean(document.querySelector('.toolbox-handoff-dialog'))"), false);
        assert.deepEqual(await readFile(receipt.file), sourceBytes);
        await foreign.close();
        foreign = null;
        record(
          "L4 exact-candidate direct copy has a genuine different native installation; its authenticated foreign-artifact rejection probe cannot open the original offer, and actual foreign UI/source bytes remain unchanged",
        );
        await disconnectSuiteConnection(api);
        await expectText(api, "자동 연결이 꺼져 있습니다.");
        await sendStored(api);
        await expectText(api, "전달 결과를 확인하지 못했습니다.");
        assert.deepEqual(await snapshotFiles(root), before);
        await restoreSuiteConnection(api);
        await expectText(api, "이 설치의 제품이 연결되어 있습니다.");
        await api.ui.click(button("제품 연결"));
        record(
          "Actual connection-off action blocks outgoing transfer and displays current failure guidance; explicit installation review restores the same connection without consuming another installation's offer",
        );
        await knowledge.close();
        const receiver = knowledge;
        knowledge = null;
        await withUnavailableReceiver(
          (action) => {
            if (action === "Deny") restoreAccess = receiver;
            changeKnowledgeExecuteAccess(receiver, action);
            restoreAccess = action === "Deny" ? receiver : null;
          },
          async () => {
            await sendStored(api);
            await expectText(api, "전달 결과를 확인하지 못했습니다.");
            await api.ui.waitForTarget(button("Knowledge에서 초안 검토", pipeline));
            assert.deepEqual(await snapshotFiles(root), before);
            assert.deepEqual(await readFile(receipt.file), sourceBytes);
          },
        );
        record(
          "L4 temporarily denies execution only on the closed owned receiver image while Suite integrity pins remain active; actual UI transfer reports unavailable and preserves source/vault, then restores the original ACL and verifies identical bytes",
        );
        await api.ui.fill(textbox("스마트 워크플로 입력"), "recovered owned output");
        await executePipeline(api);
        await storeOutput(api);
        knowledge = await createInstalledProductContext("knowledge");
        await sendStored(api);
        await review(knowledge);
        await cancelKnowledgePreview(knowledge);
        assert.deepEqual(await snapshotFiles(root), before);
        record(
          "Actual recovered receiver review succeeds after exact member restoration; cancellation still creates no note and source bytes remain intact",
        );
      }),
    );
    if (results.at(-1).status !== "PASS") return results;
    return results;
  } catch (error) {
    const id =
      results.find((row) => row.status !== "PASS")?.id ??
      (results.length === 2 ? "HANDOFF-01" : SCENARIO_IDS[results.length]);
    const failure = await scenario(api, id, async () => {
      throw error;
    });
    const index = results.findIndex((row) => row.id === id);
    if (index >= 0) results[index] = { ...failure, assertions: results[index].assertions };
    else results.push(failure);
    return results;
  } finally {
    let cleanupFailed = false;
    if (restoreAccess && receiverAccessPending(restoreAccess))
      try {
        changeKnowledgeExecuteAccess(restoreAccess, "Restore");
      } catch {
        cleanupFailed = true;
      }
    for (const context of [foreign, knowledge, workspace]) {
      if (!context) continue;
      try {
        await context.close(
          context === workspace
            ? { reviewWorkspaceClose: () => reviewHandoffClose(workspace, ownedSourceFile) }
            : undefined,
        );
      } catch {
        cleanupFailed = true;
        const identity = context.processIdentity;
        if (identity) {
          const child = context.child ?? {
            pid: identity.Pid,
            get exitCode() {
              return allWindowsProcesses().some(
                (p) => p.Pid === identity.Pid && p.Created === identity.Created && p.Path === identity.Path,
              )
                ? null
                : 0;
            },
          };
          await stopOwnedProcess(identity, context.executable ?? identity.Path, child).catch(() => {});
        }
      }
    }
    if (cleanupFailed) {
      if (!results.length)
        results.push(
          await scenario(api, "HANDOFF-01", async () => {
            throw new Error("Owned cleanup failed");
          }),
        );
      markApiCleanupFailure(results.at(-1), "handoff-owned-cleanup-failed");
    }
  }
}
export async function runIntegration() {
  const api = await createApiUserFlowContext({ measureStartup: false });
  let results = [];
  try {
    results = await run(api);
  } finally {
    let cleanupFailed = false;
    try {
      await api.ui.closeOwnedWindow();
    } catch {
      cleanupFailed = true;
    }
    try {
      await api.close();
    } catch {
      cleanupFailed = true;
    }
    if (cleanupFailed) {
      if (!results.length)
        results.push(
          await scenario(api, "HANDOFF-01", async () => {
            throw new Error("Owned cleanup failed");
          }),
        );
      markApiCleanupFailure(results.at(-1), "handoff-api-cleanup-failed");
    }
    if (results.length) await writeUserFlowResults("handoff", results);
  }
  assert.equal(results.length, 2);
  assert.ok(
    results.every((row) => row.status === "PASS"),
    "Actual Suite handoff journey incomplete",
  );
  return results;
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href)
  console.log(JSON.stringify(await runIntegration()));
