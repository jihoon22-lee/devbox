import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import test from "node:test";
import {
  validateInstallerFaultOwnership,
  observeInstallerFailurePreservation,
} from "./windows-suite-installer-failures.mjs";
import {
  rejectedInstallerIssue,
  directorySpaceRejected,
  ownedNsisSpawnOptions,
  installerFailureObservation,
  inspectInstallerFailure,
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

test("installer failure projects known status and bootstrap codes without paths or arbitrary text", () => {
  const result = installerFailureObservation(
    {
      windowCount: 1,
      selectedWindowCount: 1,
      controls: [
        { id: "1027", visible: true, enabled: true, name: "C:\\private\\token-secret" },
        { id: "", visible: true, name: "설치를 준비하지 못했습니다. existing private data" },
        { id: "", visible: true, name: "bootstrap_health_required\nC:\\private" },
        { id: "", visible: true, name: "bootstrap_secret_private" },
      ],
    },
    "installation finish",
    null,
  );
  assert.deepEqual(result.issues, ["bootstrap_health_required"]);
  assert.deepEqual(result.statuses, ["preparation_failed"]);
  assert.equal(result.stage, "installation finish");
  assert.equal(JSON.stringify(result).includes("private"), false);
  assert.equal(installerFailureObservation(null, "welcome", 1).inspectionUnavailable, true);
});

test("installer issue boundaries reject adjoining digits and underscores", () => {
  for (const name of ["1bootstrap_health_required", "bootstrap_health_required2", "bootstrap_health_required_extra"])
    assert.deepEqual(installerFailureObservation({ controls: [{ name }] }, "installation finish", null).issues, []);
});

test("failed owned installer expands exact Details once and exposes allowlisted issue", () => {
  let expanded = false;
  const actions = [];
  const installer = {
    child: { exitCode: null },
    inspect: () => ({
      buttons: [{ id: "1027", enabled: true, visible: true }],
      controls: [
        { name: "설치 항목과 바로가기를 등록하지 못했습니다" },
        ...(expanded ? [{ name: "suite_remove_file_changed\nprivate path" }] : []),
      ],
    }),
    invoke: (id) => {
      actions.push(id);
      expanded = true;
    },
  };
  const observation = inspectInstallerFailure(installer, "installation finish");
  assert.deepEqual(actions, ["1027"]);
  assert.deepEqual(observation.issues, ["suite_remove_file_changed"]);
  assert.equal(JSON.stringify(observation).includes("private path"), false);
});
test("Details diagnostic never invokes a healthy installer or replaces first observation on failure", () => {
  for (const failing of [false, true]) {
    const view = {
      buttons: [{ id: "1027", enabled: true, visible: true }],
      controls: [{ name: failing ? "설치 항목과 바로가기를 등록하지 못했습니다" : "Devbox 설치 준비 완료" }],
    };
    let invoked = 0;
    const result = inspectInstallerFailure(
      {
        child: { exitCode: null },
        inspect: () => view,
        invoke: () => {
          invoked++;
          throw new Error("diagnostic unavailable");
        },
      },
      "installation finish",
    );
    assert.equal(invoked, failing ? 1 : 0);
    assert.deepEqual(result.statuses, [failing ? "registration_failed" : "prepared"]);
  }
});

test("failed installer is cancelled normally and its exit is bounded", async () => {
  const { releaseFailedInstaller } = await import("./windows-suite-installer-actions.mjs");
  const calls = [];
  const child = { exitCode: null, unref: () => calls.push("unref") };
  const installer = {
    child,
    inspect: () => ({ buttons: [{ id: "2", enabled: true, visible: true }] }),
    invoke: (id) => {
      calls.push(id);
      child.exitCode = 1;
    },
  };
  const result = await releaseFailedInstaller(installer, async (check, _label, timeout) => {
    assert.equal(timeout, 10000);
    assert.equal(check(), true);
  });
  assert.deepEqual(calls, ["2"]);
  assert.equal(result, "cancelled");
});
test("failed installer stays preserved and detached if cancellation is unavailable", async () => {
  const { releaseFailedInstaller } = await import("./windows-suite-installer-actions.mjs");
  let detached = false;
  const child = {
    exitCode: null,
    unref: () => {
      detached = true;
    },
  };
  const result = await releaseFailedInstaller(
    { child, inspect: () => ({ buttons: [] }), invoke: () => assert.fail("unavailable Cancel must not invoke") },
    async () => {
      throw new Error("bounded timeout");
    },
  );
  assert.equal(result, "preserved");
  assert.equal(child.exitCode, null);
  assert.equal(detached, true);
});

test("preserved failed installer cannot keep its runner process alive", () => {
  const moduleUrl = new URL("./windows-suite-installer-actions.mjs", import.meta.url).href;
  const proof = execFileSync(
    process.execPath,
    [
      "--input-type=module",
      "-e",
      `
    import { spawn } from "node:child_process";
    import { once } from "node:events";
    import { releaseFailedInstaller } from ${JSON.stringify(moduleUrl)};
    const child = spawn(process.execPath, ["-e", "setTimeout(()=>{},1500)"], { stdio: "ignore" });
    await once(child, "spawn");
    const result = await releaseFailedInstaller({ child, inspect: () => null }, async () => { throw new Error("deadline"); });
    console.log(JSON.stringify({ result, alive: child.exitCode === null }));
  `,
    ],
    { encoding: "utf8", timeout: 1000 },
  );
  assert.deepEqual(JSON.parse(proof), { result: "preserved", alive: true });
});

test("setup and removal failure cleanup rethrows the identical primitive or frozen failure", async () => {
  const { rethrowInstallerFailure } = await import("./windows-suite-installer-actions.mjs");
  for (const original of [
    "original failure",
    null,
    Object.freeze(new Error("original failure")),
    new Error("original failure"),
  ]) {
    let detached = false;
    const installer = {
      child: {
        exitCode: null,
        unref: () => {
          detached = true;
        },
      },
      inspect: () => null,
    };
    try {
      await rethrowInstallerFailure(installer, original, async () => {
        throw new Error("cleanup deadline");
      });
      assert.fail("must reject");
    } catch (error) {
      assert.equal(error, original);
    }
    assert.equal(detached, true);
  }
});

test("delivery health releases the exited Knowledge policy before another Knowledge owner starts", async () => {
  const { completeHealthAfterKnowledgeClose } = await import("./windows-suite-delivery-user-flows.mjs");
  let policyOwned = true;
  const knowledge = {
    child: { exitCode: 0 },
    close: async () => {
      policyOwned = false;
    },
  };
  const result = await completeHealthAfterKnowledgeClose(knowledge, "health", async (label) => {
    assert.equal(label, "health");
    assert.equal(policyOwned, false, "Second Knowledge CDP owner must not collide with exited first owner");
    return ["screenshot"];
  });
  assert.deepEqual(result, ["screenshot"]);
  knowledge.child.exitCode = null;
  await assert.rejects(
    completeHealthAfterKnowledgeClose(knowledge, "health", () => assert.fail("live Knowledge must not be replaced")),
    /normal saved close/,
  );
});

import { activationStateObservation, bestEffortActivationObservation } from "./windows-suite-user-flow.mjs";
test("rejected setup cancellation confirms one owned nested abort prompt exactly once", async () => {
  const { cancelRejectedInstaller } = await import("./windows-suite-installer-actions.mjs");
  const calls = [];
  let confirmations = 0;
  const child = { exitCode: null };
  const installer = {
    child,
    invoke: (id) => {
      calls.push(id);
      if (id === "6") confirmations++;
    },
    inspect: () => ({ windows: [{}], controls: [{ id: "6", enabled: true, visible: true }] }),
  };
  await cancelRejectedInstaller(installer, async (check) => {
    assert.equal(check(), false);
    assert.equal(check(), false);
    child.exitCode = 1;
    assert.equal(check(), true);
  });
  assert.equal(confirmations, 1);
  assert.deepEqual(calls, ["2", "6"]);
});
test("rejected setup never confirms hidden, disabled or ambiguous abort controls", async () => {
  const { cancelRejectedInstaller } = await import("./windows-suite-installer-actions.mjs");
  for (const controls of [
    [{ id: "6", enabled: false, visible: true }],
    [{ id: "6", enabled: true, visible: false }],
    [
      { id: "6", enabled: true, visible: true },
      { id: "6", enabled: true, visible: true },
    ],
  ]) {
    const calls = [],
      child = { exitCode: null };
    await cancelRejectedInstaller(
      { child, invoke: (id) => calls.push(id), inspect: () => ({ controls }) },
      async (check) => {
        assert.equal(check(), false);
        child.exitCode = 1;
        assert.equal(check(), true);
      },
    );
    assert.deepEqual(calls, ["2"]);
  }
});
test("activation failure evidence exposes only bounded phase and revision", () => {
  assert.deepEqual(activationStateObservation({ phase: "health", revision: 3, secret: "private" }), {
    phase: "health",
    revision: 3,
  });
  assert.deepEqual(activationStateObservation({ phase: "private", revision: -1 }), { phase: null, revision: null });
});
test("activation evidence failure cannot replace original failure", async () => {
  const original = Object.freeze(new Error("original"));
  let preserved;
  try {
    throw original;
  } catch (error) {
    assert.deepEqual(
      await bestEffortActivationObservation(async () => {
        throw new Error("observer");
      }),
      { unavailable: true },
    );
    preserved = error;
  }
  assert.equal(preserved, original);
});

test("owned helper observation retains emitted checkpoint source codes without raw text", () => {
  const observation = installerFailureObservation(
    { controls: [{ name: "작업을 완료하지 못했습니다 (checkpoint_source_changed). private-path" }] },
    "activation-health-wait-marker",
    null,
  );
  assert.deepEqual(observation.issues, ["checkpoint_source_changed"]);
  assert.ok(!JSON.stringify(observation).includes("private-path"));
});

test("owned removal failure expands exact Details and preserves only fixed native codes", () => {
  let expanded = false;
  const installer = {
    child: { exitCode: null },
    inspect: () => ({
      controls: [
        { name: "제거를 완료하지 못했습니다" },
        ...(expanded ? [{ name: "bootstrap_owner_changed private-path" }] : []),
      ],
      buttons: [{ id: "1027", enabled: true, visible: true }],
    }),
    invoke: (id) => {
      assert.equal(id, "1027");
      expanded = true;
    },
  };
  const evidence = inspectInstallerFailure(installer, "removal execution");
  assert.deepEqual(evidence.statuses, ["removal_failed"]);
  assert.deepEqual(evidence.issues, ["bootstrap_owner_changed"]);
  assert.ok(!JSON.stringify(evidence).includes("private-path"));
});

import { acquireOwnedInstaller } from "./windows-suite-installer-actions.mjs";
test("failed installer acquisition releases only child reference and preserves first failure", async () => {
  const original = Object.freeze(new Error("identity unavailable"));
  let unrefs = 0;
  const child = {
    unref: () => {
      unrefs++;
    },
    kill: () => assert.fail("unverified child must not be killed"),
  };
  await assert.rejects(
    acquireOwnedInstaller(child, async () => {
      throw original;
    }),
    (error) => error === original,
  );
  assert.equal(unrefs, 1);
  await assert.rejects(
    acquireOwnedInstaller(
      {
        unref: () => {
          throw new Error("unref");
        },
      },
      async () => {
        throw original;
      },
    ),
    (error) => error === original,
  );
});
