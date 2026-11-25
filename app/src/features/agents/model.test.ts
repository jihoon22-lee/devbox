import {
  attentionCount,
  branchNameProblem,
  failureLabel,
  fillTemplate,
  groupByUrgency,
  hookMissing,
  nextWaiting,
  previewBranch,
  titleSlug,
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

test("counts attention and detects missing hooks of every hooked profile after 90 seconds", () => {
  expect(
    attentionCount([task("w", "waiting", 1), task("i", "idle", 1), task("f", "failed", 1), task("r", "running", 1)]),
  ).toBe(3);
  const missing = { ...task("r", "running", 0), sessionId: null, prompt: "fix it" };
  expect(hookMissing([missing], profiles, 90_000)).toBe(false);
  expect(hookMissing([missing], profiles, 90_001)).toBe(true);
  const hooked = [
    ...profiles,
    {
      id: "opencode",
      label: "opencode",
      command: "opencode",
      hooks: "opencode" as const,
      waitingPatterns: [],
      promptArg: null,
    },
    { id: "plain", label: "Plain", command: "plain", hooks: "none" as const, waitingPatterns: [], promptArg: null },
    {
      id: "antigravity",
      label: "Antigravity",
      command: "agy",
      hooks: "antigravity" as const,
      waitingPatterns: [],
      promptArg: null,
    },
  ];
  for (const profile of ["codex", "opencode"]) {
    expect(hookMissing([{ ...missing, profile }], hooked, 91_000)).toBe(true);
  }
  // A profile without hooks never reports a session, and a reported session is enough.
  expect(hookMissing([{ ...missing, profile: "plain" }], hooked, 91_000)).toBe(false);
  expect(hookMissing([{ ...missing, profile: "opencode", sessionId: "ses_1" }], hooked, 91_000)).toBe(false);
  // Antigravity gets no prompt argument: its hooks report a session only with the first instruction
  // the user sends, whenever that is, so a launch without one is not a missing connection.
  const agy = { ...missing, profile: "antigravity" };
  expect(hookMissing([agy], hooked, 91_000)).toBe(false);
  expect(hookMissing([agy], hooked, 24 * 3600_000)).toBe(false);
  // devbox sends its first instruction once agy shows its input box, however long the folder-trust
  // question took. While it is queued, or was never sent, there is no report to miss.
  for (const delivery of ["queued", "pending", "unknown", "notSent"] as const) {
    const held = { ...agy, firstInstruction: { delivery, atMs: 1_000 } };
    expect(hookMissing([held], hooked, 24 * 3600_000)).toBe(false);
  }
  // From its delivery, it has the same 90 seconds as the others.
  const delivered = { ...agy, firstInstruction: { delivery: "delivered" as const, atMs: 500_000 } };
  expect(hookMissing([delivered], hooked, 590_000)).toBe(false);
  expect(hookMissing([delivered], hooked, 590_001)).toBe(true);
  expect(hookMissing([{ ...delivered, sessionId: "conversation" }], hooked, 590_001)).toBe(false);
  // Without one of its own (started without a prompt, or resumed), from the first instruction sent to it.
  const instructed = { ...agy, followUps: [{ atMs: 500_000, text: "fix it" }] };
  expect(hookMissing([instructed], hooked, 590_000)).toBe(false);
  expect(hookMissing([instructed], hooked, 590_001)).toBe(true);
  expect(hookMissing([{ ...instructed, sessionId: "conversation" }], hooked, 590_001)).toBe(false);
  // Only Claude reports a session before a prompt: the others wait for the first one.
  for (const prompt of ["", " \n"]) {
    for (const profile of ["codex", "opencode"]) {
      expect(hookMissing([{ ...missing, profile, prompt }], hooked, 91_000)).toBe(false);
    }
    expect(hookMissing([{ ...missing, prompt }], hooked, 91_000)).toBe(true);
  }
});

test("a pi launch that has not said it is ready after 90 seconds is missing its extension", () => {
  const hooked = [
    ...profiles,
    { id: "pi", label: "pi", command: "pi", hooks: "pi" as const, waitingPatterns: [], promptArg: null },
  ];
  // pi has its session from the start, and says it is ready once per launch, prompt or not.
  const starting = { ...task("p", "running", 0), profile: "pi", prompt: "", startingSinceMs: 0 };
  expect(hookMissing([starting], hooked, 90_000)).toBe(false);
  expect(hookMissing([starting], hooked, 90_001)).toBe(true);
  expect(hookMissing([{ ...starting, prompt: "fix it" }], hooked, 90_001)).toBe(true);
  // It said so.
  expect(hookMissing([{ ...starting, startingSinceMs: null }], hooked, 91_000)).toBe(false);
  // pi asks about trust before it is ready: that is a question, not a missing extension.
  expect(hookMissing([{ ...starting, state: "waiting" }], hooked, 91_000)).toBe(false);
  // A resumed task is timed from its launch, not from when it was created.
  const resumed = { ...starting, createdMs: 0, startingSinceMs: 500_000 };
  expect(hookMissing([resumed], hooked, 590_000)).toBe(false);
  expect(hookMissing([resumed], hooked, 590_001)).toBe(true);
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

test("the title slug and the branch name rule are the daemon's plan::slug and checked_slug", () => {
  expect(titleSlug("Fix login bug!")).toBe("fix-login-bug");
  expect(titleSlug("  --Fix__login  ")).toBe("fix-login");
  expect(titleSlug("한글만")).toBe("");
  expect(titleSlug("한글 and 영어")).toBe("and");
  expect(titleSlug("x".repeat(100))).toHaveLength(40);
  // A cut that lands on a hyphen drops it.
  expect(titleSlug(`${"x".repeat(39)} y`)).toBe("x".repeat(39));
  for (const ok of ["fix", "fix-login", "v2", "0", "a-b-c", "x".repeat(40)]) expect(branchNameProblem(ok)).toBeNull();
  for (const bad of ["", "Fix", "fix login", "fix--login", "-fix", "fix-", "fix_login", "fix/login", "agent/fix"])
    expect(branchNameProblem(bad), bad).toContain("영문 소문자");
  for (const bad of ["fix.lock", "../escape", "가나다", "fix\n"]) expect(branchNameProblem(bad), bad).not.toBeNull();
  expect(branchNameProblem("x".repeat(41))).toContain("40자");
  // The hyphen rule is said in full: not at either end, and not twice in a row.
  for (const bad of ["-fix", "fix-", "fix--login"])
    expect(branchNameProblem(bad), bad).toContain("하이픈은 처음이나 끝에 쓸 수 없고 연달아 쓸 수도 없습니다.");
  // What the dialog fills in from a title always passes the rule the daemon applies again.
  for (const title of ["Fix login", "A  B", "a-b", "--x--", "αβγ x", `${"x".repeat(60)} y`, "Ünïcode Fix", "1"])
    expect(branchNameProblem(titleSlug(title)), title).toBeNull();
});

test("failure reasons display session loss, external kill, exit and setup", () => {
  expect(failureLabel("session_lost")).toContain("WSL");
  expect(failureLabel("killed:9")).toContain("외부");
  expect(failureLabel("exit:2")).toContain("2");
  expect(failureLabel("setup:1")).toContain("준비");
  // The tool opened another conversation than the task's: what happened, and what to do.
  expect(failureLabel("session_mismatch")).toBe(
    "도구가 이 작업의 대화가 아닌 다른 대화를 열었습니다. 이 작업은 처음 대화만 따르므로, 이어 가려면 [같은 지시로 새 작업]으로 다시 시작하세요.",
  );
  expect(failureLabel("resume_failed")).toBe("에이전트 작업이 실패했습니다");
  expect(failureLabel(null)).toBe("에이전트 작업이 실패했습니다");
});
