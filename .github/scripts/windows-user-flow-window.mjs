// UI Automation is scoped to the PID, creation time and image this fixture owns.
import assert from "node:assert/strict";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { allWindowsProcesses } from "./windows-packaged-smoke.mjs";
export function productWindowForImage(image, fixtureRoot) {
  const name = path.win32.basename(image);
  const standard = /^devbox-(workspace|api-studio|knowledge|control-center)\.exe$/i.exec(name);
  if (standard) return standard[1].toLowerCase();
  const direct = /^direct-(workspace|api-studio|knowledge|control-center)-[a-z0-9]{6}\.exe$/i.exec(name);
  const directory = path.win32.dirname(image);
  if (
    direct &&
    path.win32.basename(directory).toLowerCase() === name.slice(0, -4).toLowerCase() &&
    path.win32.dirname(directory).toLowerCase() === path.win32.resolve(fixtureRoot).toLowerCase() &&
    /^devbox-suite-delivery-[a-f0-9]{32}$/i.test(path.win32.basename(fixtureRoot))
  )
    return direct[1].toLowerCase();
  return undefined;
}
export function captureWindowOwner(identity, fixtureRoot) {
  const current = allWindowsProcesses().find(
    (p) =>
      p.Pid === identity.Pid && p.Created === identity.Created && p.Path.toLowerCase() === identity.Path.toLowerCase(),
  );
  assert.ok(current, "Owned window process identity changed");
  const result = spawnSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      `[Diagnostics.Process]::GetProcessById(${identity.Pid}).StartTime.ToUniversalTime().ToString('o')`,
    ],
    { encoding: "utf8", timeout: 10000 },
  );
  assert.equal(result.status, 0, "Owned window start time unavailable");
  const started = result.stdout.trim();
  assert.match(started, /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{7}Z$/);
  const productWindow = productWindowForImage(identity.Path, fixtureRoot);
  return { identity, fixtureRoot, started, productWindow };
}
export function nativeWindowAction(
  owner,
  action,
  { controlId, controlName, filePath, windowName, auxiliaryWindow, width, height } = {},
) {
  if (["ZoomIn", "ZoomReset"].includes(action)) {
    assert.equal(process.platform, "win32");
    assert.equal(process.env.GITHUB_ACTIONS, "true", "Native zoom requires GitHub hosted runner");
    assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted", "Native zoom requires GitHub hosted runner");
    assert.ok(owner.productWindow);
    assert.ok(!windowName && !auxiliaryWindow);
  }
  const args = [
    "-NoProfile",
    "-NonInteractive",
    "-File",
    path.resolve(".github/scripts/windows-installer-ui.ps1"),
    "-TargetProcessId",
    String(owner.identity.Pid),
    "-ExpectedExecutable",
    owner.identity.Path,
    "-ExpectedStartTimeUtc",
    owner.started,
    "-FixtureRoot",
    owner.fixtureRoot,
    "-Action",
    action,
  ];
  if (
    owner.productWindow &&
    ["Close", "Resize", "Minimize", "Activate", "Inspect", "ZoomIn", "ZoomReset"].includes(action)
  )
    args.push("-ProductWindow", owner.productWindow);
  if (auxiliaryWindow) args.push("-AuxiliaryWindow", auxiliaryWindow);
  if (controlId) args.push("-ControlId", controlId);
  if (controlName) args.push("-ControlName", controlName);
  if (filePath) args.push("-FilePath", filePath);
  if (windowName) args.push("-WindowName", windowName);
  if (width !== undefined) args.push("-Width", String(width));
  if (height !== undefined) args.push("-Height", String(height));
  const result = spawnSync("powershell.exe", args, { encoding: "utf8", timeout: 20000 });
  assert.equal(result.status, 0, `Owned ${action} UI Automation failed: ${(result.stderr ?? "").slice(0, 600)}`);
  return ["Inspect", "InspectFilePicker", "SaveFile", "ChooseFile"].includes(action)
    ? JSON.parse(result.stdout.replace(/^\uFEFF/, ""))
    : undefined;
}

export function ownedProductCohort(identity) {
  const processes = allWindowsProcesses();
  assert.ok(
    processes.some((p) => p.Pid === identity.Pid && p.Created === identity.Created && p.Path === identity.Path),
  );
  const owned = new Set([identity.Pid]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const process of processes)
      if (owned.has(process.ParentPid) && !owned.has(process.Pid)) {
        owned.add(process.Pid);
        changed = true;
      }
  }
  return processes.filter((p) => owned.has(p.Pid));
}
export async function measureWarmOwnedWindow(owner, cdp) {
  nativeWindowAction(owner, "Minimize");
  const before = performance.now();
  nativeWindowAction(owner, "Activate");
  await cdp.evaluate("new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))");
  captureWindowOwner(owner.identity, owner.fixtureRoot);
  return performance.now() - before;
}

// User zoom is native host input; CDP only observes layout and scale.
export async function withOwnedNativeZoom(owner, cdp, observe) {
  const before = await cdp.evaluate("({width:innerWidth,ratio:devicePixelRatio})");
  assert.ok(Number.isSafeInteger(before?.width) && before.width > 0, "Owned zoom viewport width unavailable");
  assert.ok(Number.isFinite(before?.ratio) && before.ratio > 0, "Owned zoom device scale unavailable");
  async function waitScale(expression, label) {
    const deadline = performance.now() + 15000;
    while (performance.now() < deadline) {
      if (await cdp.evaluate(expression)) return;
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    throw new Error(label);
  }
  // These disposable product sessions start at default browser zoom.
  let failure;
  try {
    nativeWindowAction(owner, "ZoomIn");
    await waitScale(
      `innerWidth<${before.width} && devicePixelRatio>${before.ratio}`,
      "Native browser zoom did not change renderer scale",
    );
    return await observe();
  } catch (error) {
    failure = error;
    throw error;
  } finally {
    try {
      nativeWindowAction(owner, "ZoomReset");
      await waitScale(
        `innerWidth===${before.width} && Math.abs(devicePixelRatio-${before.ratio})<0.001`,
        "Native browser zoom did not restore original scale",
      );
    } catch (error) {
      if (!failure) throw error;
      failure.zoomResetFailure = error;
    }
  }
}
