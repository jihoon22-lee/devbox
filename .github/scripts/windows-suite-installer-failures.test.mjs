import assert from "node:assert/strict";
import test from "node:test";
import {
  validateInstallerFaultOwnership,
  observeInstallerFailurePreservation,
} from "./windows-suite-installer-failures.mjs";
import {
  rejectedInstallerIssue,
  directorySpaceRejected,
  ownedNsisSpawnOptions,
} from "./windows-suite-installer-actions.mjs";
import { validateWithdrawnUpdateReceipt } from "./windows-suite-delivery-user-flows.mjs";
for (const guard of [() => validateInstallerFaultOwnership(undefined), () => observeInstallerFailurePreservation({})])
  test("installer faults reject unowned/non-Windows input before any mutation", async () => {
    await assert.rejects(guard(), /win32|true|undefined/);
  });
test("installer failure observation requires exact native issue token rather than generic Abort prose", () => {
  assert.equal(
    rejectedInstallerIssue({ controls: [{ name: "설치를 준비하지 못했습니다" }] }, ["suite_writers_must_close"]),
    null,
  );
  assert.equal(
    rejectedInstallerIssue({ controls: [{ name: "not_suite_space_insufficient_extra" }] }, [
      "suite_space_insufficient",
    ]),
    null,
  );
  assert.equal(
    rejectedInstallerIssue({ controls: [{ name: "suite_space_insufficient\n" }] }, ["suite_space_insufficient"]),
    "suite_space_insufficient",
  );
});
test("tiny volume proof also requires an actual disabled directory space gate", () => {
  const view = {
    buttons: [{ id: "1", visible: true, enabled: false }],
    controls: [
      { name: "필요한 디스크 공간: 150MB", visible: true },
      { name: "사용 가능한 디스크 공간: 60MB", visible: true },
    ],
  };
  assert.equal(directorySpaceRejected(view, { availableBytes: 60 * 1024 * 1024 }), true);
  assert.equal(directorySpaceRejected(view, { availableBytes: 1024 * 1024 * 1024 }), false);
  assert.equal(
    directorySpaceRejected(
      { ...view, buttons: [{ id: "1", visible: true, enabled: true }] },
      { availableBytes: 60 * 1024 * 1024 },
    ),
    false,
  );
  assert.equal(directorySpaceRejected({ ...view, controls: [] }, { availableBytes: 60 * 1024 * 1024 }), false);
});
test("only NSIS allowlisted final absolute raw arguments disable Windows quoting", () => {
  const image = "C:\\owned fixture\\setup.exe",
    args = ["/S", "/D=C:\\owned fixture\\Suite UI Fixture"];
  assert.deepEqual(ownedNsisSpawnOptions(image, args), { windowsVerbatimArguments: true, argv0: `"${image}"` });
  assert.equal(args.at(-1), "/D=C:\\owned fixture\\Suite UI Fixture");
  for (const invalid of [
    ["/D=relative"],
    ["/D=C:\\owned", "/S"],
    ["/S", '/D="C:\\owned"'],
    ["--arbitrary", "/D=C:\\owned"],
    ["/D=C:\\owned\nother"],
  ])
    assert.throws(() => ownedNsisSpawnOptions(image, invalid));
  assert.equal(ownedNsisSpawnOptions(image, ["_?=C:\\owned fixture"]).windowsVerbatimArguments, true);
});
test("withdrawn supplemental receipt cannot complete delivery with a failed or different-parent fixture", () => {
  const identity = {
    sourceSha: "a".repeat(40),
    fixtureSha: "a".repeat(40),
    artifactDigests: { asset: "b".repeat(64) },
  };
  assert.throws(() => validateWithdrawnUpdateReceipt({ status: "FAIL" }, identity, "c".repeat(64)));
  assert.throws(() =>
    validateWithdrawnUpdateReceipt({ status: "PASS", fixtureKind: "alternate-encoding" }, identity, "c".repeat(64)),
  );
});
