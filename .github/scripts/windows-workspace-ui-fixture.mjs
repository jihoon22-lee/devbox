import { waitForFixtureChildExit } from "./fixture-child-exit.mjs";
import { readWorkspaceAgentOperations } from "./windows-workspace-agent-observation.mjs";
import { boundedFailure } from "./user-flow-failure-evidence.mjs";
import { dismissWorkspaceUndo } from "./windows-workspace-agent-registry-ui.mjs";
import {
  selectRegisteredWorkspaceRoot,
  observeWorkspaceSelection,
  waitForSelectedWorkspaceRoot,
} from "./windows-workspace-registry-observations.mjs";
import { observeWorkspaceInput } from "./windows-workspace-input-ui.mjs";
// Owned packaged Workspace fixture. Renderer mutations always use UiDriver input.
import assert from "node:assert/strict";
import { readFile, readdir, rename, mkdir, rm, writeFile, lstat } from "node:fs/promises";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { setTimeout as delay } from "node:timers/promises";
import { downloadArchive } from "./windows-workspace-lsp.mjs";
import { workspaceRequestExpression } from "./windows-workspace-registration.mjs";
export async function withOwnedWslRunning(owner, runId, action, launch = spawn) {
  assert.equal(owner.runId, runId);
  assert.match(owner.name, /^DevboxKnowledgeFixture-[0-9]+-[a-f0-9]{12}$/);
  const child = launch(
    "wsl.exe",
    ["--distribution", owner.name, "--exec", "/bin/bash", "-c", "printf 'ready\\n'; read -r -t 900 || true"],
    { stdio: ["pipe", "pipe", "ignore"] },
  );
  let original, pipeError;
  child.stdin.on("error", (error) => {
    pipeError ??= error;
  });
  try {
    await new Promise((resolve, reject) => {
      let received = "";
      const timer = setTimeout(() => finish(new Error("Owned WSL keepalive readiness timed out")), 20_000);
      const finish = (error) => {
        clearTimeout(timer);
        child.stdout.removeListener("data", data);
        child.removeListener("exit", exited);
        error ? reject(error) : resolve();
      };
      const data = (chunk) => {
        received += chunk.toString();
        if (received.length > 64) finish(new Error("Invalid owned WSL readiness"));
        else if (received === "ready\n") finish();
      };
      const exited = () => finish(new Error("Owned WSL keepalive exited before readiness"));
      child.stdout.on("data", data);
      child.once("exit", exited);
      child.on("error", finish);
    });
    return await action();
  } catch (error) {
    original = error;
    throw error;
  } finally {
    try {
      child.stdin.end();
    } catch (error) {
      pipeError ??= error;
    }
    try {
      await waitForFixtureChildExit(child, 5000);
    } catch (error) {
      pipeError ??= error;
    }
    if (!original && pipeError) throw pipeError;
  }
}
export async function observeManagedLspTransition(ui, scope, nextAction, observe, timeoutMs = 30_000) {
  // Native status verifies archives under the same exclusive installer lock as
  // the renderer refresh. Never introduce a competing observer before it settles.
  await ui.waitForTarget({ role: "button", name: nextAction, scope }, { timeoutMs });
  return observe();
}
export async function deleteTerminalProfile(surface, mainUi, wait) {
  const remove = { role: "button", name: "owned terminal profile 프로필 삭제" };
  await surface.ui.waitForTarget(remove);
  await surface.ui.click(remove);
  const confirm = { role: "button", name: "삭제" };
  await surface.ui.waitForTarget(confirm);
  await surface.ui.click(confirm);
  await wait(
    () => surface.cdp.evaluate(`!document.querySelector('button[aria-label="owned terminal profile 프로필 삭제"]')`),
    "companion acknowledged profile deletion",
  );
  // Preserve the main window's stale selected profile to exercise native rejection.
  await mainUi.click({ role: "button", name: "프로필로 터미널 열기" });
}
export async function prepareTerminalStart(ui, root, command) {
  await ui.waitForTarget({ role: "textbox", name: "시작 경로" });
  await ui.fill({ role: "textbox", name: "시작 경로" }, root);
  await ui.fill({ role: "textbox", name: "시작 명령" }, command);
  await ui.click({ role: "button", name: "+ 터미널" });
  await ui.waitForTarget({ role: "button", name: "실행" });
  await ui.click({ role: "button", name: "실행" });
}
export async function dismissFailedManagedInstall({ ui, wait, isOpen }) {
  await wait(
    async () => (await ui.text({ role: "alert", name: "" })).includes("관리형 서버를 설치하지 못했습니다."),
    "managed install failed",
  );
  const cancel = { role: "button", name: "취소", scope: { role: "dialog", name: "관리형 서버 작업 확인" } };
  await ui.waitForTarget(cancel);
  await ui.click(cancel);
  await wait(async () => !(await isOpen()), "failed managed install review closed");
}
export function assertLostRuntimeReceipt(receipts, operationId, targetId, method) {
  const matching = receipts.filter((receipt) => receipt.operationId === operationId);
  assert.equal(matching.length, 1, "Lost native reply must retain its exact unacknowledged receipt after process exit");
  assert.equal(matching[0].targetId, targetId);
  assert.equal(matching[0].method, method);
  assert.equal(matching[0].state, "completed");
  assert.equal(matching[0].reviewed, false);
}
export async function loseRuntimeReply({
  cdp,
  ui,
  wait,
  jobId,
  scope,
  action,
  pendingExpression,
  beforeRestart,
  restart,
}) {
  let enabled = false,
    restarted = false,
    frameId = null,
    unsubscribe,
    click,
    firstError;
  try {
    await cdp.command("Debugger.enable");
    enabled = true;
    const functionValue = await cdp.command("Runtime.evaluate", {
      expression: "window.__TAURI_INTERNALS__.invoke",
      returnByValue: false,
    });
    assert.ok(functionValue.result.objectId);
    unsubscribe = cdp.onEvent("Debugger.paused", (event) => {
      frameId = event?.callFrames?.[0]?.callFrameId ?? null;
    });
    const method = action === "중지" ? "stop_active_run" : "run_job_now";
    const entry = await cdp.command("Debugger.setBreakpointOnFunctionCall", {
      objectId: functionValue.result.objectId,
      condition: `cmd==="plugin:workspace|runtime"&&payload?.request?.method==="runtime_control"&&payload.request.args?.method===${JSON.stringify(method)}&&payload.request.args.args?.id===${JSON.stringify(jobId)}`,
    });
    const target = { role: "button", name: action, scope };
    click = action === "중지" ? ui.clickWithConfirmation(target, true) : ui.click(target);
    click.catch(() => {});
    await wait(
      async () => typeof frameId === "string" && frameId.length > 0,
      "intended native control request observed before dispatch",
    );
    const submitted = await cdp.command("Debugger.evaluateOnCallFrame", {
      callFrameId: frameId,
      expression: "({requestId:payload.request.header.requestId,operationId:payload.request.args.operationId})",
      returnByValue: true,
      silent: true,
    });
    assert.ok(!submitted.exceptionDetails, "Native control request observation failed");
    const request = submitted.result?.value;
    assert.equal(typeof request?.requestId, "string");
    assert.equal(typeof request?.operationId, "string");
    const callback = await cdp.command("Debugger.evaluateOnCallFrame", {
      callFrameId: frameId,
      expression: "window.__TAURI_INTERNALS__.runCallback",
      returnByValue: false,
    });
    assert.ok(callback.result.objectId);
    await cdp.command("Debugger.removeBreakpoint", { breakpointId: entry.breakpointId });
    await cdp.command("Debugger.setBreakpointOnFunctionCall", {
      objectId: callback.result.objectId,
      condition: `data?.operation?.provenance?.requestId===${JSON.stringify(request.requestId)}&&data?.value?.jobId===${JSON.stringify(jobId)}`,
    });
    frameId = null;
    await cdp.command("Debugger.resume");
    await wait(
      async () => typeof frameId === "string" && frameId.length > 0,
      "exact native control result paused before renderer receipt",
    );
    // Runtime.evaluate awaits execution on the paused renderer. This read stays
    // in its current call frame and never resumes/consumes the native callback.
    const response = await cdp.command("Debugger.evaluateOnCallFrame", {
      callFrameId: frameId,
      expression: pendingExpression,
      returnByValue: true,
      silent: true,
    });
    assert.ok(!response.exceptionDetails, "Paused pending-request observation failed");
    const requests = response.result?.value;
    assert.ok(Array.isArray(requests), "Paused pending-request observation must be an array");
    assert.equal(requests.length, 1, "Lost reply must retain exactly one original request");
    assert.equal(requests[0].operationId, request.operationId, "Paused reply must match the original control request");
    await beforeRestart();
    await restart(true);
    restarted = true;
    return requests[0].operationId;
  } catch (error) {
    firstError = error;
    throw error;
  } finally {
    unsubscribe?.();
    try {
      // A successful crash already destroyed the paused session. On failure,
      // release the debugger so later read-only journeys cannot inherit a pause.
      if (enabled && !restarted) await cdp.command("Debugger.disable");
    } catch (error) {
      if (!firstError) throw error;
    } finally {
      await click?.catch(() => {});
    }
  }
}

export async function openCurrentProjectTerminal(ui) {
  await ui.click({ role: "button", name: "터미널" });
  const target = { role: "button", name: "현재 프로젝트의 터미널 열기" };
  await ui.waitForTarget(target);
  await ui.click(target);
}

export async function waitForRuntimeControlIdle(pending, receipts, wait, jobId) {
  await wait(
    async () =>
      (await pending()).length === 0 &&
      !(await receipts()).some((receipt) => receipt.targetId === jobId && !receipt.reviewed),
    "previous explicit control acknowledged before the next lost reply",
  );
}

export async function stopReconciledRuntimeRun(ui, wait, activeRun, scope) {
  const target = { role: "button", name: "중지", scope };
  await ui.waitForTarget(target);
  await ui.clickWithConfirmation(target, true);
  await wait(async () => (await activeRun()) === null, "original owned run stopped before the next explicit run");
  // The native stop can settle before the renderer finishes its busy refresh.
  await ui.waitForTarget({ role: "button", name: "지금 실행", scope });
}

export async function resumeRuntimeUi(fixture, ui) {
  await fixture.selectWindows();
  await ui.click({ role: "button", name: "작업 및 서비스" });
  await ui.waitForTarget({ role: "button", name: "+ 새 작업" });
}

export function createWorkspaceUiFixture({
  ui,
  cdp,
  dataRoot,
  fixtureRoot,
  windowOwner,
  network,
  chooseArchive,
  terminalUi,
  restart,
  waitForExit,
  cleanup,
}) {
  let fault = null,
    agentRoot = null,
    agentDistro = null;
  const read = async (component, method, args = {}) => {
    const result = await cdp.evaluate(workspaceRequestExpression(component, method, args));
    assert.equal(result?.operation?.outcome?.state, "succeeded", `Owned observation failed: ${component}.${method}`);
    return result.value;
  };
  const context = async () =>
    (await cdp.evaluate('window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe")')).context;
  async function wait(predicate, label, timeout = 20_000) {
    const deadline = performance.now() + timeout;
    while (performance.now() < deadline) {
      try {
        if (await predicate()) return;
      } catch {
        /* bounded observation during renderer transitions */
      }
      await delay(100);
    }
    throw new Error(`Owned Workspace observation timed out: ${label}`);
  }
  async function selectOption(target, index, driver = ui) {
    await driver.waitForTarget(target);
    await driver.click(target);
    await driver.press("Home");
    // Native select keyboard/typeahead, no renderer setter or synthetic DOM event.
    assert.ok(Number.isInteger(index) && index >= 0);
    for (let option = 0; option < index; option++) await driver.press("ArrowDown");
    await driver.press("Enter");
  }
  async function createRuntimeJob(name, command) {
    await ui.click({ role: "button", name: "작업 및 서비스" });
    await ui.waitForTarget({ role: "button", name: "+ 새 작업" });
    await ui.click({ role: "button", name: "+ 새 작업" });
    await ui.fill({ role: "textbox", name: "작업 이름" }, name);
    await ui.fill({ role: "textbox", name: "실행 명령" }, command);
    await ui.fill({ role: "textbox", name: "작업 디렉터리" }, fixtureRoot);
    await ui.click({ role: "button", name: "작업 저장" });
    let job;
    await wait(async () => {
      job = (await read("workspace.runtime", "list_jobs")).find((item) => item.name === name);
      return !!job;
    }, "owned disabled job definition saved");
    assert.equal(job.enabled, false);
    // Persistence can precede refreshJobs and closeEditor in the real UI.
    await ui.waitForTarget({ role: "button", name: "지금 실행", scope: { role: "article", name } });
    return job;
  }
  return {
    agentFailureOperations: () => readWorkspaceAgentOperations(dataRoot),
    context,
    observeInput: (fileName) => observeWorkspaceInput({ ui, cdp, fileName, windowOwner }),
    registry: () => read("workspace.registry", "snapshot"),
    recovery: () => read("workspace.files", "load_recovery"),
    wait,
    selectOption,
    async waitForText(target) {
      await wait(async () => {
        await ui.text(target);
        return true;
      }, target.name);
    },
    async performanceTask() {
      const name = "owned performance task",
        started = performance.now();
      const job = await createRuntimeJob(name, `"${process.execPath}" -e "process.exit(0)"`);
      await ui.click({ role: "button", name: "지금 실행", scope: { role: "article", name } });
      await wait(async () => {
        const runs = await read("workspace.runtime", "list_runs", {
          jobId: job.id,
          limit: 10,
          startAt: null,
          endAt: null,
          status: null,
          kind: null,
          minDurationMs: null,
          maxDurationMs: null,
        });
        return runs.some((run) => run.jobId === job.id && run.status === "succeeded" && run.exitCode === 0);
      }, "actual representative task exits zero");
      return { taskCompleted: true, completeMs: performance.now() - started, result: "passed" };
    },
    async prepare(root) {
      await ui.click({ role: "button", name: "개요" });
      this.windowsRoot = root;
      const git = (...args) => {
        const result = spawnSync("git.exe", ["-C", root, ...args], { encoding: "utf8" });
        assert.equal(result.status, 0, "Owned Git fixture setup failed");
      };
      git("init", "-b", "main");
      git("config", "user.name", "Workspace Fixture");
      git("config", "user.email", "workspace-fixture@example.invalid");
      git("add", ".");
      git("commit", "-m", "owned fixture");
      await ui.fill({ role: "textbox", name: "Windows 프로젝트 폴더" }, root);
      await ui.click({ role: "button", name: "프로젝트 등록" });
      await selectRegisteredWorkspaceRoot(ui, () => this.registry(), wait, root);
      await waitForSelectedWorkspaceRoot(context, () => this.registry(), wait, root);
    },
    async trustSource() {
      await ui.click({ role: "button", name: "소스" });
      await ui.waitForTarget({ role: "button", name: "Git 실행 검토" });
      await ui.click({ role: "button", name: "Git 실행 검토" });
      await ui.waitForTarget({ role: "button", name: "검토한 Git 실행 승인" });
      await ui.click({ role: "button", name: "검토한 Git 실행 승인" });
      await this.waitForText({ role: "textbox", name: "커밋 메시지" });
      // Source mounts several native readers; the textbox can appear before the
      // aggregate context-change blocker settles. Observe readiness before leaving.
      await ui.waitForTarget({ role: "button", name: "Git 승인 상태 확인" });
    },
    async prepareAgent() {
      const distro = process.env.DEVBOX_KNOWLEDGE_WSL_DISTRO;
      const owner = JSON.parse(
        (await readFile(path.join(process.env.RUNNER_TEMP, "devbox-knowledge-wsl-owner.json"), "utf8")).replace(
          /^\uFEFF/,
          "",
        ),
      );
      assert.equal(owner.runId, process.env.GITHUB_RUN_ID);
      assert.equal(owner.name, distro);
      assert.match(distro, /^DevboxKnowledgeFixture-[0-9]+-[a-f0-9]{12}$/);
      agentDistro = distro;
      agentRoot = `/tmp/devbox-workspace-agent-ui-${randomUUID()}`;
      const result = spawnSync(
        "wsl.exe",
        [
          "--distribution",
          distro,
          "--exec",
          "/bin/bash",
          "-c",
          'set -eu; mkdir "$1"; cd "$1"; git init -b main; git config user.name "Workspace Fixture"; git config user.email workspace-fixture@example.invalid; printf "owned fixture\\n" > preserved.txt; git add .; git commit -m "owned fixture"',
          "fixture",
          agentRoot,
        ],
        { encoding: "utf8", timeout: 20_000 },
      );
      assert.equal(result.status, 0, "Owned WSL Git fixture setup failed");
      await ui.click({ role: "button", name: "개요" });
      await ui.waitForTarget({ role: "button", name: "WSL 프로젝트 추가" });
      await ui.click({ role: "button", name: "WSL 프로젝트 추가" });
      const distros = await read("workspace.registry", "list_wsl_distros");
      const index = distros.findIndex((item) => item.name === distro);
      assert.ok(index >= 0 && distros[index].running);
      await selectOption({ role: "combobox", name: "WSL 배포판" }, index + 1);
      await ui.fill({ role: "textbox", name: "Linux 프로젝트 폴더" }, agentRoot);
      await ui.click({ role: "button", name: "WSL 프로젝트 등록" });
      const project = await selectRegisteredWorkspaceRoot(ui, () => this.registry(), wait, agentRoot);
      this.agentProjectName = project.name;
      await waitForSelectedWorkspaceRoot(context, () => this.registry(), wait, agentRoot);
      await this.trustSource();
    },
    async configureAgent() {
      await dismissWorkspaceUndo(ui, () => cdp.evaluate("!!document.querySelector('.shell-undo')"), wait);
      await selectOption({ role: "combobox", name: "도구" }, 2);
      await ui.fill({ role: "textbox", name: "명령" }, "printf 'owned-agent-ui\\n'");
    },
    async waitForAgent() {
      await wait(async () => {
        const registry = await this.registry(),
          selected = await context();
        return (
          registry.worktrees.some((tree) => tree.id === selected?.worktreeId) &&
          !(await ui.text({ role: "region", name: "에이전트 작업" })).includes("처리 중")
        );
      }, "Agent UI settled");
    },
    async waitForAgentSourceIdle() {
      const selected = await context();
      const tree = (await this.registry()).worktrees.find(
        (item) => item.id === selected.worktreeId && item.projectId === selected.projectId,
      );
      assert.ok(tree, "Selected Agent worktree absent");
      await wait(
        () =>
          cdp.evaluate(`(() => {
        const source = document.querySelector('.workspace-feature-source .workspace-native-source');
        if (!source || !Array.from(source.querySelectorAll('p')).some(node => node.textContent === ${JSON.stringify(tree.binding.root)})) return false;
        const button = Array.from(source.querySelectorAll('button')).find(node => node.textContent.trim() === 'Git 승인 상태 확인');
        return !!button && !button.disabled;
      })()`),
        "selected Source inspection idle before Agent review",
      );
    },
    async attemptAgentReview() {
      await ui.click({ role: "button", name: "에이전트" });
      await ui.waitForTarget({ role: "button", name: "변경 검토" });
      await ui.click({ role: "button", name: "변경 검토" });
      await this.waitForText({ role: "alert", name: "" });
      return ui.text({ role: "alert", name: "" });
    },
    async selectRoot(root) {
      await ui.click({ role: "button", name: "개요" });
      await selectRegisteredWorkspaceRoot(ui, () => this.registry(), wait, root);
      await waitForSelectedWorkspaceRoot(context, () => this.registry(), wait, root);
    },
    async selectWindows() {
      assert.ok(this.windowsRoot);
      const selected = await context();
      if (
        (await this.registry()).worktrees.some(
          (tree) => tree.id === selected?.worktreeId && tree.binding.root === this.windowsRoot,
        )
      )
        return;
      await this.selectRoot(this.windowsRoot);
    },
    async runtimeLostReply() {
      await this.selectWindows();
      const name = "owned lost reply job",
        counter = path.join(fixtureRoot, "runtime-launch-count.txt");
      const script = path.join(fixtureRoot, "runtime-owned.cjs");
      await writeFile(
        script,
        `require('node:fs').appendFileSync(${JSON.stringify(counter)}, 'launch\\n');setInterval(()=>{},1000);`,
      );
      const job = await createRuntimeJob(name, `"${process.execPath}" "${script}"`);
      const scope = { role: "article", name };
      const pendingExpression = `Object.keys(localStorage).filter(key=>key.startsWith("devbox-runtime-pending:")&&key.endsWith(":"+${JSON.stringify(job.id)})).map(key=>({key,...JSON.parse(localStorage.getItem(key))}))`;
      const pending = () => cdp.evaluate(pendingExpression);
      const loseReply = (action) =>
        loseRuntimeReply({
          cdp,
          ui,
          wait,
          jobId: job.id,
          scope,
          action,
          pendingExpression,
          restart,
          beforeRestart: async () => {
            if (action === "지금 실행")
              await wait(
                async () => (await readFile(counter, "utf8")).includes("launch"),
                "owned child launched before losing reply",
              );
          },
        });
      const runId = await loseReply("지금 실행");
      assert.equal(await readFile(counter, "utf8"), "launch\n");
      const recoveredPending = await pending();
      const recoveredReceipts = await read("workspace.runtime", "list_runtime_controls", {});
      await writeFile(
        "product-foundation-evidence/workspace-runtime-restart-observation.json",
        JSON.stringify(
          {
            schemaVersion: 1,
            operationId: runId,
            jobId: job.id,
            matchingPendingCount: recoveredPending.filter((item) => item.operationId === runId).length,
            matchingNativeReceiptCount: recoveredReceipts.filter((item) => item.operationId === runId).length,
          },
          null,
          2,
        ),
        { flag: "wx" },
      );
      assertLostRuntimeReceipt(recoveredReceipts, runId, job.id, "run_job_now");
      await resumeRuntimeUi(this, ui);
      try {
        await wait(
          async () => (await cdp.evaluate("document.body.innerText")).includes("실행 요청의 완료 상태를 확인했습니다"),
          "completed request confirmation visible",
        );
      } catch (error) {
        // Fixed fields only: preserve the first failure without dumping document,
        // local-storage, command or response contents into an artifact.
        const observation = { schemaVersion: 1, operationId: runId, jobId: job.id };
        try {
          observation.renderer = await cdp.evaluate(`(()=>{
            const section=document.querySelector('[aria-label="실행 요청 복구"]');
            const tasks=document.querySelector('.workspace-feature-tasks');
            return {tasksMounted:!!tasks,tasksVisible:!!tasks&&!tasks.hidden,
              recoveryMounted:!!section,recoveryReadFailed:!!section?.querySelector('[role="alert"]'),
              completionVisible:!!section?.innerText.includes('실행 요청의 완료 상태를 확인했습니다'),
              pendingOperationPresent:(${pendingExpression}).some(item=>item.operationId===${JSON.stringify(runId)})};
          })()`);
          observation.agentStatus = await cdp.evaluate(
            "window.__TAURI_INTERNALS__.invoke('plugin:product-shell|agent_status')",
          );
        } catch {
          observation.rendererUnavailable = true;
        }
        try {
          const receipt = await read("workspace.runtime", "runtime_control_status", { operationId: runId });
          observation.originalReceiptReadable = true;
          observation.originalReceiptMatchesJob = receipt?.jobId === job.id;
        } catch (receiptError) {
          observation.originalReceiptReadable = false;
          observation.originalReceiptFailure = boundedFailure(receiptError);
          observation.originalReceiptCode =
            [
              "runtime_control_in_progress",
              "runtime_control_recovery_required",
              "runtime_control_failed",
              "runtime_control_unavailable",
              "unavailable",
            ].find((code) => String(receiptError?.message).includes(`"${code}"`)) ?? "unknown";
        }
        try {
          await writeFile(
            "product-foundation-evidence/workspace-runtime-recovery-first-failure.json",
            JSON.stringify(observation, null, 2),
            { flag: "wx" },
          );
        } catch {
          /* The scenario's original failure remains authoritative. */
        }
        throw error;
      }
      const completedRun = await read("workspace.runtime", "runtime_control_status", { operationId: runId });
      assert.equal(completedRun.jobId, job.id);
      await wait(
        async () =>
          (await pending()).length === 0 &&
          !(await read("workspace.runtime", "list_runtime_controls", {})).some((item) => item.operationId === runId),
        "lost run result reconciled read-only",
      );
      assert.equal(await readFile(counter, "utf8"), "launch\n");
      // The installed Agent preserves the first live run across a Workspace
      // crash. Respect overlap=skip: explicitly stop it before the next run.
      await stopReconciledRuntimeRun(
        ui,
        wait,
        () => read("workspace.runtime", "get_active_run", { id: job.id }),
        scope,
      );
      // A new explicit run now has a new request. Stop's reply is independently lost.
      await ui.click({ role: "button", name: "지금 실행", scope });
      await wait(async () => (await readFile(counter, "utf8")) === "launch\nlaunch\n", "second explicit owned run");
      await waitForRuntimeControlIdle(
        pending,
        () => read("workspace.runtime", "list_runtime_controls", {}),
        wait,
        job.id,
      );
      const stopId = await loseReply("중지");
      assertLostRuntimeReceipt(
        await read("workspace.runtime", "list_runtime_controls", {}),
        stopId,
        job.id,
        "stop_active_run",
      );
      await resumeRuntimeUi(this, ui);
      const completedStop = await read("workspace.runtime", "runtime_control_status", { operationId: stopId });
      assert.ok(completedStop === null || completedStop.jobId === job.id);
      await wait(
        async () =>
          (await pending()).length === 0 &&
          !(await read("workspace.runtime", "list_runtime_controls", {})).some((item) => item.operationId === stopId),
        "lost stop result reconciled read-only",
      );
      assert.equal(await readFile(counter, "utf8"), "launch\nlaunch\n");
      assert.equal(await read("workspace.runtime", "get_active_run", { id: job.id }), null);
      return {
        assertions: [
          "Real native run and stop callbacks were paused before renderer consumption then owned process crashed",
          "Existing durable operation IDs were read and reconciled after restart without another control submission",
          "Only two explicit launches occurred; no replayed launch or unrelated resource was stopped",
        ],
        screenshots: [await ui.screenshot("workspace-runtime-lost-reply-reconciled")],
      };
    },
    async dependencyRefresh() {
      await this.selectWindows();
      const root = this.windowsRoot,
        lock = path.join(root, "package-lock.json");
      const manifest = {
        name: "owned-ui-fixture",
        version: "1.0.0",
        dependencies: { "fixture-dependency": "1.2.3" },
        scripts: { sentinel: "echo do-not-execute" },
      };
      await writeFile(path.join(root, "package.json"), JSON.stringify(manifest));
      const writeLock = (version) =>
        writeFile(
          lock,
          JSON.stringify({
            name: manifest.name,
            version: "1.0.0",
            lockfileVersion: 3,
            packages: { "": manifest, "node_modules/fixture-dependency": { version } },
          }),
        );
      await writeLock("1.2.3");
      await ui.click({ role: "button", name: "의존성" });
      await ui.waitForTarget({ role: "button", name: "의존성 분석" });
      await ui.click({ role: "button", name: "의존성 분석" });
      const inventory = () => read("workspace.dependencies", "dependency_inventory", { request: { path: root } });
      const initial = await inventory();
      assert.ok(initial.packages.some((item) => item.version === "1.2.3"));
      await ui.click({ role: "button", name: "전송 내용 검토" });
      await this.waitForText({ role: "region", name: "원격 전송 검토" });
      await ui.click({ role: "button", name: "전송 검토 취소" });
      await ui.click({ role: "button", name: "전송 내용 검토" });
      await this.waitForText({ role: "region", name: "원격 전송 검토" });
      await writeLock("1.2.4");
      await ui.click({ role: "button", name: "검토한 정보 보내기" });
      await this.waitForText({ role: "button", name: "lockfile 다시 분석" });
      await ui.click({ role: "button", name: "lockfile 다시 분석" });
      const fresh = await inventory();
      assert.notEqual(fresh.revision, initial.revision);
      assert.ok(fresh.packages.some((item) => item.version === "1.2.4"));
      // Approved transmission reaches only the owned failure proxy, retaining local inventory.
      const attempts = network.attempts();
      await ui.click({ role: "button", name: "전송 내용 검토" });
      await this.waitForText({ role: "region", name: "원격 전송 검토" });
      await ui.click({ role: "button", name: "검토한 정보 보내기" });
      await network.waitForAttempt(attempts);
      await wait(
        async () =>
          (await cdp.evaluate('document.querySelector(".dependency-lens-panel")?.textContent ?? ""')).includes("실패"),
        "provider failure shown with local report",
      );
      assert.ok((await inventory()).packages.some((item) => item.version === "1.2.4"));
      assert.deepEqual(JSON.parse(await readFile(path.join(root, "package.json"), "utf8")), manifest);
      return {
        assertions: [
          "Actual cancellation releases the reviewed transmission without sending",
          "Lockfile mutation invalidates old preview; local reanalysis obtains new revision before new approval",
          "Owned OSV/deps.dev proxy failures preserve local package inventory and never execute package scripts",
        ],
        screenshots: [await ui.screenshot("workspace-dependencies-safe-refresh")],
      };
    },
    async terminalLifecycle() {
      const owner = JSON.parse(
        (await readFile(path.join(process.env.RUNNER_TEMP, "devbox-knowledge-wsl-owner.json"), "utf8")).replace(
          /^\uFEFF/,
          "",
        ),
      );
      assert.equal(owner.name, agentDistro);
      return withOwnedWslRunning(owner, process.env.GITHUB_RUN_ID, () => this.terminalLifecycleWhileRunning());
    },
    async terminalLifecycleWhileRunning() {
      assert.ok(agentRoot && this.agentProjectName);
      await ui.click({ role: "button", name: "개요" });
      try {
        await selectRegisteredWorkspaceRoot(ui, () => this.registry(), wait, agentRoot);
        await waitForSelectedWorkspaceRoot(context, () => this.registry(), wait, agentRoot);
      } catch (error) {
        const observation = {
          schemaVersion: 1,
          selection: await observeWorkspaceSelection({ context, registry: () => this.registry(), root: agentRoot }),
        };
        try {
          observation.guards = await cdp.evaluate(`(() => {
            const text = Array.from(document.querySelectorAll('[role="alert"],[role="status"]')).slice(0,32).map(node => node.textContent).join('\\n');
            return {runtimeRecoveryAlert:!!document.querySelector('[aria-label="실행 요청 복구"] [role="alert"]'),runtimeRecoveryItems:document.querySelectorAll('[aria-label="실행 요청 복구"] li').length,...Object.fromEntries(Object.entries({tasksDirty:'Tasks 편집',filesDirty:'Files 편집 또는 저장',definitionsDirty:'프로젝트 정의 편집',dependenciesBusy:'의존성 검토',sourceBusy:'Source 작업',sourceDirty:'Source 초안',transitionPending:'프로젝트 전환을 확인 중입니다',contextBusy:'파일 또는 Git 작업이 진행 중입니다',runtimePending:'이전 실행 요청의 상태를 먼저 확인',runtimeRecovery:'완료되지 않은 실행 요청'}).map(([key,value]) => [key,text.includes(value)]))};
          })()`);
        } catch {
          observation.guardsUnavailable = true;
        }
        try {
          observation.operations = await readWorkspaceAgentOperations(dataRoot);
        } catch {
          observation.operationsUnavailable = true;
        }
        try {
          await writeFile(
            "product-foundation-evidence/workspace-runtime-context-first-failure.json",
            JSON.stringify(observation, null, 2),
            { flag: "wx" },
          );
        } catch {
          /* Preserve the original selection failure. */
        }
        throw error;
      }
      const counter = `${agentRoot}/terminal-count`,
        afterInterrupt = `${agentRoot}/ctrl-c-confirmed`;
      const wslRead = (file) => {
        const response = spawnSync("wsl.exe", ["--distribution", agentDistro, "--exec", "/bin/cat", file], {
          encoding: "utf8",
          timeout: 5000,
        });
        if (response.status !== 0) throw new Error("Owned WSL terminal marker unavailable");
        return response.stdout;
      };
      const sessions = () => read("workspace.terminal", "terminal_sessions");
      const initial = await sessions();
      await openCurrentProjectTerminal(ui);
      let opened;
      await wait(async () => {
        opened = (await sessions()).find(
          (session) => session.state === "active" && !initial.some((prior) => prior.id === session.id),
        );
        return !!opened;
      }, "new owned terminal active");
      const scoped = async (id) => ({
        role: "listitem",
        name: `${(await sessions()).findIndex((session) => session.id === id) + 1}번째 터미널`,
      });
      let surface = await terminalUi(opened.id);
      try {
        const command = `printf 'launch\\n' >> '${counter}'; sleep 300`;
        await prepareTerminalStart(surface.ui, agentRoot, command);
        await wait(async () => wslRead(counter) === "launch\n", "explicit terminal start command");
        await surface.ui.press("Control+c");
        await surface.ui.typeText(`printf 'ctrl-c\\n' > '${afterInterrupt}'`);
        await surface.ui.press("Enter");
        await wait(async () => wslRead(afterInterrupt) === "ctrl-c\n", "Ctrl+C returns the exact owned shell");
        await surface.ui.click({ role: "button", name: "현재 상태 저장" });
        await surface.ui.waitForTarget({ role: "textbox", name: "프로필 이름" });
        await surface.ui.fill({ role: "textbox", name: "프로필 이름" }, "owned terminal profile");
        await surface.ui.click({ role: "button", name: "저장" });
        await wait(
          async () =>
            (await read("workspace.terminal", "terminal_commands")).profiles.some(
              (profile) => profile.name === "owned terminal profile",
            ),
          "actual terminal profile saved",
        );
        await ui.click({ role: "button", name: "상태 새로고침" });
        await selectOption({ role: "combobox", name: "저장한 프로필" }, 1);
        // Native X hides the companion; it must not stop its generation.
        await surface.ui.closeOwnedWindow();
        assert.equal((await sessions()).find((session) => session.id === opened.id).state, "active");
        await ui.click({ role: "button", name: "창 표시", scope: await scoped(opened.id) });
        await deleteTerminalProfile(surface, ui, wait);
        await this.waitForText({ role: "alert", name: "" });
        assert.equal(wslRead(counter), "launch\n");
      } catch (error) {
        try {
          await surface.ui.screenshot("RUNTIME-02-terminal-first-failure");
        } catch {
          /* preserve the original failure */
        }
        throw error;
      } finally {
        surface.close();
      }
      await restart(true);
      await ui.click({ role: "button", name: "터미널" });
      await wait(
        async () => (await sessions()).find((session) => session.id === opened.id)?.state === "interrupted",
        "interrupted original generation",
      );
      await ui.click({ role: "button", name: "상태만 다시 연결", scope: await scoped(opened.id) });
      await wait(
        async () => (await sessions()).find((session) => session.id === opened.id)?.state === "active",
        "explicit restore generation active",
      );
      assert.equal(wslRead(counter), "launch\n", "Restoration must never replay the saved start command");
      surface = await terminalUi(opened.id);
      try {
        const screenshot = await surface.ui.screenshot("workspace-terminal-explicit-restore");
        await ui.click({ role: "button", name: "이 터미널 종료", scope: await scoped(opened.id) });
        await wait(
          async () => (await sessions()).find((session) => session.id === opened.id)?.state === "stopped",
          "exact owned terminal stopped",
        );
        return {
          assertions: [
            "Actual Ctrl+C interrupts only the owned foreground command and returns the same shell",
            "Native companion X hides its active generation; changed/deleted profile is rejected without launching",
            "Crash/reopen restores only the reviewed session generation, never replays saved start command, and explicit stop targets that session",
          ],
          screenshots: [screenshot],
        };
      } finally {
        surface.close();
      }
    },
    async closeFailedLsp() {
      const confirmationOpen = () =>
        cdp.evaluate(`!!document.querySelector('[role="dialog"][aria-label="관리형 서버 작업 확인"]')`);
      if (await confirmationOpen()) {
        const cancel = { role: "button", name: "취소", scope: { role: "dialog", name: "관리형 서버 작업 확인" } };
        await ui.waitForTarget(cancel);
        await ui.click(cancel);
        await wait(async () => !(await confirmationOpen()), "failed LSP confirmation dismissed");
      }
      const panelOpen = () => cdp.evaluate(`!!document.querySelector('[role="dialog"][aria-label="언어 서버 설정"]')`);
      if (await panelOpen()) {
        const close = { role: "button", name: "닫기", scope: { role: "dialog", name: "언어 서버 설정" } };
        await ui.waitForTarget(close);
        await ui.click(close);
        await wait(async () => !(await panelOpen()), "failed LSP panel dismissed");
      }
    },
    async managedLspLifecycle() {
      await this.selectWindows();
      await ui.click({ role: "button", name: "파일" });
      await ui.waitForTarget({ role: "button", name: "언어 서버" });
      await ui.click({ role: "button", name: "언어 서버" });
      const catalog = await read("workspace.lsp", "lsp_catalog");
      const rust = catalog.find((item) => item.id === "rust-analyzer"),
        node = catalog.find((item) => item.id === "typescript-language-server");
      assert.ok(rust && node);
      const archiveRoot = path.join(fixtureRoot, "reviewed-lsp-archives");
      await mkdir(archiveRoot);
      const archive = await downloadArchive(rust.artifact, path.join(archiveRoot, "rust-analyzer.zip"));
      const scope = { role: "article", name: `${rust.id} ${rust.version}` };
      const state = async (item) =>
        (await read("workspace.lsp", "lsp_installed")).find(
          (status) => status.manifest_id === item.id && status.version === item.version,
        );
      await observeManagedLspTransition(ui, scope, "설치", async () =>
        assert.equal((await state(rust)).state, "not_installed"),
      );
      await ui.click({ role: "button", name: "설치", scope });
      const confirmation = { role: "dialog", name: "관리형 서버 작업 확인" };
      assert.ok((await ui.text(confirmation)).includes(rust.artifact.sha256));
      await ui.click({ role: "button", name: "취소", scope: confirmation });
      assert.equal((await state(rust)).state, "not_installed");
      await ui.click({ role: "button", name: "local archive 가져오기", scope });
      await chooseArchive(archive);
      await this.waitForText({ role: "button", name: "가져오기 확인" });
      assert.ok((await ui.text(confirmation)).includes(rust.artifact.sha256));
      await ui.click({ role: "button", name: "가져오기 확인" });
      // Native commit can precede the response and renderer status refresh.
      // Do not overwrite an import error with the later intentional download failure.
      try {
        await observeManagedLspTransition(
          ui,
          scope,
          "제거",
          () =>
            wait(async () => (await state(rust)).state === "installed", "digest-verified archive imported", 120_000),
          120_000,
        );
      } catch (error) {
        try {
          await writeFile(
            "product-foundation-evidence/workspace-lsp-import-first-failure.json",
            JSON.stringify(
              {
                schemaVersion: 1,
                operations: await readWorkspaceAgentOperations(dataRoot),
                ui: await cdp.evaluate(
                  `(() => { const text = document.querySelector('[aria-label="언어 서버 설정"]')?.textContent ?? ''; return {importFailed:text.includes('local archive를 가져오지 못했습니다.'), statusFailed:text.includes('관리형 서버 상태를 확인하지 못했습니다.'), confirmationOpen:!!document.querySelector('[aria-label="관리형 서버 작업 확인"]')}; })()`,
                ),
              },
              null,
              2,
            ),
            { flag: "wx" },
          );
        } catch {
          /* Preserve original readiness failure. */
        }
        throw error;
      }
      const imported = await state(rust);
      assert.equal(imported.installed.sha256, rust.artifact.sha256);
      assert.equal(imported.installed.install_source, "local_archive");
      const pointer = JSON.parse(await readFile(path.join(dataRoot, "active-stores.json"), "utf8"));
      assert.match(pointer.id, /^[a-f0-9-]{36}$/);
      const lspRoot = path.join(dataRoot, "stores", pointer.id, "files", "lsp");
      const installed = path.join(lspRoot, "servers", rust.id, rust.version, rust.platform);
      const before = (await readdir(installed)).sort();
      assert.ok(before.length);
      const beforeAttempts = network.attempts();
      await ui.click({ role: "button", name: "설치", scope: { role: "article", name: `${node.id} ${node.version}` } });
      await ui.click({ role: "button", name: "설치 확인" });
      await network.waitForAttempt(beforeAttempts);
      await dismissFailedManagedInstall({
        ui,
        wait,
        isOpen: () =>
          cdp.evaluate(`document.querySelector('[role="dialog"][aria-label="관리형 서버 작업 확인"]') !== null`),
      });
      await this.waitForText({ role: "button", name: "설치 상태 새로 고침" });
      assert.equal((await state(node)).state, "not_installed");
      assert.equal((await state(rust)).installed.sha256, imported.installed.sha256);
      assert.deepEqual((await readdir(installed)).sort(), before);
      async function assertNoPartial(directory) {
        for (const entry of await readdir(directory, { withFileTypes: true })) {
          assert.ok(!entry.isSymbolicLink());
          assert.ok(!entry.name.includes(".partial"));
          if (entry.isDirectory()) await assertNoPartial(path.join(directory, entry.name));
        }
      }
      await assertNoPartial(lspRoot);
      await ui.waitForTarget({ role: "button", name: "제거", scope });
      await ui.click({ role: "button", name: "제거", scope });
      await ui.click({ role: "button", name: "제거 확인" });
      await observeManagedLspTransition(ui, scope, "설치", () =>
        wait(async () => (await state(rust)).state === "not_installed", "exact indexed installation removed"),
      );
      await assert.rejects(readdir(installed));
      assert.equal((await state(rust)).archive_cached, true);
      // A cancelled cached reinstall stays absent; approved cache reinstall creates the exact reviewed version.
      await ui.waitForTarget({ role: "button", name: "설치", scope });
      await ui.click({ role: "button", name: "설치", scope });
      await ui.click({ role: "button", name: "취소", scope: confirmation });
      assert.equal((await state(rust)).state, "not_installed");
      const cachedAttempts = network.attempts();
      await ui.click({ role: "button", name: "설치", scope });
      await ui.click({ role: "button", name: "설치 확인" });
      await observeManagedLspTransition(
        ui,
        scope,
        "제거",
        () => wait(async () => (await state(rust)).state === "installed", "cached exact server installed", 120_000),
        120_000,
      );
      assert.equal((await state(rust)).installed.install_source, "archive_cache");
      assert.equal(network.attempts(), cachedAttempts);
      const screenshot = await ui.screenshot("workspace-lsp-reviewed-lifecycle");
      await ui.waitForTarget({ role: "button", name: "제거", scope });
      await ui.click({ role: "button", name: "제거", scope });
      await ui.click({ role: "button", name: "제거 확인" });
      await observeManagedLspTransition(ui, scope, "설치", () =>
        wait(async () => (await state(rust)).state === "not_installed", "owned server removed after cache proof"),
      );
      await ui.press("Escape");
      return {
        assertions: [
          "Actual UI review shows fixed catalog digest before archive import and cached installation",
          "Cancelled confirmation and failed owned offline download clean partial artifacts while preserving existing verified installation",
          "Exact indexed uninstall removes only owned server directory and retains verified archive cache",
        ],
        screenshots: [screenshot],
      };
    },
    async crashAndReopen() {
      await restart(true);
    },
    async reopenAfterClose() {
      await waitForExit();
      await restart(false);
    },
    async failRecoveryWriter() {
      assert.equal(fault, null);
      const selected = await context();
      assert.match(selected.worktreeId, /^[a-f0-9-]{36}$/);
      const pointer = JSON.parse(await readFile(path.join(dataRoot, "active-stores.json"), "utf8"));
      assert.match(pointer.id, /^[a-f0-9-]{36}$/);
      const parts = ["stores", pointer.id, "files", "views", `worktree-${selected.worktreeId}`];
      let view = dataRoot;
      for (const part of parts) {
        view = path.join(view, part);
        const info = await lstat(view);
        assert.ok(info.isDirectory() && !info.isSymbolicLink(), "Owned recovery view redirected");
      }
      const file = path.join(view, "recovery.json"),
        backup = path.join(view, "recovery-ui-fixture-backup.json");
      await readFile(file);
      await rename(file, backup);
      await mkdir(file);
      fault = { file, backup };
    },
    async restoreRecoveryWriter() {
      if (!fault) return;
      await rm(fault.file, { recursive: true });
      await rename(fault.backup, fault.file);
      fault = null;
    },
    async cleanup() {
      await this.restoreRecoveryWriter();
      await cleanup();
    },
    async dispose() {
      await this.cleanup();
      if (agentRoot) {
        assert.match(agentRoot, /^\/tmp\/devbox-workspace-agent-ui-[a-f0-9-]{36}$/);
        const result = spawnSync(
          "wsl.exe",
          ["--distribution", agentDistro, "--exec", "/bin/rm", "-rf", "--", agentRoot],
          { encoding: "utf8", timeout: 15_000 },
        );
        assert.equal(result.status, 0, "Owned WSL fixture cleanup failed");
        agentRoot = null;
      }
    },
  };
}
