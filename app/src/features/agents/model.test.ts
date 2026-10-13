import {
  attentionCount,
  failureLabel,
  fillTemplate,
  groupByUrgency,
  hookMissing,
  nextWaiting,
  previewBranch,
} from "./model";
import { profiles, task } from "./test-fixtures";

test("urgency sorts waiting oldest first, others newest and excludes closed after 24 hours", () => {
  const now = 100 * 3600_000;
  const groups = groupByUrgency(
    [
      task("r", "running", 5),
      task("w", "waiting", 9),
      task("i", "idle", 3),
      task("f", "failed", 4),
      task("c", "closed", now - 1000),
      task("old", "closed", now - 24 * 3600_000),
      task("p", "preparing", 1),
      task("v", "review", 2),
      task("r2", "running", 6),
    ],
    now,
  );
  expect(groups.map((g) => [g.key, g.items.map((t) => t.id)])).toEqual([
    ["attention", ["i", "w"]],
    ["failed", ["f"]],
    ["review", ["v"]],
    ["running", ["r2", "r"]],
    ["preparing", ["p"]],
    ["closed", ["c"]],
  ]);
  expect(nextWaiting(groups)).toBe("i");
  expect(nextWaiting(groups, "i")).toBe("w");
  expect(nextWaiting(groups, "w")).toBe("i");
  expect(nextWaiting(groupByUrgency([task("r", "running", 1)], now))).toBeUndefined();
});

test("counts attention and detects only Claude missing hooks after 90 seconds", () => {
  expect(
    attentionCount([task("w", "waiting", 1), task("i", "idle", 1), task("f", "failed", 1), task("r", "running", 1)]),
  ).toBe(3);
  const missing = { ...task("r", "running", 0), sessionId: null };
  expect(hookMissing([missing], profiles, 90_000)).toBe(false);
  expect(hookMissing([missing], profiles, 90_001)).toBe(true);
  expect(hookMissing([{ ...missing, profile: "codex" }], profiles, 91_000)).toBe(false);
});

test("template only substitutes known variables and branch preview follows backend ASCII slug", () => {
  expect(
    fillTemplate("{{title}} {{branch}} {{project}} {{other}}", {
      title: "로그인",
      branch: "agent/login",
      project: "app",
    }),
  ).toBe("로그인 agent/login app {{other}}");
  expect(previewBranch("Fix login bug!", 0)).toBe("agent/fix-login-bug");
  expect(previewBranch("한글만", 1700000000000)).toBe("agent/task-1700000000");
  expect(previewBranch("x".repeat(100), 0)).toHaveLength(46);
});

test("failure reasons display session loss, external kill, exit and setup", () => {
  expect(failureLabel("session_lost")).toContain("WSL");
  expect(failureLabel("killed:9")).toContain("외부");
  expect(failureLabel("exit:2")).toContain("2");
  expect(failureLabel("setup:1")).toContain("준비");
});
