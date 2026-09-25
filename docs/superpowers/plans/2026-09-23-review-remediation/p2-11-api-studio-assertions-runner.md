# P2-11 API Studio 보강 2: 검증·요청 이어 쓰기·컬렉션 실행 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** 저장한 요청에 응답 검증(assertion)과 값 캡처(capture)를 붙이고, 캡처한 값을 다음 요청의 `{{변수}}`로 이어 쓰며, 컬렉션이나 폴더를 순서대로 실행해 결과를 한 표로 보는 러너를 만든다(D25 4순위, `review.md` §8 표 5번).

**Architecture:**
- 전부 프런트의 순수 로직 + 기존 전송 경로다. 새 native 명령은 없다.
  - `lib/jsonPath.ts`: 작은 JSONPath 부분집합(`$`, `.key`, `['key']`, `[n]`(음수는 뒤에서), `[*]`, `.*`, `..key`).
  - `lib/assertions.ts`: 출처(status·header·jsonPath·body·duration) × 연산(equals·notEquals·contains·notContains·matches·exists·notExists·lessThan·greaterThan) 평가.
  - `lib/captures.ts`: 응답에서 값을 뽑아 변수 이름에 넣는다.
  - `lib/runner.ts`: 요청 목록을 순서대로 보내는 오케스트레이터(전송·대기 함수를 주입받아 테스트 가능).
- 저장: `CollectionEntry`에 선택 필드 `assertions?: Assertion[]`, `captures?: Capture[]`(없으면 빈 목록). 컬렉션 문서 검증(`cleanCollectionEntry`), JSON 내보내기, 파일 컬렉션(P2-10 PR B) 요청 파일에 함께 들어간다.
- 캡처 값은 세션 메모리에만 둔다(저장하지 않음). 전송할 때 native로 넘기는 변수 목록 끝에 붙인다 — native `resolve_template`은 뒤의 값이 앞의 같은 이름을 덮는다. 데스크톱에서는 `sealSecret`으로 봉인해 `secret: true`로 넘겨 native redaction 대상이 되게 한다. 화면에는 가려서 보이고 "보기"를 눌러야 드러난다.
- 러너는 `requiresSecretReview` 요청과 파일 파트를 다시 골라야 하는 multipart 요청을 건너뛴다(가린 값이나 빈 파일을 보내지 않게).

**Tech Stack:** TypeScript·React 19, Vitest

**Spec:** `review.md` §8 신규 기능 표 5번 · `00-roadmap.md` D25

## Global Constraints

- `00-roadmap.md` §3 전부 적용. 새 의존성 없음.
- 한도: 요청당 assertion 50개·capture 20개, JSONPath 식 256자·방문 노드 10,000개, 캡처 값 64KiB, 러너 한 번에 요청 200개, 요청 사이 대기 0–5,000ms.
- 변수 이름 규칙: `^[A-Za-z_][A-Za-z0-9_.-]{0,63}$`.
- 정규식 연산은 `u` 플래그로 만들고, 잘못된 식은 그 assertion만 실패(메시지 "정규식이 올바르지 않습니다")로 처리한다.
- assertion의 기대값에 알려진 토큰 모양(`looksLikeSecret`)이 있으면 저장 전에 `[REDACTED]`로 바꾸고 요청을 "비밀 검토 필요"로 표시한다(기존 저장 규칙과 같게).

## Review Focus

1. JSON이 아닌 응답·binary 응답에서 jsonPath·body 검증은 "JSON이 아닌 응답"·"binary 응답"으로 실패하고 다른 검증은 계속 평가된다. (Task 2 테스트)
2. 캡처가 실패(경로 없음)하면 그 변수는 이전 값을 유지하지 않고 비워지며, 이 변수를 쓰는 다음 요청은 러너에서 "변수 없음: token"으로 실패한다. (Task 4 테스트)
3. "실패하면 멈춤"이 꺼져 있으면 실패 뒤에도 끝까지 실행하고, 중지 버튼은 진행 중 요청을 취소한 뒤 나머지를 "건너뜀"으로 표시한다. (Task 4 테스트)
4. 비밀 검토가 필요한 요청은 러너에서 보내지 않는다. (Task 4 테스트)
5. assertion·capture가 없는 옛 컬렉션 문서도 그대로 열리고, 새 필드가 있는 문서를 내보냈다 다시 가져와도 값이 같다. (Task 3 테스트)

## Branch · PR

- 묶음: **B12** — 브랜치 `feat/devbox-api-studio/imports-runner-auth`, PR 제목 `feat(devbox-api-studio): imports, file collections, runner, OAuth 2.0, TLS and code generation`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(devbox-api-studio): assertions, request chaining and a collection runner`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: JSONPath

**Files:** Create `packages/api-studio-features/src/requests/lib/jsonPath.ts`, `jsonPath.test.ts`

**Interfaces (Produces):** `evaluateJsonPath(value: unknown, path: string): unknown[]`, `class JsonPathError extends Error`

- [ ] **Step 1: 실패하는 테스트**

```ts
import { describe, expect, it } from "vitest";
import { evaluateJsonPath, JsonPathError } from "./jsonPath";

const doc = { data: { items: [{ id: 1, tags: ["a"] }, { id: 2, tags: [] }], "odd key": true }, token: "t" };

describe("jsonPath", () => {
  it("selects members, indices and wildcards", () => {
    expect(evaluateJsonPath(doc, "$.token")).toEqual(["t"]);
    expect(evaluateJsonPath(doc, "$.data.items[0].id")).toEqual([1]);
    expect(evaluateJsonPath(doc, "$.data.items[-1].id")).toEqual([2]);
    expect(evaluateJsonPath(doc, "$.data.items[*].id")).toEqual([1, 2]);
    expect(evaluateJsonPath(doc, "$['data']['odd key']")).toEqual([true]);
    expect(evaluateJsonPath(doc, "$.data.*")).toHaveLength(2);
  });
  it("descends recursively", () => {
    expect(evaluateJsonPath(doc, "$..id")).toEqual([1, 2]);
  });
  it("returns nothing for missing paths and rejects bad syntax", () => {
    expect(evaluateJsonPath(doc, "$.missing.deeper")).toEqual([]);
    for (const bad of ["token", "$.", "$[", "$.a[?(@.b)]", "$..", "$['x"]) {
      expect(() => evaluateJsonPath(doc, bad)).toThrow(JsonPathError);
    }
  });
  it("stops on very large documents", () => {
    const wide = { list: Array.from({ length: 20_000 }, (_, i) => ({ i })) };
    expect(() => evaluateJsonPath(wide, "$..i")).toThrow("JSONPath 탐색 한도");
  });
});
```

- [ ] **Step 2: 실패 확인·구현·확인** — 식을 토큰(`root`, `member(name)`, `index(n)`, `wildcard`, `descend(name | wildcard)`)으로 파싱한 뒤 현재 노드 목록에 차례로 적용한다. 필터 식(`?(`)·슬라이스(`:`)·스크립트는 지원하지 않고 `JsonPathError("지원하지 않는 JSONPath입니다")`. 방문 노드 수가 10,000을 넘으면 `JsonPathError("JSONPath 탐색 한도를 넘었습니다")`. Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/requests/lib/jsonPath.test.ts` → PASS. 커밋: `git add -A && git commit -m "feat(devbox-api-studio): JSONPath subset"`

---

### Task 2: assertion과 capture

**Files:** Create `lib/{assertions.ts,assertions.test.ts,captures.ts,captures.test.ts}`

**Interfaces (Produces):**
- `type AssertionSource = "status" | "header" | "jsonPath" | "body" | "duration"`, `type AssertionOperator = "equals" | "notEquals" | "contains" | "notContains" | "matches" | "exists" | "notExists" | "lessThan" | "greaterThan"`, `interface Assertion { id: string; enabled: boolean; source: AssertionSource; target: string; operator: AssertionOperator; expected: string }`, `interface AssertionResult { id: string; passed: boolean; actual: string | null; message: string }`, `evaluateAssertions(assertions: Assertion[], response: ApiResponse): AssertionResult[]`, `validateAssertion(a: Assertion): string | null`
- `interface Capture { id: string; enabled: boolean; variable: string; source: "jsonPath" | "header" | "status"; target: string }`, `interface CaptureOutcome { values: Map<string, string>; missing: string[]; errors: string[] }`, `applyCaptures(captures: Capture[], response: ApiResponse): CaptureOutcome`, `VARIABLE_NAME: RegExp`

- [ ] **Step 1: 실패하는 테스트** — `assertions.test.ts`

```ts
import { describe, expect, it } from "vitest";
import { evaluateAssertions, type Assertion } from "./assertions";
import type { ApiResponse } from "../types";

function response(overrides: Partial<ApiResponse> = {}): ApiResponse {
  return { status: 201, status_text: "Created", headers: [{ key: "Content-Type", value: "application/json" }, { key: "X-Id", value: "42" }],
    duration_ms: 120, size_bytes: 20, body: '{"user":{"id":7,"name":"kim"}}', is_json: true, final_url: "https://x.test", redirects: [],
    cookies: [], response_id: null, raw_headers_available: false, headers_truncated: false, ...overrides };
}
const a = (source: Assertion["source"], operator: Assertion["operator"], expected = "", target = ""): Assertion =>
  ({ id: `${source}-${operator}-${target}`, enabled: true, source, target, operator, expected });

describe("assertions", () => {
  it("checks status, headers, JSON values and duration", () => {
    const results = evaluateAssertions([
      a("status", "equals", "201"),
      a("header", "equals", "42", "x-id"),
      a("jsonPath", "equals", "7", "$.user.id"),
      a("jsonPath", "exists", "", "$.user.name"),
      a("jsonPath", "notExists", "", "$.user.email"),
      a("body", "contains", "kim"),
      a("duration", "lessThan", "500"),
    ], response());
    expect(results.map((r) => r.passed)).toEqual([true, true, true, true, true, true, true]);
  });

  it("reports actual values and reasons on failure", () => {
    const [status, regex, numeric] = evaluateAssertions([
      a("status", "equals", "200"),
      a("body", "matches", "(unclosed"),
      a("jsonPath", "greaterThan", "10", "$.user.name"),
    ], response());
    expect(status).toMatchObject({ passed: false, actual: "201" });
    expect(regex).toMatchObject({ passed: false, message: "정규식이 올바르지 않습니다" });
    expect(numeric).toMatchObject({ passed: false, message: "숫자가 아닌 값입니다" });
  });

  it("fails JSON checks on non-JSON and binary responses without stopping others", () => {
    const text = evaluateAssertions([a("jsonPath", "exists", "", "$.a"), a("status", "equals", "201")], response({ is_json: false, body: "<html>" }));
    expect(text.map((r) => [r.passed, r.message])).toEqual([[false, "JSON이 아닌 응답"], [true, ""]]);
    const binary = evaluateAssertions([a("body", "contains", "x")], response({ binary: { media_type: "image/png", size_bytes: 3, hex_preview: "00", hex_truncated: false } as ApiResponse["binary"] }));
    expect(binary[0]).toMatchObject({ passed: false, message: "binary 응답" });
  });

  it("skips disabled assertions", () => {
    expect(evaluateAssertions([{ ...a("status", "equals", "500"), enabled: false }], response())).toEqual([]);
  });
});
```

  `captures.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { applyCaptures } from "./captures";

const res = { status: 200, headers: [{ key: "Location", value: "/users/7" }], body: '{"access_token":"abc","user":{"id":7}}', is_json: true } as never;

describe("captures", () => {
  it("extracts JSON, header and status values as strings", () => {
    const out = applyCaptures([
      { id: "1", enabled: true, variable: "token", source: "jsonPath", target: "$.access_token" },
      { id: "2", enabled: true, variable: "userId", source: "jsonPath", target: "$.user.id" },
      { id: "3", enabled: true, variable: "location", source: "header", target: "location" },
      { id: "4", enabled: true, variable: "lastStatus", source: "status", target: "" },
    ], res);
    expect(Object.fromEntries(out.values)).toEqual({ token: "abc", userId: "7", location: "/users/7", lastStatus: "200" });
    expect(out.missing).toEqual([]);
  });
  it("reports missing values and invalid variable names", () => {
    const out = applyCaptures([
      { id: "1", enabled: true, variable: "refresh", source: "jsonPath", target: "$.refresh_token" },
      { id: "2", enabled: true, variable: "bad name", source: "status", target: "" },
    ], res);
    expect(out.missing).toEqual(["refresh"]);
    expect(out.errors).toEqual(["변수 이름이 올바르지 않습니다: bad name"]);
  });
});
```

- [ ] **Step 2: 실패 확인·구현·확인** — header 비교는 이름 대소문자 무시, 첫 값. jsonPath 결과가 여러 개면 첫 값(exists는 하나라도 있으면 통과). 값이 문자열이 아니면 `JSON.stringify`. equals·notEquals는 양쪽이 모두 유한한 숫자로 읽히면 숫자로, 아니면 문자열로 비교. lessThan·greaterThan은 숫자만. 실패 메시지는 기대·실제를 담는다(예: `"기대 200, 실제 201"`). 캡처 값이 64KiB를 넘으면 오류. Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/requests/lib/assertions.test.ts src/requests/lib/captures.test.ts` → PASS. 커밋: `git add -A && git commit -m "feat(devbox-api-studio): evaluate assertions and captures"`

---

### Task 3: 저장 형식

**Files:** Modify `lib/{collections.ts,transfer.ts,fileCollection.ts}`(+tests), P1-17 컬렉션 문서 검증 위치(`DocumentStorage`의 컬렉션 parse)

- [ ] **Step 1: 실패하는 테스트** — `transfer.test.ts`·`collections.test.ts`·`fileCollection.test.ts`에 추가:
  - 옛 문서(필드 없음)를 읽으면 `assertions`·`captures`가 빈 배열.
  - 두 필드가 있는 컬렉션을 `serializeCollectionExport` → `parseCollectionExport`하면 같은 값, 파일 컬렉션 요청 파일도 같은 값으로 왕복.
  - 알 수 없는 연산·출처, 51번째 assertion, 잘못된 변수 이름은 그 항목만 버리고(`cleanCollectionEntry`가 항목 전체를 거부하지 않음) 경고 수를 센다.
  - 기대값 `ghp_abcdefghijklmnopqrstuvwxyz0123`은 `[REDACTED]`가 되고 `requiresSecretReview: true`.
- [ ] **Step 2: 실패 확인·구현·확인** — `CollectionEntry`에 두 선택 필드를 더하고 `onlyKeys` 허용 목록에 넣는다. 정리 함수 `cleanAssertions(value: unknown): { assertions: Assertion[]; dropped: number }`와 `cleanCaptures`를 `assertions.ts`·`captures.ts`에 두고 세 경로(저장소 읽기, JSON 가져오기, 파일 컬렉션)가 같이 쓴다. Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/requests/lib` → PASS. 커밋: `git add -A && git commit -m "feat(devbox-api-studio): persist assertions and captures with requests"`

---

### Task 4: 러너

**Files:** Create `lib/runner.ts`, `runner.test.ts`

**Interfaces (Produces):**
- `interface RunOptions { stopOnFailure: boolean; delayMs: number }`
- `interface RunDeps { send(request: RequestTemplate, variables: EnvVariable[], signal: AbortSignal): Promise<ApiResponse>; seal(value: string): Promise<string>; sleep(ms: number, signal: AbortSignal): Promise<void> }`
- `interface RunStep { entryId: string; name: string; status: "passed" | "failed" | "error" | "skipped"; httpStatus: number | null; durationMs: number | null; assertions: AssertionResult[]; captured: string[]; message: string }`
- `interface RunSummary { steps: RunStep[]; passed: number; failed: number; errors: number; skipped: number; cancelled: boolean }`
- `runCollection(entries: CollectionEntry[], environment: EnvVariable[], session: SessionVariables, options: RunOptions, deps: RunDeps, signal: AbortSignal, onStep: (step: RunStep) => void): Promise<RunSummary>`
- `class SessionVariables { set(name: string, plain: string, sealed: string | null): void; delete(name: string): void; forSend(): EnvVariable[]; entries(): { name: string; plain: string }[] }`
- `missingVariables(request: RequestTemplate, available: Set<string>): string[]`

- [ ] **Step 1: 실패하는 테스트**

```ts
import { describe, expect, it, vi } from "vitest";
import { runCollection, SessionVariables } from "./runner";
import { emptyRequest } from "./importers";

const entry = (id: string, url: string, extra: Record<string, unknown> = {}) => ({
  id, name: id, folder: "", saved_at: 1, requiresSecretReview: false,
  request: { ...emptyRequest(), url, requiresSecretReview: false }, assertions: [], captures: [], ...extra,
});
const ok = (body: string, status = 200) => ({ status, status_text: "", headers: [], duration_ms: 5, size_bytes: body.length, body, is_json: true,
  final_url: "", redirects: [], cookies: [], response_id: null, raw_headers_available: false, headers_truncated: false });
const deps = (send: (url: string, vars: { key: string; value: string }[]) => ReturnType<typeof ok>) => ({
  send: vi.fn(async (request: { url: string }, variables: { key: string; value: string }[]) => send(request.url, variables)),
  seal: vi.fn(async (value: string) => `sealed:${value}`),
  sleep: vi.fn(async () => {}),
});

describe("collection runner", () => {
  it("chains a captured token into the next request", async () => {
    const d = deps((url, vars) => url.endsWith("/login") ? ok('{"token":"abc"}') : ok(JSON.stringify({ auth: vars.find((v) => v.key === "token")?.value })));
    const login = entry("login", "https://x.test/login", { captures: [{ id: "c", enabled: true, variable: "token", source: "jsonPath", target: "$.token" }] });
    const me = entry("me", "https://x.test/me", { request: { ...emptyRequest(), url: "https://x.test/me", headers: [{ key: "Authorization", value: "Bearer {{token}}", enabled: true }], requiresSecretReview: false },
      assertions: [{ id: "a", enabled: true, source: "jsonPath", target: "$.auth", operator: "equals", expected: "sealed:abc" }] });
    const summary = await runCollection([login, me], [], new SessionVariables(), { stopOnFailure: true, delayMs: 0 }, d, new AbortController().signal, () => {});
    expect(summary).toMatchObject({ passed: 2, failed: 0 });
    expect(d.send.mock.calls[1][1]).toContainEqual({ key: "token", value: "sealed:abc", secret: true });
  });

  it("fails a request whose variable was not captured", async () => {
    const d = deps(() => ok("{}"));
    const login = entry("login", "https://x.test/login", { captures: [{ id: "c", enabled: true, variable: "token", source: "jsonPath", target: "$.token" }] });
    const me = entry("me", "https://x.test/{{token}}");
    const summary = await runCollection([login, me], [], new SessionVariables(), { stopOnFailure: false, delayMs: 0 }, d, new AbortController().signal, () => {});
    expect(summary.steps[1]).toMatchObject({ status: "error", message: "변수 없음: token" });
    expect(d.send).toHaveBeenCalledTimes(1);
  });

  it("skips requests that need a secret review and continues without stop-on-failure", async () => {
    const d = deps((url) => url.includes("bad") ? ok("{}", 500) : ok("{}"));
    const steps = [
      entry("secret", "https://x.test/a", { requiresSecretReview: true }),
      entry("bad", "https://x.test/bad", { assertions: [{ id: "s", enabled: true, source: "status", target: "", operator: "equals", expected: "200" }] }),
      entry("good", "https://x.test/good"),
    ];
    const summary = await runCollection(steps, [], new SessionVariables(), { stopOnFailure: false, delayMs: 0 }, d, new AbortController().signal, () => {});
    expect(summary.steps.map((s) => s.status)).toEqual(["skipped", "failed", "passed"]);
  });

  it("stops after the first failure when asked and marks the rest skipped", async () => {
    const d = deps(() => ok("{}", 500));
    const assertion = [{ id: "s", enabled: true, source: "status" as const, target: "", operator: "equals" as const, expected: "200" }];
    const summary = await runCollection([entry("a", "https://x.test/a", { assertions: assertion }), entry("b", "https://x.test/b")], [], new SessionVariables(), { stopOnFailure: true, delayMs: 0 }, d, new AbortController().signal, () => {});
    expect(summary.steps.map((s) => s.status)).toEqual(["failed", "skipped"]);
  });

  it("cancels the in-flight request and skips the rest", async () => {
    const controller = new AbortController();
    const d = { ...deps(() => ok("{}")), send: vi.fn((_request: unknown, _vars: unknown, signal: AbortSignal) => new Promise<never>((_, reject) => signal.addEventListener("abort", () => reject(new Error("요청이 취소되었습니다"))))) };
    const running = runCollection([entry("a", "https://x.test/a"), entry("b", "https://x.test/b")], [], new SessionVariables(), { stopOnFailure: false, delayMs: 0 }, d as never, controller.signal, () => {});
    controller.abort();
    const summary = await running;
    expect(summary.cancelled).toBe(true);
    expect(summary.steps.map((s) => s.status)).toEqual(["error", "skipped"]);
  });
});
```

- [ ] **Step 2: 실패 확인·구현·확인** — 요청마다: 건너뛸 조건(비밀 검토, 파일 파트가 경로 없이 저장된 multipart, 중지됨) → `missingVariables`(템플릿의 `{{name}}` 중 환경·세션 어디에도 없는 이름; 러너 시작 때 세션 값이 있으면 그것도 사용) → `deps.send(request, [...environment, ...session.forSend()], signal)` → `evaluateAssertions` → `applyCaptures`(성공한 값은 `seal`로 봉인해 세션에 넣고, `missing`은 세션에서 지운다) → 상태 결정(assertion 하나라도 실패 → failed, 전송 오류 → error) → `onStep` → `delayMs` 대기. `SessionVariables.forSend()`는 봉인 값이 있으면 `{key, value: sealed, secret: true}`, 없으면(브라우저 모드) `{key, value: plain, secret: false}`. Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/requests/lib/runner.test.ts` → PASS. 커밋: `git add -A && git commit -m "feat(devbox-api-studio): collection runner with chaining"`

---

### Task 5: 화면

**Files:** Create `packages/api-studio-features/src/requests/{AssertionEditor.tsx,CaptureEditor.tsx,SessionVariablesPanel.tsx,RunnerPanel.tsx}` + 각 `.test.tsx`; Modify 요청 편집 탭(P1-15에서 나눈 부품), `ResponseViewer.tsx`(+test), 컬렉션 영역 부품

- [ ] **Step 1: 실패하는 테스트**
  - `AssertionEditor.test.tsx`: 행 추가(출처·대상·연산·기대값), 출처가 status·body·duration이면 대상 칸이 없고, exists·notExists면 기대값 칸이 없다. 잘못된 JSONPath는 칸 옆에 "지원하지 않는 JSONPath입니다". 50개에서 추가 버튼 비활성.
  - `CaptureEditor.test.tsx`: 변수 이름 규칙 위반을 칸 옆에 표시.
  - `ResponseViewer.test.tsx`에 추가: 응답이 오면 "검증" 탭에 "3개 중 2개 통과"와 실패 항목의 기대·실제. 캡처 결과 "token ← $.access_token 캡처함".
  - `SessionVariablesPanel.test.tsx`: 캡처한 변수 목록(값 가림, "보기"로 드러냄, "지우기", "모두 지우기").
  - `RunnerPanel.test.tsx`: 범위(컬렉션 전체·폴더 선택), "실패하면 멈춤", 대기(ms) → "실행" → 진행 목록이 한 줄씩 채워짐 → 요약("통과 5 · 실패 1 · 오류 0 · 건너뜀 1") → "결과 복사"가 JSON(요약·단계, 응답 본문 제외)을 클립보드에 쓴다 → "중지"는 실행 중에만 활성. 실패 행을 누르면 그 요청을 편집기에 연다.
  - 모든 새 화면 axe 위반 0.
- [ ] **Step 2: 실패 확인** — Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/requests` → FAIL
- [ ] **Step 3: 구현** — 요청 편집 탭에 "검증"·"캡처"를 더하고, 저장하면 컬렉션 항목에 같이 저장된다. 수동 전송 뒤에도 같은 평가·캡처를 한다(세션 변수 갱신). `RunnerPanel`은 `useOperation`(P1-15)과 `AbortController`로 중지하고, `deps.send`는 기존 `sendRequest`, `deps.seal`은 `sealSecret`(브라우저 모드면 `null` 봉인), `deps.sleep`은 `setTimeout` + signal. 러너는 컬렉션 영역의 "실행" 버튼에서 연다.
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS. `git add -A && git commit -m "feat(devbox-api-studio): assertion, capture and runner views"`

---

### Task 6: PR 완료

- [ ] 가이드에 "검증과 실행" 문단(JSONPath 지원 범위, 캡처 값은 저장되지 않음, 러너가 건너뛰는 요청).
- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): 로그인 → 토큰 캡처 → 인증 요청 두 개를 러너로 실행해 모두 통과, 서버를 끈 상태에서 오류 표시, 중지 버튼.
