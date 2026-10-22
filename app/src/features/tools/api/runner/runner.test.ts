// v0.9.0 runner.test.ts 이식 9·적응 2(07b A 표) + 회귀 표 CV-F04 4개 + 캡처 출처(3c 선행 결정).
import { describe, expect, it, vi } from "vitest";
import type { EnvVar, RequestSnapshot } from "../../../../rpc/gen/rpc";
import type { Assertion } from "../lib/assertions";
import type { Capture } from "../lib/captures";
import type { ApiResponse } from "../lib/template";
import { emptyDraft, type RequestDraft, row, toSnapshot } from "../model/draft";
import { variableUses } from "./plan";
import { type RunDeps, type RunEntry, runCollection } from "./runner";
import { Session } from "./session";

const RUN = "run-1";
const capture = (variable: string, target = `$.${variable}`): Capture => ({
  id: `c-${variable}`,
  enabled: true,
  variable,
  source: "jsonPath",
  target,
});
const status200: Assertion = {
  id: "s",
  enabled: true,
  source: "status",
  target: "",
  operator: "equals",
  expected: "200",
};

function entry(id: string, url: string, extra: Partial<RequestDraft> & Partial<RunEntry> = {}): RunEntry {
  const { reviewNeeded = false, produces, masked, ...draft } = extra;
  const full: RequestDraft = { ...emptyDraft(), url, ...draft };
  return {
    id,
    name: id,
    draft: full,
    reviewNeeded,
    produces: produces ?? full.captures.filter((c) => c.enabled).map((c) => c.variable),
    ...(masked ? { masked } : {}),
  };
}
const ok = (body: string, status = 200): ApiResponse => ({
  status,
  status_text: "",
  headers: [],
  duration_ms: 5,
  size_bytes: body.length,
  body,
  is_json: true,
  final_url: "",
  redirects: [],
  cookies: [],
  response_id: null,
  raw_headers_available: false,
  headers_truncated: false,
});
type Reply = (snapshot: RequestSnapshot, vars: EnvVar[]) => ApiResponse;
const deps = (reply: Reply) => ({
  runId: RUN,
  send: vi.fn<RunDeps["send"]>(async (snapshot, variables) => reply(snapshot, variables)),
  sleep: vi.fn<RunDeps["sleep"]>(async () => {}),
});
const value = (vars: EnvVar[], name: string) => vars.filter((v) => v.name === name).at(-1)?.value;
const run = (entries: RunEntry[], d: RunDeps, session = new Session(), stopOnFailure = false, env = []) =>
  runCollection(entries, env, session, { stopOnFailure, delayMs: 0 }, d, new AbortController().signal, () => {});
const sentTo = (d: ReturnType<typeof deps>, path: string) =>
  d.send.mock.calls.filter(([snapshot]) => snapshot.url.endsWith(path));

describe("collection runner (v0.9.0 이식)", () => {
  it("chains a captured token into the next request", async () => {
    const d = deps((s, vars) =>
      s.url.endsWith("/login") ? ok('{"token":"abc"}') : ok(JSON.stringify({ auth: value(vars, "token") })),
    );
    const login = entry("login", "https://x.test/login", { captures: [capture("token")] });
    const me = entry("me", "https://x.test/me", {
      headers: [row("Authorization", "Bearer {{token}}")],
      assertions: [{ ...status200, source: "jsonPath", target: "$.auth", expected: "abc" }],
    });
    const summary = await run([login, me], d, new Session(), true);
    expect(summary).toMatchObject({ passed: 2, failed: 0, runId: RUN });
    expect(d.send.mock.calls[1]![1]).toEqual([{ name: "token", value: "abc", sensitive: true, origin: "capture" }]);
    expect(summary.steps[1]!.sources).toEqual([{ name: "token", from: { runId: RUN, requestId: "login" } }]);
  });

  it("fails a request whose variable was not captured", async () => {
    const d = deps(() => ok("{}"));
    const login = entry("login", "https://x.test/login", { captures: [capture("token")] });
    const summary = await run([login, entry("me", "https://x.test/{{token}}")], d);
    expect(summary.steps[0]).toMatchObject({ status: "error" });
    expect(summary.steps[1]).toMatchObject({ status: "skipped", message: "출처 요청 실패: login" });
    expect(d.send).toHaveBeenCalledTimes(1);
    // 생산 요청이 없는 변수는 v0.9.0처럼 "변수 없음"이다.
    const lone = await run(
      [entry("me", "https://x.test/{{nobody}}")],
      deps(() => ok("{}")),
    );
    expect(lone.steps[0]).toMatchObject({ status: "error", message: "변수 없음: nobody" });
  });

  it("skips requests that need a secret review and continues without stop-on-failure", async () => {
    const d = deps((s) => (s.url.includes("bad") ? ok("{}", 500) : ok("{}")));
    const summary = await run(
      [
        entry("secret", "https://x.test/a", { reviewNeeded: true }),
        entry("bad", "https://x.test/bad", { assertions: [status200] }),
        entry("good", "https://x.test/good"),
      ],
      d,
    );
    expect(summary.steps.map((s) => s.status)).toEqual(["skipped", "failed", "passed"]);
    expect(summary.steps[0]!.message).toBe("비밀 검토가 필요합니다");
    expect(sentTo(d, "/a")).toHaveLength(0);
  });

  it("stops after the first failure when asked and marks the rest skipped", async () => {
    const d = deps(() => ok("{}", 500));
    const summary = await run(
      [entry("a", "https://x.test/a", { assertions: [status200] }), entry("b", "https://x.test/b")],
      d,
      new Session(),
      true,
    );
    expect(summary.steps.map((s) => s.status)).toEqual(["failed", "skipped"]);
    expect(summary.steps[1]!.message).toBe("앞선 실패로 건너뛰었습니다");
  });

  it("cancels the in-flight request and skips the rest", async () => {
    const controller = new AbortController();
    const d = {
      ...deps(() => ok("{}")),
      send: vi.fn<RunDeps["send"]>(
        (_snapshot, _vars, signal) =>
          new Promise<never>((_, reject) => signal.addEventListener("abort", () => reject(new Error("cancelled")))),
      ),
    };
    const running = runCollection(
      [entry("a", "https://x.test/a"), entry("b", "https://x.test/b")],
      [],
      new Session(),
      { stopOnFailure: false, delayMs: 0 },
      d,
      controller.signal,
      () => {},
    );
    controller.abort();
    const summary = await running;
    expect(summary.cancelled).toBe(true);
    expect(summary.steps.map((s) => s.status)).toEqual(["error", "skipped"]);
    expect(summary.steps.map((s) => s.message)).toEqual(["요청이 취소되었습니다", "실행을 중지했습니다"]);
  });

  it("a missing capture cannot reuse old session or environment values", async () => {
    const session = new Session();
    session.set("token", "previous", { runId: "run-0", requestId: "login" });
    const d = deps(() => ok("{}"));
    const summary = await runCollection(
      [
        entry("login", "https://x.test/login", { captures: [capture("token")] }),
        entry("next", "https://x.test/{{token}}"),
      ],
      [{ name: "token", value: "older" }],
      session,
      { stopOnFailure: false, delayMs: 0 },
      d,
      new AbortController().signal,
      () => {},
    );
    expect(summary.steps[1]!.message).toBe("출처 요청 실패: login");
    expect(d.send).toHaveBeenCalledTimes(1);
    expect(JSON.stringify(d.send.mock.calls)).not.toMatch(/previous|older/);
    // v1: 실패한 생산 요청의 세션 값은 지우지 않는다(이번 실행에서만 사용 불가).
    expect(session.get("token")).toEqual({ value: "previous", from: { runId: "run-0", requestId: "login" } });
  });

  it("enforces request and delay bounds before sending", async () => {
    const d = deps(() => ok("{}"));
    for (const [entries, delayMs] of [
      [Array.from({ length: 201 }, (_, i) => entry(String(i), "https://x.test")), 0],
      [[], 5001],
      [[], -1],
      [[], Number.NaN],
    ] as const) {
      await expect(
        runCollection(
          [...entries],
          [],
          new Session(),
          { stopOnFailure: false, delayMs },
          d,
          new AbortController().signal,
          () => {},
        ),
      ).rejects.toThrow("러너 한도");
    }
    expect(d.send).not.toHaveBeenCalled();
  });

  it("ignores hidden body and authentication drafts until selected", async () => {
    const draft: RequestDraft = {
      ...emptyDraft(),
      url: "https://x.test",
      bodies: { json: { text: "{{body}}" } },
      auths: {
        basic: { username: "{{user}}", password: "{{password}}" },
        bearer: { token: "{{token}}" },
        apiKey: { name: "{{key}}", value: "{{value}}", placement: "header" },
      },
    };
    expect(variableUses(toSnapshot(draft))).toEqual([]);
    expect(variableUses(toSnapshot({ ...draft, bodyKind: "json" }))).toEqual(["body"]);
    for (const [authKind, expected] of [
      ["basic", ["user", "password"]],
      ["bearer", ["token"]],
      ["apiKey", ["key", "value"]],
    ] as const) {
      expect(variableUses(toSnapshot({ ...draft, authKind }))).toEqual(expected);
    }
    const d = deps(() => ok("{}"));
    const summary = await run([{ ...entry("hidden", "https://x.test"), draft }], d);
    expect(summary.steps[0]!.status).toBe("passed");
    expect(d.send.mock.calls[0]![0].body).toEqual({ kind: "none" });
  });

  it("cancels during regex assertions before captures or a subsequent request", async () => {
    const terminate = vi.fn();
    let created!: () => void;
    const started = new Promise<void>((resolve) => {
      created = resolve;
    });
    vi.stubGlobal(
      "Worker",
      class {
        terminate = terminate;
        postMessage = vi.fn();
        constructor() {
          created();
        }
      },
    );
    try {
      const controller = new AbortController();
      const d = deps(() => ok('{"token":"new"}'));
      const session = new Session();
      const first = entry("first", "https://x.test/first", {
        assertions: [{ ...status200, id: "regex", source: "body", operator: "matches", expected: "token" }],
        captures: [capture("token")],
      });
      const task = runCollection(
        [first, entry("next", "https://x.test/next")],
        [],
        session,
        { stopOnFailure: false, delayMs: 0 },
        d,
        controller.signal,
        () => {},
      );
      await started;
      controller.abort();
      const result = await task;
      expect(result.cancelled).toBe(true);
      expect(result.steps.map((step) => step.status)).toEqual(["error", "skipped"]);
      expect(d.send).toHaveBeenCalledOnce();
      expect(session.entries()).toEqual([]);
      expect(terminate).toHaveBeenCalledOnce();
    } finally {
      vi.unstubAllGlobals();
    }
  });
});

describe("collection runner (v0.9.0 적응)", () => {
  it("never sends retained multipart file placeholders and keeps failed captures out of the summary", async () => {
    const session = new Session();
    session.set("token", "old", { runId: "run-0", requestId: "login" });
    const d = deps(() => ok('{"token":"[REDACTED]","other":"new-secret"}'));
    const file = entry("file", "https://x.test/upload", {
      bodyKind: "multipart",
      bodies: {
        multipart: {
          parts: [
            {
              id: "p",
              enabled: true,
              kind: "file",
              name: "upload",
              value: "",
              path: "",
              fileName: "x.txt",
              contentType: "",
            },
          ],
        },
      },
    });
    const binary = entry("binary", "https://x.test/blob", { bodyKind: "binary", bodies: { binary: { path: "" } } });
    const login = entry("login", "https://x.test/login", { captures: [capture("token")] });
    const summary = await run([file, binary, login], d, session);
    expect(summary.steps.map((step) => step.status)).toEqual(["skipped", "skipped", "error"]);
    expect(summary.steps[0]!.message).toBe("파일을 다시 선택해야 합니다");
    expect(d.send).toHaveBeenCalledTimes(1);
    // 데몬이 가린 `[REDACTED]`는 캡처 값이 아니다. 세션의 이전 값은 그대로 둔다.
    expect(session.get("token")?.value).toBe("old");
    expect(JSON.stringify(summary)).not.toContain("new-secret");
  });

  it("discarding captures keeps tombstones instead of reviving older environment values", async () => {
    const session = new Session();
    session.set("token", "new", { runId: "run-0", requestId: "login" });
    session.discard(["token"]);
    expect(session.isDiscarded("token")).toBe(true);
    const d = deps(() => ok("{}"));
    const summary = await runCollection(
      [entry("use", "https://x.test/{{token}}")],
      [{ name: "token", value: "old" }],
      session,
      { stopOnFailure: false, delayMs: 0 },
      d,
      new AbortController().signal,
      () => {},
    );
    expect(summary.steps[0]).toMatchObject({ status: "error", message: "변수 없음: token" });
    expect(d.send).not.toHaveBeenCalled();
    session.set("token", "again", { runId: "run-0", requestId: "login" });
    expect(session.isDiscarded("token")).toBe(false);
  });
});

describe("capture provenance (CV-F04·AR-F03)", () => {
  const login = (extra: Partial<RequestDraft> & Partial<RunEntry> = {}) =>
    entry("login", "https://x.test/login", { captures: [capture("token")], ...extra });
  const use = () => entry("use", "https://x.test/use", { headers: [row("Authorization", "Bearer {{token}}")] });
  const oldSession = () => {
    const session = new Session();
    session.set("token", "old", { runId: "run-0", requestId: "login" });
    return session;
  };

  it("skipped_or_cancelled_producer_blocks_the_consumer", async () => {
    const missingFile = {
      bodyKind: "multipart" as const,
      bodies: {
        multipart: {
          parts: [
            {
              id: "p",
              enabled: true,
              kind: "file" as const,
              name: "f",
              value: "",
              path: "",
              fileName: "",
              contentType: "",
            },
          ],
        },
      },
    };
    for (const stopOnFailure of [true, false]) {
      for (const [producer, reason] of [
        [login({ reviewNeeded: true }), "비밀 검토가 필요합니다"],
        [login(missingFile), "파일을 다시 선택해야 합니다"],
      ] as const) {
        const session = oldSession();
        const d = deps(() => ok('{"token":"new"}'));
        const summary = await run([producer, use()], d, session, stopOnFailure);
        expect(summary.steps[0]).toMatchObject({ status: "skipped", message: reason });
        expect(summary.steps[1]).toMatchObject({ status: "skipped", message: "출처 요청 실패: login" });
        expect(sentTo(d, "/use")).toHaveLength(0);
        expect(session.get("token")?.value).toBe("old");
      }
      // ③ 송신 중 취소
      const controller = new AbortController();
      const session = oldSession();
      const d = {
        ...deps(() => ok("{}")),
        send: vi.fn<RunDeps["send"]>(
          (_s, _v, signal) =>
            new Promise<never>((_, reject) => signal.addEventListener("abort", () => reject(new Error("cancelled")))),
        ),
      };
      const task = runCollection(
        [login(), use()],
        [],
        session,
        { stopOnFailure, delayMs: 0 },
        d,
        controller.signal,
        () => {},
      );
      await vi.waitFor(() => expect(d.send).toHaveBeenCalledOnce());
      controller.abort();
      const summary = await task;
      expect(summary.steps[1]).toMatchObject({ status: "skipped", message: "출처 요청 실패: login" });
      expect(d.send).toHaveBeenCalledOnce();
      expect(session.get("token")?.value).toBe("old");
    }
  });

  it("captured_value_from_failed_request_is_not_used", async () => {
    const session = oldSession();
    const d = deps((s) => (s.url.endsWith("/login") ? ok('{"error":"boom"}', 500) : ok("{}")));
    const summary = await run([login({ assertions: [status200] }), use()], d, session);
    expect(summary.steps[0]!.status).toBe("failed");
    expect(summary.steps[1]).toMatchObject({ status: "skipped", message: "출처 요청 실패: login" });
    expect(sentTo(d, "/use")).toHaveLength(0);
    // 응답에 캡처 값이 있어도 assertion이 실패한 요청의 값은 쓰지 않는다.
    const leaked = deps((s) => (s.url.endsWith("/login") ? ok('{"token":"from-500"}', 500) : ok("{}")));
    const again = await run([login({ assertions: [status200] }), use()], leaked, oldSession());
    expect(again.steps[1]!.status).toBe("skipped");
    expect(JSON.stringify(leaked.send.mock.calls)).not.toContain("from-500");
  });

  it("token_refresh_can_read_the_previous_run_value", async () => {
    const session = oldSession();
    const d = deps(() => ok('{"token":"fresh"}'));
    const refresh = entry("refresh", "https://x.test/refresh", {
      headers: [row("Authorization", "Bearer {{token}}")],
      captures: [capture("token")],
    });
    const summary = await run([refresh], d, session);
    expect(summary.steps[0]!.status).toBe("passed");
    expect(d.send).toHaveBeenCalledOnce();
    expect(d.send.mock.calls[0]![1]).toEqual([{ name: "token", value: "old", sensitive: true, origin: "capture" }]);
    expect(summary.steps[0]!.sources).toEqual([{ name: "token", from: { runId: "run-0", requestId: "login" } }]);
    expect(session.get("token")).toEqual({ value: "fresh", from: { runId: RUN, requestId: "refresh" } });
  });

  it("other_producers_and_environment_values_are_kept", async () => {
    const d = deps((s) => {
      if (s.url.endsWith("/a")) return ok('{"x":"from-a"}');
      if (s.url.endsWith("/login")) return ok("{}", 500);
      return ok("{}");
    });
    const entries = [
      entry("a", "{{base}}/a", { captures: [capture("x")] }),
      entry("login", "{{base}}/login", { captures: [capture("token")], assertions: [status200] }),
      entry("use", "{{base}}/use", { headers: [row("X", "{{x}}"), row("Authorization", "Bearer {{token}}")] }),
      entry("only-x", "{{base}}/only-x", { headers: [row("X", "{{x}}")] }),
    ];
    const summary = await runCollection(
      entries,
      [{ name: "base", value: "https://x.test" }],
      new Session(),
      { stopOnFailure: false, delayMs: 0 },
      d,
      new AbortController().signal,
      () => {},
    );
    expect(summary.steps.map((s) => s.status)).toEqual(["passed", "failed", "skipped", "passed"]);
    expect(summary.steps[2]!.message).toBe("출처 요청 실패: login");
    const [, vars] = sentTo(d, "/only-x")[0]!;
    expect(vars).toEqual([
      { name: "base", value: "https://x.test", sensitive: false, origin: "environment" },
      { name: "x", value: "from-a", sensitive: true, origin: "capture" },
    ]);
    expect(summary.steps[3]!.sources).toEqual([
      { name: "base", from: "environment" },
      { name: "x", from: { runId: RUN, requestId: "a" } },
    ]);
  });

  it("captured_secret_reference_is_sent_as_a_capture_not_resolved", async () => {
    // 악성 서버가 캡처 값에 비밀 참조를 넣는다. 화면은 출처를 capture로 보내 데몬이 글자 그대로 쓰게 한다
    // (데몬 쪽은 crates/api/tests/http_capture.rs가 fixture 서버로 확인).
    const d = deps((s) => (s.url.endsWith("/login") ? ok('{"token":"{{secret:GH}}"}') : ok("{}")));
    const summary = await runCollection(
      [login(), use()],
      [{ name: "gh", value: "{{secret:GH}}" }],
      new Session(),
      { stopOnFailure: true, delayMs: 0 },
      d,
      new AbortController().signal,
      () => {},
    );
    expect(summary.passed).toBe(2);
    expect(sentTo(d, "/use")[0]![1]).toEqual([
      { name: "token", value: "{{secret:GH}}", sensitive: true, origin: "capture" },
    ]);
    // 사용자가 쓴 환경 값의 참조는 environment 출처로 간다.
    const own = deps(() => ok("{}"));
    await runCollection(
      [entry("own", "https://x.test/own", { headers: [row("Authorization", "Bearer {{gh}}")] })],
      [{ name: "gh", value: "{{secret:GH}}" }],
      new Session(),
      { stopOnFailure: false, delayMs: 0 },
      own,
      new AbortController().signal,
      () => {},
    );
    expect(own.send.mock.calls[0]![1]).toEqual([
      { name: "gh", value: "{{secret:GH}}", sensitive: true, origin: "environment" },
    ]);
  });

  it("masked_values_from_the_file_block_the_run_step", async () => {
    const d = deps(() => ok("{}"));
    const masked = entry("masked", "https://x.test/m", {
      headers: [row("Authorization", "[REDACTED]")],
      masked: ["request.headers.0.value"],
    });
    const typed = entry("typed", "https://x.test/t", { headers: [row("X-Note", "[REDACTED]")] });
    const summary = await run([masked, typed], d);
    expect(summary.steps[0]).toMatchObject({ status: "skipped", message: "가린 값을 다시 입력해 주세요" });
    expect(summary.steps[1]!.status).toBe("passed");
    expect(d.send).toHaveBeenCalledOnce();
  });
});
