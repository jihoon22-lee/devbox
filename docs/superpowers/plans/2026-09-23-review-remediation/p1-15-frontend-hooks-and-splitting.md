# P1-15 프런트 공통 hook과 대형 컴포넌트 분리 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** 손으로 반복하던 세 가지 패턴(바쁨·오류·취소 상태 133곳, `setInterval` 폴링 19곳, 미리보기→적용→취소 흐름)을 공통 hook 세 개로 바꾸고, 창이 최소화되면 폴링이 멈추게 하며, 2,000줄이 넘는 컴포넌트 6개를 기능 단위 파일로 나눈다(A5, P2 폴링 부분, D20).

**Architecture:** 새 패키지 `@devbox/hooks`에 `usePolling`(겹침 없는 `setTimeout` 사슬, `active`·`document.hidden`이면 멈춤, 다시 보이면 즉시 1회), `useOperation`(busy·issue·취소·오래된 결과 무시·언마운트 안전), `useReviewFlow`(preview→reviewed→applying→done/failed 상태 기계, 언마운트 시 미리보기 폐기)를 둔다. 라이브러리 없이 React state/useReducer만 쓴다(D20). 대형 컴포넌트는 화면 절(section) 경계대로 `components/`·`hooks/`로 옮기고, "TSX 1,000줄 초과 금지" 검사를 CI에 넣되 지금 초과 파일은 현재 줄 수를 상한으로 하는 허용 목록으로 시작해 줄어들게만 한다.

**Tech Stack:** React 19, TypeScript, Vitest(fake timers), Testing Library

**Spec:** `review.md` §6 A5, §5 P2 · `00-roadmap.md` D20

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 동작 불변: 분리는 코드 이동이다. 각 화면의 기존 테스트가 그대로 통과해야 한다(문구·role·순서 불변).
- 폴링 간격은 지금 값 그대로 옮긴다. 달라지는 것은 "숨김 중 멈춤·다시 보일 때 즉시 1회·겹침 없음"뿐이다.
- 터미널 출력 폴링(`terminalReplay.ts`)은 P1-16에서 Channel로 바꾸므로 여기서 건드리지 않는다.
- P1-07이 넣은 `useExhaustiveDependencies` suppression은 이 PR에서 손대는 파일에 한해 의존성을 바로잡고 지운다. 동작이 바뀌는 곳은 suppression을 남기고 사유를 구체적으로 적는다.

## Review Focus

1. 창을 최소화한 동안 Activity·Logs·Tasks 폴링 IPC가 0이 되고, 다시 열면 즉시 새로 고친다. (Task 1 테스트, 사용자 실기)
2. 느린 폴링 콜백(응답 3초)이 1초 간격보다 길 때 요청이 겹치지 않는다. (Task 1)
3. 버튼을 빠르게 두 번 눌러도 작업이 한 번만 실행되고, 앞선 느린 결과가 나중 결과를 덮지 않는다(`useOperation` 순번). (Task 1)
4. 미리보기를 연 채 화면을 떠나면 native 미리보기가 폐기된다(`useReviewFlow` 언마운트). (Task 1)
5. 분리 뒤 한 파일에서만 쓰던 state를 두 컴포넌트가 따로 가져 동기화가 깨짐 → state는 부모(App)나 hook 하나에만 두고 props로 내려 준다(분리 규칙). (Task 3)

## Branch · PR

- 묶음: **B8** — 브랜치 `refactor/suite/hooks-streaming-store-undo`, PR 제목 `refactor(suite): shared hooks, terminal streaming, native API Studio store, undo and agent protocol`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `refactor(frontend): share polling, operation and review hooks and split large views`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: `@devbox/hooks`

**Files:** Create `packages/hooks/{package.json,tsconfig.json,vitest.config.ts,src/index.ts,src/usePolling.ts,src/useOperation.ts,src/useReviewFlow.ts,src/*.test.tsx}`

**Interfaces (Produces):**
- `usePolling(callback: () => Promise<void> | void, options: { intervalMs: number; active?: boolean; pauseWhenHidden?: boolean; immediate?: boolean }): { refresh: () => void }`
- `useOperation(): { busy: boolean; issue: string | null; run<T>(task: (signal: AbortSignal) => Promise<T>): Promise<T | undefined>; cancel(): void; clearIssue(): void }` (+ `messageOf(error: unknown): string`)
- `useReviewFlow<P, R>(steps: { preview(signal: AbortSignal): Promise<P>; apply(preview: P): Promise<R>; discard?(preview: P): Promise<void> }): { state: "idle" | "previewing" | "reviewed" | "applying" | "done" | "failed"; preview: P | null; result: R | null; issue: string | null; start(): Promise<void>; confirm(): Promise<void>; reset(): Promise<void> }`

- [ ] **Step 1: 패키지 뼈대** — `packages/markdown-view`(P1-10)와 같은 구조로 만들고 `name`을 `@devbox/hooks`로 한다.

- [ ] **Step 2: 실패하는 테스트** — `src/usePolling.test.tsx`

```tsx
import { act, cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePolling } from "./usePolling";

function Probe({ callback, active = true, intervalMs = 1000 }: { callback: () => Promise<void> | void; active?: boolean; intervalMs?: number }) {
  usePolling(callback, { intervalMs, active });
  return null;
}

function setHidden(hidden: boolean) {
  Object.defineProperty(document, "hidden", { configurable: true, get: () => hidden });
  document.dispatchEvent(new Event("visibilitychange"));
}

beforeEach(() => { vi.useFakeTimers(); setHidden(false); });
afterEach(() => { cleanup(); vi.useRealTimers(); });

describe("usePolling", () => {
  it("runs immediately and then every interval", async () => {
    const callback = vi.fn();
    render(<Probe callback={callback} />);
    await act(async () => {});
    expect(callback).toHaveBeenCalledTimes(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
    expect(callback).toHaveBeenCalledTimes(4);
  });

  it("never overlaps a slow callback", async () => {
    let finish!: () => void;
    const callback = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
    render(<Probe callback={callback} />);
    await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
    expect(callback).toHaveBeenCalledTimes(1);
    await act(async () => { finish(); await vi.advanceTimersByTimeAsync(1000); });
    expect(callback).toHaveBeenCalledTimes(2);
  });

  it("pauses while hidden and refreshes once when visible again", async () => {
    const callback = vi.fn();
    render(<Probe callback={callback} />);
    await act(async () => {});
    act(() => setHidden(true));
    await act(async () => { await vi.advanceTimersByTimeAsync(10_000); });
    expect(callback).toHaveBeenCalledTimes(1);
    await act(async () => { setHidden(false); });
    expect(callback).toHaveBeenCalledTimes(2);
  });

  it("does nothing while inactive", async () => {
    const callback = vi.fn();
    render(<Probe callback={callback} active={false} />);
    await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
    expect(callback).not.toHaveBeenCalled();
  });
});
```

`src/useOperation.test.tsx`

```tsx
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useOperation } from "./useOperation";

describe("useOperation", () => {
  it("tracks busy and keeps only the latest result", async () => {
    const { result } = renderHook(() => useOperation());
    let slow!: (value: string) => void;
    let first: Promise<string | undefined>;
    act(() => { first = result.current.run(() => new Promise<string>((resolve) => { slow = resolve; })); });
    expect(result.current.busy).toBe(true);
    let second: string | undefined;
    await act(async () => { second = await result.current.run(async () => "fresh"); });
    await act(async () => { slow("stale"); });
    expect(second).toBe("fresh");
    await expect(first!).resolves.toBeUndefined();
    expect(result.current.busy).toBe(false);
  });

  it("records the error message and clears it on the next run", async () => {
    const { result } = renderHook(() => useOperation());
    await act(async () => { await result.current.run(async () => { throw new Error("저장하지 못했습니다."); }); });
    expect(result.current.issue).toBe("저장하지 못했습니다.");
    await act(async () => { await result.current.run(async () => 1); });
    expect(result.current.issue).toBeNull();
  });
});
```

`src/useReviewFlow.test.tsx`

```tsx
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useReviewFlow } from "./useReviewFlow";

describe("useReviewFlow", () => {
  it("previews, applies and discards on unmount", async () => {
    const discard = vi.fn(async () => {});
    const { result, unmount } = renderHook(() => useReviewFlow({ preview: async () => ({ id: "p" }), apply: async (p: { id: string }) => p.id, discard }));
    await act(async () => { await result.current.start(); });
    expect(result.current.state).toBe("reviewed");
    unmount();
    expect(discard).toHaveBeenCalledWith({ id: "p" });
  });

  it("applies once and reports failure", async () => {
    const apply = vi.fn(async () => { throw new Error("충돌"); });
    const { result } = renderHook(() => useReviewFlow({ preview: async () => 1, apply }));
    await act(async () => { await result.current.start(); });
    await act(async () => { await result.current.confirm(); });
    expect(result.current.state).toBe("failed");
    expect(result.current.issue).toBe("충돌");
    expect(apply).toHaveBeenCalledTimes(1);
  });
});
```

- [ ] **Step 3: 실패 확인** — Run: `pnpm install && pnpm --filter @devbox/hooks exec vitest run` → FAIL.

- [ ] **Step 4: 구현**

`src/usePolling.ts`:

```ts
import { useCallback, useEffect, useRef } from "react";

export interface PollingOptions { intervalMs: number; active?: boolean; pauseWhenHidden?: boolean; immediate?: boolean }

/** Poll without overlap. Paused while inactive or while the window is hidden;
 * runs once as soon as it becomes visible again. */
export function usePolling(callback: () => Promise<void> | void, { intervalMs, active = true, pauseWhenHidden = true, immediate = true }: PollingOptions): { refresh: () => void } {
  const latest = useRef(callback);
  latest.current = callback;
  const tick = useRef<() => void>(() => {});

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let running = false;
    const hidden = () => pauseWhenHidden && typeof document !== "undefined" && document.hidden;
    const schedule = () => {
      if (cancelled || hidden()) return;
      timer = setTimeout(run, intervalMs);
    };
    const run = async () => {
      if (cancelled || running || hidden()) return;
      running = true;
      try { await latest.current(); } catch { /* the caller reports its own errors */ }
      running = false;
      schedule();
    };
    tick.current = () => { if (timer) clearTimeout(timer); void run(); };
    const onVisibility = () => { if (!hidden()) tick.current(); };
    document.addEventListener("visibilitychange", onVisibility);
    if (immediate) void run(); else schedule();
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisibility);
      tick.current = () => {};
    };
  }, [active, intervalMs, pauseWhenHidden, immediate]);

  return { refresh: useCallback(() => tick.current(), []) };
}
```

`src/useOperation.ts`:

```ts
import { useCallback, useEffect, useRef, useState } from "react";

export function messageOf(error: unknown): string {
  return error instanceof Error && error.message ? error.message : "작업을 완료하지 못했습니다. 다시 시도해 주세요.";
}

export function useOperation() {
  const [busy, setBusy] = useState(false);
  const [issue, setIssue] = useState<string | null>(null);
  const sequence = useRef(0);
  const controller = useRef<AbortController | null>(null);
  const mounted = useRef(true);
  useEffect(() => () => { mounted.current = false; controller.current?.abort(); }, []);

  const run = useCallback(async <T,>(task: (signal: AbortSignal) => Promise<T>): Promise<T | undefined> => {
    const id = ++sequence.current;
    controller.current?.abort();
    const abort = new AbortController();
    controller.current = abort;
    setBusy(true);
    setIssue(null);
    try {
      const value = await task(abort.signal);
      return mounted.current && id === sequence.current ? value : undefined;
    } catch (error) {
      if (mounted.current && id === sequence.current && !abort.signal.aborted) setIssue(messageOf(error));
      return undefined;
    } finally {
      if (mounted.current && id === sequence.current) setBusy(false);
    }
  }, []);

  const cancel = useCallback(() => { controller.current?.abort(); sequence.current += 1; setBusy(false); }, []);
  const clearIssue = useCallback(() => setIssue(null), []);
  return { busy, issue, run, cancel, clearIssue };
}
```

`src/useReviewFlow.ts`:

```ts
import { useCallback, useEffect, useReducer, useRef } from "react";
import { messageOf } from "./useOperation";

type State<P, R> = { state: "idle" | "previewing" | "reviewed" | "applying" | "done" | "failed"; preview: P | null; result: R | null; issue: string | null };
type Action<P, R> = { type: "previewing" } | { type: "reviewed"; preview: P } | { type: "applying" } | { type: "done"; result: R } | { type: "failed"; issue: string } | { type: "reset" };

function reducer<P, R>(state: State<P, R>, action: Action<P, R>): State<P, R> {
  switch (action.type) {
    case "previewing": return { state: "previewing", preview: null, result: null, issue: null };
    case "reviewed": return { ...state, state: "reviewed", preview: action.preview };
    case "applying": return { ...state, state: "applying", issue: null };
    case "done": return { ...state, state: "done", result: action.result };
    case "failed": return { ...state, state: "failed", issue: action.issue };
    case "reset": return { state: "idle", preview: null, result: null, issue: null };
  }
}

export function useReviewFlow<P, R>(steps: { preview(signal: AbortSignal): Promise<P>; apply(preview: P): Promise<R>; discard?(preview: P): Promise<void> }) {
  const [current, dispatch] = useReducer(reducer<P, R>, { state: "idle", preview: null, result: null, issue: null });
  const stepsRef = useRef(steps);
  stepsRef.current = steps;
  const pending = useRef<P | null>(null);
  useEffect(() => () => { if (pending.current !== null) void stepsRef.current.discard?.(pending.current); }, []);

  const start = useCallback(async () => {
    dispatch({ type: "previewing" });
    try {
      const preview = await stepsRef.current.preview(new AbortController().signal);
      pending.current = preview;
      dispatch({ type: "reviewed", preview });
    } catch (error) { dispatch({ type: "failed", issue: messageOf(error) }); }
  }, []);

  const confirm = useCallback(async () => {
    const preview = pending.current;
    if (preview === null) return;
    dispatch({ type: "applying" });
    try {
      const result = await stepsRef.current.apply(preview);
      pending.current = null;
      dispatch({ type: "done", result });
    } catch (error) { dispatch({ type: "failed", issue: messageOf(error) }); }
  }, []);

  const reset = useCallback(async () => {
    const preview = pending.current;
    pending.current = null;
    if (preview !== null) await stepsRef.current.discard?.(preview);
    dispatch({ type: "reset" });
  }, []);

  return { ...current, start, confirm, reset };
}
```

`src/index.ts`: 세 hook과 `messageOf`를 export.

- [ ] **Step 5: 통과 확인·커밋** — Run: `pnpm --filter @devbox/hooks exec vitest run && pnpm --filter @devbox/hooks exec tsc --noEmit` → PASS. `git add -A && git commit -m "feat(frontend): add shared polling, operation and review hooks"`

---

### Task 2: 폴링 19곳

- [ ] `rg -n "setInterval\(" apps packages --glob '!**/*.test.*'`의 모든 곳을 `usePolling`으로 바꾼다. 규칙: 간격은 그대로, `active`에는 그 화면이 이미 쓰던 활성 조건(route 활성·패널 열림 등)을 넘긴다. 한 컴포넌트에 두 개 있으면 두 번 호출한다. 기존 `clearInterval` 정리 코드는 지운다. 해당 패키지 `package.json`에 `"@devbox/hooks": "workspace:*"`를 추가한다.
- [ ] 각 화면의 기존 테스트가 `vi.advanceTimersByTime`에 기대던 곳은 "처음 한 번 즉시 실행" 때문에 호출 횟수가 1 늘 수 있다. 테스트 기대값을 새 동작(즉시 1회)에 맞게 고치고 PR 본문에 목록을 적는다.
- [ ] Run: `pnpm -r --workspace-concurrency 2 --filter './packages/*' --filter './apps/*' exec vitest run` → PASS. `! rg -n "setInterval\(" apps packages --glob '!**/*.test.*' --glob '!packages/hooks/**'` → 0건. 커밋: `git commit -am "refactor(frontend): poll through usePolling and pause while hidden"`

---

### Task 3: 대형 컴포넌트 분리

**Files:** `packages/api-studio-features/src/requests/App.tsx`(2,597줄), `packages/workspace-features/src/{terminal,files,tasks,overview}/App.tsx`(각 2,000줄 이상), `packages/knowledge-features/src/activity/App.tsx`(1,789줄), `apps/devbox-workspace/src/Workspace.tsx`; Create `.github/scripts/check-component-size.mjs`

- [ ] **Step 1: 크기 검사(실패 확인)** — `.github/scripts/check-component-size.mjs`

```js
#!/usr/bin/env node
// Components above LIMIT lines must shrink over time; the allowlist records
// today's size and may only go down.
import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";

const LIMIT = 1000;
const allow = JSON.parse(readFileSync(new URL("./component-size-allowlist.json", import.meta.url), "utf8"));
const files = execFileSync("git", ["ls-files", "*.tsx"], { encoding: "utf8" }).split("\n").filter((file) => file && !file.includes(".test."));
const problems = [];
for (const file of files) {
  const lines = readFileSync(file, "utf8").split("\n").length;
  const ceiling = allow[file] ?? LIMIT;
  if (lines > ceiling) problems.push(`${file}: ${lines} lines (limit ${ceiling})`);
}
for (const [file, ceiling] of Object.entries(allow)) {
  if (ceiling <= LIMIT) problems.push(`${file}: allowlist entry no longer needed`);
}
if (problems.length) { console.error(problems.join("\n")); process.exit(1); }
console.log("component sizes OK");
```

  `.github/scripts/component-size-allowlist.json`은 비워 둔 채(`{}`) 실행해 실패 목록을 확인한다(Step 3 목표치 확인용).

- [ ] **Step 2: 분리 규칙** — 각 대형 파일에서
  - 화면 절(최상위 `<section aria-label=…>`·패널·대화상자) 하나를 `components/<이름>.tsx`로 옮긴다. 옮긴 컴포넌트는 필요한 값을 props로 받고, 공유 state는 App이나 `hooks/use<기능>.ts` 하나에만 둔다.
  - `setBusy(true) … finally setBusy(false)` 블록은 `useOperation().run(...)`으로, 미리보기→적용→취소 흐름은 `useReviewFlow`로 바꾼다(그 파일 안의 것만).
  - 옮긴 뒤 각 파일 1,000줄 이하가 목표다. 이 PR에서 1,000줄 이하로 못 줄인 파일은 allowlist에 **줄인 뒤 줄 수**를 적는다(늘리면 CI 실패).
  - `apps/devbox-workspace/src/Workspace.tsx`: 모듈 변수 `connected`를 지운다. `packages/workspace-features/src/transport.ts`의 `configureProductTransport(fn, installationId)`가 같은 installation id로 다시 불리면 아무것도 하지 않고, 다른 id면 오류를 던지게 바꾼 뒤 `NativeContent`에서 매 렌더가 아니라 `useEffect`(의존성 `description.handshake.installationId`)로 한 번 부른다.
- [ ] **Step 3: 파일마다 커밋** — 한 파일을 나눌 때마다 그 패키지 테스트를 돌리고 커밋한다: `pnpm --filter <패키지> exec vitest run && pnpm --filter <패키지> exec tsc --noEmit && git commit -am "refactor(<scope>): split <파일> into feature components"`.
- [ ] **Step 4: 검사 연결** — `ci.yml` Frontend job의 Biome step 다음에 `node .github/scripts/check-component-size.mjs`를 추가한다. Run: `node .github/scripts/check-component-size.mjs` → PASS. 커밋: `git add -A && git commit -m "ci(frontend): cap component size"`

---

### Task 4: suppression 정리

- [ ] `rg -n "review in P1-15" apps packages`로 P1-07이 넣은 suppression을 찾는다. 이 PR에서 손댄 파일의 것은 의존성 배열을 바로잡고 지운다(바로잡으면 effect가 더 자주 도는 곳은 `useCallback`/`useRef`로 안정화). 동작이 바뀌어 지울 수 없는 것은 사유를 "`<값>`이 바뀌어도 다시 실행하면 안 됨: <이유>"로 구체화한다. 남은 개수를 PR 본문에 적는다.
- [ ] Run: `pnpm exec biome ci . && pnpm -r --workspace-concurrency 2 exec vitest run` → PASS. 커밋: `git commit -am "refactor(frontend): resolve hook dependency suppressions in split views"`

---

### Task 5: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): Activity·Logs·Tasks를 연 채 창을 최소화하고 1분 뒤 복원 → 즉시 최신 상태. 작업 관리자에서 최소화 중 CPU가 거의 0.
