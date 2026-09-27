import { describe, expect, it, vi } from "vitest";
import { runCollection, SessionVariables } from "./runner";
import { emptyRequest } from "./importers";

const entry = (id: string, url: string, extra: Record<string, unknown> = {}) => ({
  id,
  name: id,
  folder: "",
  saved_at: 1,
  requiresSecretReview: false,
  request: { ...emptyRequest(), url, requiresSecretReview: false },
  assertions: [],
  captures: [],
  ...extra,
});
const ok = (body: string, status = 200) => ({
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
const deps = (send: (url: string, vars: { key: string; value: string }[]) => ReturnType<typeof ok>) => ({
  send: vi.fn(async (request: { url: string }, variables: { key: string; value: string }[]) =>
    send(request.url, variables),
  ),
  seal: vi.fn(async (value: string) => `sealed:${value}`),
  sleep: vi.fn(async () => {}),
});

describe("collection runner", () => {
  it("chains a captured token into the next request", async () => {
    const d = deps((url, vars) =>
      url.endsWith("/login")
        ? ok('{"token":"abc"}')
        : ok(JSON.stringify({ auth: vars.find((v) => v.key === "token")?.value })),
    );
    const login = entry("login", "https://x.test/login", {
      captures: [{ id: "c", enabled: true, variable: "token", source: "jsonPath", target: "$.token" }],
    });
    const me = entry("me", "https://x.test/me", {
      request: {
        ...emptyRequest(),
        url: "https://x.test/me",
        headers: [{ key: "Authorization", value: "Bearer {{token}}", enabled: true }],
        requiresSecretReview: false,
      },
      assertions: [
        { id: "a", enabled: true, source: "jsonPath", target: "$.auth", operator: "equals", expected: "sealed:abc" },
      ],
    });
    const summary = await runCollection(
      [login, me],
      [],
      new SessionVariables(),
      { stopOnFailure: true, delayMs: 0 },
      d,
      new AbortController().signal,
      () => {},
    );
    expect(summary).toMatchObject({ passed: 2, failed: 0 });
    expect(d.send.mock.calls[1][1]).toContainEqual({ key: "token", value: "sealed:abc", secret: true });
  });

  it("fails a request whose variable was not captured", async () => {
    const d = deps(() => ok("{}"));
    const login = entry("login", "https://x.test/login", {
      captures: [{ id: "c", enabled: true, variable: "token", source: "jsonPath", target: "$.token" }],
    });
    const me = entry("me", "https://x.test/{{token}}");
    const summary = await runCollection(
      [login, me],
      [],
      new SessionVariables(),
      { stopOnFailure: false, delayMs: 0 },
      d,
      new AbortController().signal,
      () => {},
    );
    expect(summary.steps[1]).toMatchObject({ status: "error", message: "변수 없음: token" });
    expect(d.send).toHaveBeenCalledTimes(1);
  });

  it("skips requests that need a secret review and continues without stop-on-failure", async () => {
    const d = deps((url) => (url.includes("bad") ? ok("{}", 500) : ok("{}")));
    const steps = [
      entry("secret", "https://x.test/a", { requiresSecretReview: true }),
      entry("bad", "https://x.test/bad", {
        assertions: [{ id: "s", enabled: true, source: "status", target: "", operator: "equals", expected: "200" }],
      }),
      entry("good", "https://x.test/good"),
    ];
    const summary = await runCollection(
      steps,
      [],
      new SessionVariables(),
      { stopOnFailure: false, delayMs: 0 },
      d,
      new AbortController().signal,
      () => {},
    );
    expect(summary.steps.map((s) => s.status)).toEqual(["skipped", "failed", "passed"]);
  });

  it("stops after the first failure when asked and marks the rest skipped", async () => {
    const d = deps(() => ok("{}", 500));
    const assertion = [
      { id: "s", enabled: true, source: "status" as const, target: "", operator: "equals" as const, expected: "200" },
    ];
    const summary = await runCollection(
      [entry("a", "https://x.test/a", { assertions: assertion }), entry("b", "https://x.test/b")],
      [],
      new SessionVariables(),
      { stopOnFailure: true, delayMs: 0 },
      d,
      new AbortController().signal,
      () => {},
    );
    expect(summary.steps.map((s) => s.status)).toEqual(["failed", "skipped"]);
  });

  it("cancels the in-flight request and skips the rest", async () => {
    const controller = new AbortController();
    const d = {
      ...deps(() => ok("{}")),
      send: vi.fn(
        (_request: unknown, _vars: unknown, signal: AbortSignal) =>
          new Promise<never>((_, reject) =>
            signal.addEventListener("abort", () => reject(new Error("요청이 취소되었습니다"))),
          ),
      ),
    };
    const running = runCollection(
      [entry("a", "https://x.test/a"), entry("b", "https://x.test/b")],
      [],
      new SessionVariables(),
      { stopOnFailure: false, delayMs: 0 },
      d as never,
      controller.signal,
      () => {},
    );
    controller.abort();
    const summary = await running;
    expect(summary.cancelled).toBe(true);
    expect(summary.steps.map((s) => s.status)).toEqual(["error", "skipped"]);
  });
});
it("a missing capture cannot reuse old session or environment values", async () => {
  const session = new SessionVariables();
  session.set("token", "previous", "sealed-previous");
  const d = deps(() => ok("{}"));
  const summary = await runCollection(
    [
      entry("login", "https://x.test/login", {
        captures: [{ id: "c", enabled: true, variable: "token", source: "jsonPath", target: "$.token" }],
      }),
      entry("next", "https://x.test/{{token}}"),
    ],
    [{ key: "token", value: "older", secret: false }],
    session,
    { stopOnFailure: false, delayMs: 0 },
    d,
    new AbortController().signal,
    () => {},
  );
  expect(summary.steps[1].message).toBe("변수 없음: token");
  expect(d.send).toHaveBeenCalledTimes(1);
  expect(session.entries()).toEqual([]);
});
it("never sends retained multipart file placeholders and clears failed sealed captures", async () => {
  const session = new SessionVariables();
  session.set("token", "old", "sealed-old");
  const d = {
    ...deps(() => ok('{"token":"new-secret"}')),
    seal: vi.fn(async () => {
      throw new Error("private failure");
    }),
  };
  const summary = await runCollection(
    [
      entry("file", "https://x.test", {
        request: {
          ...emptyRequest(),
          url: "https://x.test",
          body_kind: "multipart",
          multipart: [
            {
              kind: "file",
              name: "upload",
              value: "",
              file_path: "",
              file_name: "x.txt",
              content_type: "",
              enabled: true,
            },
          ],
          requiresSecretReview: false,
        },
      }),
      entry("login", "https://x.test/login", {
        captures: [{ id: "c", enabled: true, variable: "token", source: "jsonPath", target: "$.token" }],
      }),
    ],
    [],
    session,
    { stopOnFailure: false, delayMs: 0 },
    d,
    new AbortController().signal,
    () => {},
  );
  expect(summary.steps.map((step) => step.status)).toEqual(["skipped", "error"]);
  expect(d.send).toHaveBeenCalledTimes(1);
  expect(session.entries()).toEqual([]);
  expect(JSON.stringify(summary)).not.toContain("new-secret");
  expect(JSON.stringify(summary)).not.toContain("private failure");
});
it("enforces request and delay bounds before sending", async () => {
  const d = deps(() => ok("{}"));
  for (const [entries, delayMs] of [
    [Array.from({ length: 201 }, (_, i) => entry(String(i), "https://x.test")), 0],
    [[], 5001],
  ] as const) {
    await expect(
      runCollection(
        [...entries],
        [],
        new SessionVariables(),
        { stopOnFailure: false, delayMs },
        d,
        new AbortController().signal,
        () => {},
      ),
    ).rejects.toThrow("러너 한도");
  }
  expect(d.send).not.toHaveBeenCalled();
});
it("refuses to undo a discard over a newer captured value", async () => {
  const session = new SessionVariables();
  session.set("token", "first", "sealed-first");
  const undo = session.discard(["token"]);
  session.set("token", "second", "sealed-second");
  await expect(undo()).rejects.toThrow("그 사이 바뀐 내용");
  expect(session.entries()).toEqual([{ name: "token", plain: "second" }]);
});
