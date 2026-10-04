import assert from "node:assert/strict";
import test from "node:test";
import { reviewedHelperCodes } from "./windows-reviewed-helper-evidence.mjs";

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
  assert.deepEqual(reviewedHelperCodes({ name: observation.name, controls: [{ name: "(token=secret)" }] }), []);
});
