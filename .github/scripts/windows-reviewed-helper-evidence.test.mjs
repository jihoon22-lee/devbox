import assert from "node:assert/strict";
import test from "node:test";
import { nativeIssueCollector, reviewedHelperCodes } from "./windows-reviewed-helper-evidence.mjs";

test("owned native stderr keeps bounded complete issue codes across chunks only", () => {
  const collector = nativeIssueCollector();
  collector.write("bootstrap_owner_");
  collector.write("changed\r\nsecret token=hidden\nsuite_writers_must_close\n");
  collector.write("x".repeat(10000) + "update_plan_changed\n");
  collector.write("bootstrap_owner_changed\n");
  assert.deepEqual(collector.codes, ["bootstrap_owner_changed", "suite_writers_must_close"]);
  for (let i = 0; i < 30; i++) collector.write(`update_issue_${i}\n`);
  assert.equal(collector.codes.length, 16);
});

test("only fixed native recovery issue codes enter evidence", () => {
  const observation = {
    name: "Devbox 데이터 복구",
    controls: [
      { name: "작업을 완료하지 못했습니다 (suite_writers_must_close).\nprivate fixture path and text" },
      { name: "작업을 완료하지 못했습니다 (update_agent_busy)." },
      { name: "token=secret unrelated_error" },
    ],
  };
  assert.deepEqual(reviewedHelperCodes(observation), ["suite_writers_must_close", "update_agent_busy"]);
  assert.deepEqual(reviewedHelperCodes({ ...observation, name: "Foreign dialog" }), []);
  assert.deepEqual(
    reviewedHelperCodes({
      name: "Devbox 작업 시작 실패",
      controls: [{ name: "검토한 작업을 시작하지 못했습니다 (bootstrap_helper_busy)." }],
    }),
    ["bootstrap_helper_busy"],
  );
  assert.deepEqual(reviewedHelperCodes({ name: observation.name, controls: [{ name: "(token=secret)" }] }), []);
});
