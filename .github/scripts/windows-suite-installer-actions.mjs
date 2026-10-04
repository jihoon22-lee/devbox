// Exact owned NSIS windows; no silent flags and no activation helper shortcuts.
import assert from "node:assert/strict";
import { copyFile, readFile, mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { randomUUID } from "node:crypto";
import { captureWindowOwner, nativeWindowAction } from "./windows-user-flow-window.mjs";
import { allWindowsProcesses } from "./windows-packaged-smoke.mjs";
import { observeUntil } from "./windows-suite-ui-context.mjs";
import { fileDigest } from "./suite-user-flow-results.mjs";
export function ownedNsisSpawnOptions(image, args) {
  assert.ok(path.win32.isAbsolute(image) && !/["\r\n]/.test(image));
  assert.ok(Array.isArray(args) && args.length >= 1);
  assert.ok(args.slice(0, -1).every((flag) => flag === "/S"));
  const last = args.at(-1),
    match = /^(?:\/D=|_\?=)(.+)$/.exec(last);
  assert.ok(
    match && path.win32.isAbsolute(match[1]) && !/["\r\n]/.test(match[1]),
    "Final unquoted absolute NSIS path required",
  );
  return { windowsVerbatimArguments: true, argv0: `"${image}"` };
}
export async function startOwnedInstaller(image, args, fixtureRoot, env = process.env) {
  assert.equal(process.platform, "win32");
  assert.equal(process.env.GITHUB_ACTIONS, "true");
  assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  const child = spawn(image, args, { env, stdio: "ignore", windowsHide: false, ...ownedNsisSpawnOptions(image, args) });
  await once(child, "spawn");
  let identity;
  await observeUntil(() => {
    identity = allWindowsProcesses().find((p) => p.Pid === child.pid && p.Path.toLowerCase() === image.toLowerCase());
    return !!identity;
  }, "owned NSIS process");
  const owner = captureWindowOwner(identity, fixtureRoot);
  const inspect = () => {
    try {
      return nativeWindowAction(owner, "Inspect");
    } catch {
      return null;
    }
  };
  const wait = async (test, label, timeout = 180000) => observeUntil(() => test(inspect()), label, timeout);
  return { child, owner, wait, inspect, invoke: (id) => nativeWindowAction(owner, "Invoke", { controlId: id }) };
}
// Fixed source issue tokens only: never preserve arbitrary native labels or paths.
const installerIssues = [
  "bootstrap_arguments_invalid",
  "bootstrap_busy",
  "bootstrap_clock_invalid",
  "bootstrap_current_uninstall_failed",
  "bootstrap_data_restore_pending",
  "bootstrap_data_unavailable",
  "bootstrap_data_unsafe",
  "bootstrap_directory_unavailable",
  "bootstrap_directory_unsafe",
  "bootstrap_dispatcher_untrusted",
  "bootstrap_existing_installation_conflict",
  "bootstrap_file_changed",
  "bootstrap_file_unavailable",
  "bootstrap_file_unsafe",
  "bootstrap_gate_changed",
  "bootstrap_gate_unavailable",
  "bootstrap_gate_unsafe",
  "bootstrap_generation_review_required",
  "bootstrap_health_required",
  "bootstrap_helper_busy",
  "bootstrap_identity_missing",
  "bootstrap_identity_unavailable",
  "bootstrap_input_changed",
  "bootstrap_input_copy_expired",
  "bootstrap_input_copy_failed",
  "bootstrap_input_unavailable",
  "bootstrap_input_unsafe",
  "bootstrap_journal_changed",
  "bootstrap_journal_missing",
  "bootstrap_launch_failed",
  "bootstrap_lock_unsafe",
  "bootstrap_manifest_changed",
  "bootstrap_manifest_invalid",
  "bootstrap_manifest_write_failed",
  "bootstrap_marker_invalid",
  "bootstrap_marker_write_failed",
  "bootstrap_mcp_launcher_changed",
  "bootstrap_owner_changed",
  "bootstrap_owner_invalid",
  "bootstrap_owner_unavailable",
  "bootstrap_payload_changed",
  "bootstrap_payload_incomplete",
  "bootstrap_payload_unavailable",
  "bootstrap_payload_write_failed",
  "bootstrap_phase_requires_recovery",
  "bootstrap_receipt_invalid",
  "bootstrap_receipt_unavailable",
  "bootstrap_recovery_conflict",
  "bootstrap_recovery_requires_data_plan",
  "bootstrap_recovery_unavailable",
  "bootstrap_restart_requires_recovery",
  "bootstrap_restore_journal_changed",
  "bootstrap_restore_journal_missing",
  "bootstrap_restore_plan_failed",
  "bootstrap_restore_retention_review_required",
  "bootstrap_revision_exhausted",
  "bootstrap_root_changed",
  "bootstrap_root_not_empty",
  "bootstrap_root_unavailable",
  "bootstrap_root_unsafe",
  "bootstrap_stage_incomplete",
  "bootstrap_stage_recovery_required",
  "bootstrap_stage_retention_review_required",
  "bootstrap_stage_unavailable",
  "bootstrap_stage_unrecognized",
  "bootstrap_stage_unsafe",
  "bootstrap_store_preparation_required",
  "bootstrap_uninstall_external_helper_required",
  "bootstrap_uninstall_pending",
  "bootstrap_update_pending",
  "bootstrap_version_mismatch",
  "bootstrap_windows_required",
  "installation_cancelled",
  "suite_health_required",
  "suite_registration_changed",
  "suite_registration_incomplete",
  "suite_registration_invalid",
  "suite_registration_unavailable",
  "suite_registry_foreign",
  "suite_registry_unavailable",
  "suite_reinstall_cleanup_required",
  "suite_remove_file_changed",
  "suite_remove_file_unavailable",
  "suite_remove_path_unsafe",
  "suite_remove_root_changed",
  "suite_remove_plan_invalid",
  "suite_remove_plan_unavailable",
  "suite_shortcut_com_unavailable",
  "suite_shortcut_conflict",
  "suite_shortcut_directory_changed",
  "suite_shortcut_directory_conflict",
  "suite_shortcut_directory_unavailable",
  "suite_shortcut_directory_unsafe",
  "suite_shortcut_foreign",
  "suite_shortcut_invalid",
  "suite_shortcut_unavailable",
  "suite_shortcut_unsafe",
  "suite_space_insufficient",
  "suite_space_unavailable",
  "suite_uninstaller_invalid",
  "suite_uninstaller_missing",
  "suite_writers_must_close",
  "update_activation_invalid",
  "update_agent_busy",
  "update_already_installed",
  "update_caller_untrusted",
  "update_claim_changed",
  "update_claim_invalid",
  "update_commit_started",
  "update_committed_installation_required",
  "update_journal_changed",
  "update_manifest_changed",
  "update_operation_conflict",
  "update_operation_invalid",
  "update_other_generation_pending",
  "update_other_recovery_pending",
  "update_owner_changed",
  "update_owner_invalid",
  "update_plan_changed",
  "update_plan_invalid",
  "update_previous_operation_incomplete",
  "update_progress_invalid",
  "update_record_invalid",
  "update_record_write_failed",
  "update_records_changed",
  "update_resume_selected_action",
  "update_retention_review_required",
  "update_revision_exhausted",
  "update_root_changed",
  "update_store_unavailable",
  "update_version_invalid",
];
export function installerFailureObservation(view, stage, exitCode) {
  const controls = Array.isArray(view?.controls) ? view.controls.slice(0, 256) : [];
  const text = controls.map((control) => String(control.name ?? "").slice(0, 4096)).join("\n");
  const statuses = [
    ["preparation_failed", "설치를 준비하지 못했습니다"],
    ["registration_failed", "설치 항목과 바로가기를 등록하지 못했습니다"],
    ["prepared", "Devbox 설치 준비 완료"],
  ]
    .filter(([, label]) => text.includes(label))
    .map(([status]) => status);
  return {
    stage,
    exitCode,
    inspectionUnavailable: !view,
    windowCount: Number.isInteger(view?.windowCount) ? view.windowCount : null,
    selectedWindowCount: Number.isInteger(view?.selectedWindowCount) ? view.selectedWindowCount : null,
    statuses,
    issues: installerIssues.filter((issue) => new RegExp(`(^|[^a-z0-9_])${issue}([^a-z0-9_]|$)`).test(text)),
    controls: controls.map(({ id, enabled, visible }) => ({
      id: /^\d{1,8}$/.test(String(id ?? "")) ? String(id) : null,
      enabled: enabled === true,
      visible: visible === true,
    })),
  };
}
// Failure diagnostics may reveal NSIS Details, but cannot change the first failure.
export function inspectInstallerFailure(installer, stage) {
  const view = installer.inspect();
  const baseline = installerFailureObservation(view, stage, installer.child.exitCode);
  if (
    baseline.issues.length === 0 &&
    baseline.statuses.some((status) => status === "registration_failed" || status === "preparation_failed") &&
    view?.buttons?.some((button) => button.id === "1027" && button.enabled && button.visible)
  ) {
    try {
      installer.invoke("1027");
      const expanded = installerFailureObservation(installer.inspect(), stage, installer.child.exitCode);
      return { ...expanded, statuses: [...new Set([...baseline.statuses, ...expanded.statuses])] };
    } catch {
      return baseline;
    }
  }
  return baseline;
}
export async function runVisibleSetup(setup, root, env = process.env) {
  const scratch = path.dirname(root),
    image = path.join(scratch, `setup-${randomUUID()}.exe`);
  await copyFile(setup, image);
  assert.equal(await fileDigest(image), await fileDigest(setup));
  const installer = await startOwnedInstaller(image, [`/D=${root}`], scratch, env);
  let stage = "welcome";
  try {
    await installer.wait((view) => view?.buttons?.some((b) => b.id === "1" && b.enabled && b.visible), "welcome");
    installer.invoke("1");
    stage = "directory";
    await installer.wait((view) => view?.controls?.some((c) => c.id === "1019" && c.visible), "directory");
    installer.invoke("1");
    stage = "installation finish";
    await installer.wait(
      (view) => view?.controls?.some((c) => c.name === "Devbox 설치 준비 완료" && c.visible),
      "installation finish",
    );
    installer.invoke("1");
    stage = "NSIS finish exit";
    await observeUntil(() => installer.child.exitCode !== null, "NSIS finish exit");
    assert.equal(installer.child.exitCode, 0);
    return JSON.parse(await readFile(path.join(root, "suite-registration.json"), "utf8"));
  } catch (error) {
    // Capture before caller cleanup. Diagnostics must never replace the first error.
    try {
      const evidence = inspectInstallerFailure(installer, stage);
      error.installerObservation = evidence;
      const evidenceRoot = "product-foundation-evidence";
      await mkdir(evidenceRoot, { recursive: true });
      await writeFile(
        path.join(evidenceRoot, `installer-first-failure-${randomUUID()}.json`),
        JSON.stringify({ schemaVersion: 1, status: "FAIL", ...evidence }, null, 2),
        { flag: "wx" },
      );
    } catch {}
    throw error;
  }
}
export async function runVisibleRemoval(root, { cancel = false } = {}) {
  const scratch = path.dirname(root),
    source = path.join(root, "Uninstall.exe"),
    image = path.join(scratch, `uninstall-${randomUUID()}.exe`);
  await copyFile(source, image);
  assert.equal(await fileDigest(image), await fileDigest(source));
  // NSIS _?= selects its already-owned installation and avoids an untracked
  // temporary executable. This is the same interactive confirmation/sections.
  const installer = await startOwnedInstaller(image, [`_?=${root}`], scratch);
  await installer.wait(
    (view) => view?.buttons?.some((b) => b.id === "1" && b.enabled && b.visible),
    "removal confirmation",
  );
  if (cancel) {
    installer.invoke("2");
    await observeUntil(() => installer.child.exitCode !== null, "removal cancelled");
    return;
  }
  installer.invoke("1");
  await observeUntil(
    async () => {
      try {
        return Boolean(JSON.parse(await readFile(path.join(root, "uninstall-complete.json"), "utf8")));
      } catch {
        return false;
      }
    },
    "owned removal receipt",
    180000,
  );
  await installer.wait((view) => view?.buttons?.some((b) => b.id === "1" && b.enabled && b.visible), "removal finish");
  installer.invoke("1");
  await observeUntil(() => installer.child.exitCode !== null, "removal closed");
  assert.equal(installer.child.exitCode, 0);
}

export function rejectedInstallerIssue(view, expectedIssues) {
  const text = (view?.controls ?? []).map((control) => control.name ?? "").join("\n");
  return expectedIssues.find((issue) => new RegExp(`(^|[^a-z0-9_])${issue}([^a-z0-9_]|$)`).test(text)) ?? null;
}
export function directorySpaceRejected(view, proof) {
  if (!proof || !Number.isFinite(proof.availableBytes) || proof.availableBytes >= 128 * 1024 * 1024) return false;
  const labels = (view?.controls ?? [])
    .filter((control) => control.visible)
    .map((control) => control.name ?? "")
    .join("\n");
  return (
    view?.buttons?.some((button) => button.id === "1" && button.visible && !button.enabled) &&
    /필요.*(?:공간|디스크)/.test(labels) &&
    /(?:사용 가능|남은).*(?:공간|디스크)/.test(labels)
  );
}
export async function rejectVisibleSetup(setup, root, { expectedIssues, onRejected, spaceProof } = {}) {
  assert.ok(Array.isArray(expectedIssues) && expectedIssues.length > 0);
  assert.ok(expectedIssues.every((issue) => /^[a-z_]+$/.test(issue)));
  const scratch = path.dirname(process.env.DEVBOX_USER_FLOW_INSTALL_ROOT),
    image = path.join(scratch, `rejected-setup-${randomUUID()}.exe`);
  await copyFile(setup, image);
  assert.equal(await fileDigest(image), await fileDigest(setup));
  const installer = await startOwnedInstaller(image, [`/D=${root}`], scratch);
  try {
    await installer.wait(
      (view) => view?.buttons?.some((b) => b.id === "1" && b.enabled && b.visible),
      "rejected setup welcome",
    );
    installer.invoke("1");
    await installer.wait(
      (view) => view?.controls?.some((control) => control.id === "1019" && control.visible),
      "rejected setup directory",
    );
    let observed = directorySpaceRejected(installer.inspect(), spaceProof) ? "nsis_directory_space_insufficient" : null,
      detailsOpened = false;
    if (!observed) installer.invoke("1");
    if (!observed)
      await installer.wait((view) => {
        if (
          !detailsOpened &&
          view?.buttons?.some((button) => button.id === "1027" && button.enabled && button.visible)
        ) {
          installer.invoke("1027");
          detailsOpened = true;
          return false;
        }
        observed = rejectedInstallerIssue(view, expectedIssues);
        return !!observed;
      }, "owned installer displays expected native preparation issue");
    if (onRejected) await onRejected(installer, observed);
    installer.invoke("2");
    await observeUntil(() => {
      if (installer.child.exitCode !== null) return true;
      const view = installer.inspect();
      if (view?.windows?.length > 1) installer.invoke("6");
      return false;
    }, "rejected setup cancelled");
    assert.notEqual(installer.child.exitCode, 0, "Rejected setup cannot report success");
    return { issue: observed, exitCode: installer.child.exitCode };
  } finally {
    if (installer.child.exitCode === null) {
      installer.invoke("2");
      await observeUntil(() => {
        if (installer.child.exitCode !== null) return true;
        if (installer.inspect()?.windows?.length > 1) installer.invoke("6");
        return false;
      }, "owned failed installer normal cancellation");
    }
  }
}
export async function rejectBusyVisibleUpdate(setup, root) {
  return rejectVisibleSetup(setup, root, { expectedIssues: ["suite_writers_must_close", "update_agent_busy"] });
}
