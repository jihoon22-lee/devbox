// Faults target only an existing owned receipt or a newly created disposable 64 MiB VHD.
import assert from "node:assert/strict";
import path from "node:path";
import { readFile, writeFile, realpath, lstat, mkdtemp, access } from "node:fs/promises";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { installedFixtureIdentity, fileDigest } from "./suite-user-flow-results.mjs";
import { rejectVisibleSetup } from "./windows-suite-installer-actions.mjs";
import { observeUntil } from "./windows-suite-ui-context.mjs";
const helper = path.resolve(".github/scripts/windows-owned-installer-fault.ps1");
async function exists(file) {
  try {
    await access(file);
    return true;
  } catch {
    return false;
  }
}
function faultAction(action, scratch, args = []) {
  const result = spawnSync(
    "powershell.exe",
    ["-NoProfile", "-NonInteractive", "-File", helper, "-Action", action, "-FixtureRoot", scratch, ...args],
    { encoding: "utf8", timeout: 30000 },
  );
  assert.equal(result.status, 0, `Owned installer ${action} failed: ${(result.stderr ?? "").slice(0, 300)}`);
  return result.stdout.replace(/^\uFEFF/u, "").trim();
}
async function screenshot(installer, scratch, name) {
  const image = path.resolve("product-foundation-evidence/user-flows/screenshots/installer-failures", `${name}.png`);
  faultAction("Capture", scratch, [
    "-TargetProcessId",
    String(installer.owner.identity.Pid),
    "-ExpectedExecutable",
    installer.owner.identity.Path,
    "-ExpectedStartTimeUtc",
    installer.owner.started,
    "-FilePath",
    image,
  ]);
  await fileDigest(image);
  return image;
}
export async function validateInstallerFaultOwnership(root) {
  assert.equal(process.platform, "win32");
  assert.equal(process.env.GITHUB_ACTIONS, "true");
  assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  const actual = await realpath(process.env.DEVBOX_USER_FLOW_INSTALL_ROOT);
  assert.equal(await realpath(root), actual);
  const key = await installedFixtureIdentity();
  const registration = JSON.parse(await readFile(path.join(actual, "suite-registration.json"), "utf8"));
  assert.equal(registration.installationKey, key);
  return { root: actual, key, scratch: path.dirname(actual) };
}
export async function observeInstallerFailurePreservation({ setup, root, noteFile, noteSha256, marker, markerSha256 }) {
  const owner = await validateInstallerFaultOwnership(root),
    screenshots = [],
    assertions = [];
  const preserved = async () => {
    assert.equal(await fileDigest(noteFile), noteSha256);
    assert.equal(await fileDigest(marker), markerSha256);
  };
  const receipt = path.join(owner.root, "uninstall-complete.json"),
    info = await lstat(receipt);
  assert.ok(
    info.isFile() && !info.isSymbolicLink(),
    "Existing owned uninstall receipt required for write-failure probe",
  );
  const receiptHash = await fileDigest(receipt),
    ready = path.join(owner.scratch, "write-fault-ready"),
    release = path.join(owner.scratch, "write-fault-release");
  assert.equal(await exists(ready), false);
  assert.equal(await exists(release), false);
  const child = spawn(
    "powershell.exe",
    [
      "-NoProfile",
      "-NonInteractive",
      "-File",
      helper,
      "-Action",
      "Lock",
      "-FixtureRoot",
      owner.scratch,
      "-FilePath",
      receipt,
      "-ReadyPath",
      ready,
      "-ReleasePath",
      release,
    ],
    { stdio: "ignore" },
  );
  await once(child, "spawn");
  try {
    await observeUntil(async () => await exists(ready), "owned sharing-deny receipt handle ready");
    const failure = await rejectVisibleSetup(setup, owner.root, {
      expectedIssues: ["reinstall_archive_unavailable"],
      onRejected: async (installer) =>
        screenshots.push(await screenshot(installer, owner.scratch, "DELIVERY-02-write-failure")),
    });
    assert.equal(failure.issue, "reinstall_archive_unavailable");
    assert.equal(child.exitCode, null, "Actual write-failure lease must still hold the owned receipt");
    await preserved();
    assert.equal(await fileDigest(receipt), receiptHash);
    assertions.push(
      "Actual visible same-directory reinstall displayed native write/archive failure while an owned read handle denied receipt deletion; saved document, unlisted file and uninstall receipt stayed byte-identical",
    );
  } finally {
    await writeFile(release, "release", { flag: "wx" });
    if (child.exitCode === null) await observeUntil(() => child.exitCode !== null, "owned receipt handle released");
    assert.equal(child.exitCode, 0, "Owned lock helper must exit cleanly");
  }
  const volumeScratch = await mkdtemp(path.join(owner.scratch, "space-fault-")),
    metadata = path.join(volumeScratch, "volume-owner.json");
  let volume;
  try {
    volume = JSON.parse(faultAction("CreateVolume", volumeScratch, ["-MetadataPath", metadata]));
    assert.ok(volume.available < 128 * 1024 * 1024 && volume.size <= 64 * 1024 * 1024);
    const volumeMarker = path.join(volume.root, "owned-preserved-user-file.txt");
    await writeFile(volumeMarker, "synthetic disposable-volume user data\n", { flag: "wx" });
    const hash = await fileDigest(volumeMarker);
    const target = path.join(volume.root, "Suite UI Fixture");
    const failure = await rejectVisibleSetup(setup, target, {
      expectedIssues: ["suite_space_insufficient"],
      spaceProof: { availableBytes: volume.available },
      onRejected: async (installer) =>
        screenshots.push(await screenshot(installer, owner.scratch, "DELIVERY-02-space-failure")),
    });
    assert.ok(["suite_space_insufficient", "nsis_directory_space_insufficient"].includes(failure.issue));
    assert.equal(await fileDigest(volumeMarker), hash);
    await preserved();
    assertions.push(
      "Actual exact candidate setup on a newly owned 64 MiB VHD displayed its directory space gate or native insufficient-space failure; its synthetic user file and the existing installation's saved data were unchanged",
    );
  } finally {
    if (await exists(metadata)) faultAction("CleanupVolume", volumeScratch, ["-MetadataPath", metadata]);
  }
  return {
    assertions,
    screenshotPaths: screenshots,
    proofScope: "hosted-owned-installer-faults",
    writeFailure: true,
    spaceFailure: true,
  };
}
