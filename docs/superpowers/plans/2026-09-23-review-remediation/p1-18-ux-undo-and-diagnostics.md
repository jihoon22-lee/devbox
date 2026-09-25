# P1-18 확인 단계 줄이기·되돌리기·진단 복사 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** 되돌릴 수 있는 작업은 미리보기 단계 없이 바로 실행하고 "되돌리기"를 제공하며, 위험하거나 외부에 영향을 주는 작업만 확인을 유지한다(D13의 UX 부분, 리뷰 §7 승인 피로). 오류가 나면 "진단 복사"로 코드·component·method·요청 ID·버전을 한 번에 복사할 수 있게 한다(리뷰 §7 오류 문구).

**Architecture:** product-shell에 공용 `UndoToast`/`useUndo`(8초 동안 되돌리기 버튼, 한 번에 하나)와 `recentIssues` 기록(transport가 던지는 `ProductIssueError`를 최근 20개까지 메모리에 보관)을 둔다. "작업 상태" 패널에 "최근 오류" 목록과 "진단 복사"를 더한다(기능 화면마다 버튼을 넣지 않고 한곳에서). 바로 실행으로 바꾸는 흐름은 네 개다: 빠른 기록, 템플릿으로 노트 만들기, 일일 기록 만들기, 새 프로젝트 등록(판단이 필요 없는 경우). 되돌리기는 native가 "만든 뒤 바뀌지 않았을 때만" 지우는 조건부 삭제로 한다.

**Tech Stack:** React 19, TypeScript, Rust(Knowledge·Workspace host), Vitest

**Spec:** `review.md` §7 · `00-roadmap.md` D13(UX 부분만; 네이티브 확인 대화상자는 ADR 0016에 따라 하지 않음)

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 확인을 유지하는 흐름(바꾸지 않음): 프로젝트 정의 신뢰(`preview_trust`→`approve_trust`), 정의 편집 diff(`preview_edit`→`apply_edit`), 노트 이름 바꾸기(링크 갱신, `preview_rename`), 노트 폴더 변경, worktree 만들기, 다른 제품에서 온 초안·텍스트·모의 응답 받기, 개발 세션 시작, 런타임 제어·정리 범위, 개발 환경 설정 적용, 업데이트·복원, 판단이 필요한 프로젝트 등록(별칭·이동·교체된 루트·연결된 worktree).
- 되돌리기는 "작업이 만든 대상이 그 뒤로 바뀌지 않았을 때만" 한다. 바뀌었으면 되돌리지 않고 "이미 수정되어 되돌릴 수 없습니다"를 보인다.
- 진단 복사 내용(이 외 없음): `product`, `version`, `component`, `method`, `code`, `requestId`, `occurredAt`(ISO). 인자·값·경로·문장은 넣지 않는다(P0-07과 같은 원칙).

## Review Focus

1. 빠른 기록 저장 직후 "되돌리기" → 노트가 지워진다. 그 사이 노트를 고쳤다면 지우지 않고 안내한다. (Task 2 테스트)
2. 되돌리기 토스트가 떠 있는 동안 두 번째 작업 → 앞 토스트는 사라지고(되돌리기 기회 종료) 새 토스트만 남는다. (Task 1)
3. 새 프로젝트 등록에서 경로가 이미 등록된 프로젝트의 별칭·이동으로 판정되면 지금처럼 미리보기에서 선택을 받는다(바로 실행하지 않음). (Task 3)
4. 오류가 20개를 넘으면 오래된 것부터 버린다. 진단 복사 텍스트에 경로·인자가 없다. (Task 1)
5. 키보드만으로 되돌리기 버튼에 닿고(토스트는 `role="status"`, 버튼 포커스 가능), 8초 뒤 사라져도 포커스가 문서 흐름으로 돌아간다. (Task 1, axe)

## Branch · PR

- 묶음: **B8** — 브랜치 `refactor/suite/hooks-streaming-store-undo`, PR 제목 `refactor(suite): shared hooks, terminal streaming, native API Studio store, undo and agent protocol`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(suite): run reversible actions immediately with undo and copy diagnostics`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: 공용 되돌리기·최근 오류

**Files:** Create `packages/product-shell/src/{undo.tsx,undo.test.tsx,issues.ts,issues.test.ts}`; Modify `packages/product-shell/src/{index.tsx,Operations.tsx,package.json exports}`, 네 제품 `transport.ts`

**Interfaces (Produces):**
- `class ProductIssueError extends Error { code; product; component; method; requestId; occurredAt }`
- `recordIssue(error: ProductIssueError): void`, `recentIssues(): readonly ProductIssueError[]`(최대 20), `diagnosticText(error: ProductIssueError, version: string): string`
- `useUndo(): { offer(label: string, undo: () => Promise<void>): void; toast: ReactNode }`, `<UndoToast/>`

- [ ] **Step 1: 실패하는 테스트** — `issues.test.ts`

```ts
import { afterEach, describe, expect, it } from "vitest";
import { diagnosticText, ProductIssueError, recentIssues, recordIssue, resetIssues } from "./issues";

afterEach(() => resetIssues());

describe("recent issues", () => {
  it("keeps the newest 20 and copies only identifiers", () => {
    for (let index = 0; index < 25; index++) {
      recordIssue(new ProductIssueError("저장하지 못했습니다. C:\\Users\\me\\a.md", { code: "note_conflict", product: "knowledge", component: "knowledge.notes", method: "write_file", requestId: `r-${index}` }));
    }
    expect(recentIssues()).toHaveLength(20);
    expect(recentIssues()[0].requestId).toBe("r-24");
    const text = diagnosticText(recentIssues()[0], "0.9.0");
    expect(JSON.parse(text)).toEqual({ product: "knowledge", version: "0.9.0", component: "knowledge.notes", method: "write_file", code: "note_conflict", requestId: "r-24", occurredAt: expect.any(String) });
    expect(text).not.toContain("Users");
  });
});
```

`undo.test.tsx`

```tsx
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { useUndo } from "./undo";

function Harness({ undo }: { undo: () => Promise<void> }) {
  const { offer, toast } = useUndo();
  return <><button onClick={() => offer("노트를 만들었습니다.", undo)}>만들기</button>{toast}</>;
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); vi.useRealTimers(); });

describe("useUndo", () => {
  it("offers undo for eight seconds", async () => {
    vi.useRealTimers(); // axe schedules its own timers
    const undo = vi.fn(async () => {});
    const { container } = render(<Harness undo={undo} />);
    fireEvent.click(screen.getByText("만들기"));
    expect(screen.getByRole("status")).toHaveTextContent("노트를 만들었습니다.");
    await assertNoA11yViolations(container);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "되돌리기" })); });
    expect(undo).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "되돌리기" })).toBeNull();
  });

  it("expires and a newer action replaces the older offer", async () => {
    const first = vi.fn(async () => {});
    const { rerender } = render(<Harness undo={first} />);
    fireEvent.click(screen.getByText("만들기"));
    const second = vi.fn(async () => {});
    rerender(<Harness undo={second} />);
    fireEvent.click(screen.getByText("만들기"));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "되돌리기" })); });
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByText("만들기"));
    act(() => { vi.advanceTimersByTime(8_000); });
    expect(screen.queryByRole("button", { name: "되돌리기" })).toBeNull();
  });
});
```

- [ ] **Step 2: 실패 확인** — Run: `pnpm --filter @devbox/product-shell exec vitest run src/issues.test.ts src/undo.test.tsx` → FAIL.
- [ ] **Step 3: 구현**
  - `issues.ts`: `ProductIssueError`(생성자 `(message, fields)`, `name`은 `code`로 둬 기존 `error.name` 검사와 호환), `recordIssue`(앞에 넣고 20개로 자름), `recentIssues`, `resetIssues`(테스트용), `diagnosticText`(위 7개 필드만 `JSON.stringify(…, null, 2)`).
  - `undo.tsx`: `useUndo`는 `{label, undo, id}` 하나만 state로 들고, `offer`가 새 offer로 바꾸며 8초 `setTimeout`으로 지운다. 토스트는 `<div role="status" className="shell-undo">{label} <button type="button" onClick={…}>되돌리기</button></div>`. 되돌리기 실패 시 문구를 `"되돌리지 못했습니다: " + messageOf(error)`로 바꿔 3초 보인다.
  - `Operations.tsx`: 패널에 "최근 오류" 목록(`recentIssues()`; 코드·component·method·시각)과 항목별 "진단 복사" 버튼(`navigator.clipboard.writeText(diagnosticText(issue, description.product.version ?? ""))`, 제품 버전은 describe의 제품 정보나 `import.meta.env`에서 얻는다)을 추가한다.
  - 네 제품 `transport.ts`: 실패 응답에서 던지던 오류를 `ProductIssueError`로 만들고(`code`=issue, `component`, `method`, `requestId`=header.requestId, `product`) `recordIssue`를 부른다. 기존 메시지 조회(`issueError`, `componentFailure`, `issueMessage`)는 메시지로 그대로 쓴다.
  - `package.json`의 `exports`에 `./undo`, `./issues`를 추가한다.
- [ ] **Step 4: 통과 확인·커밋** — Run: `pnpm --filter @devbox/product-shell exec vitest run && pnpm -r --workspace-concurrency 2 exec tsc --noEmit` → PASS. `git add -A && git commit -m "feat(suite): add undo offers and a recent-issue diagnostic copy"`

---

### Task 2: Knowledge — 빠른 기록·템플릿 노트·일일 기록

**Files:** `crates/knowledge-vault-engine/src/{api.rs,commands/docs.rs,commands/daily.rs}`, `packages/knowledge-features/src/notes/{QuickCapture*.tsx,App.tsx,api.ts}`, `packages/knowledge-features/src/daily/*`(또는 `apps/devbox-knowledge/src/Daily.tsx`)

**Interfaces (Produces):** engine 메서드 `capture_note { input } -> CreatedNote { path, revision }`, `create_note_from_template { template_id, path } -> CreatedNote`, `open_daily { date } -> DailyOpened { path, created, revision }`, `undo_created_note { path, revision } -> UndoResult { removed: bool }`(파일의 현재 revision이 같을 때만 삭제, 다르면 `removed: false`)

- [ ] **Step 1: 실패하는 테스트** — `commands/docs.rs` 테스트 모듈(기존 vault fixture 도우미 사용)

```rust
    #[test]
    fn undo_removes_a_created_note_only_while_unchanged() {
        let fixture = VaultFixture::new(); // 기존 테스트의 vault 준비 도우미 이름으로 바꾼다
        let created = capture_note_inner(&fixture.state(), capture_input("첫 기록", "내용")).unwrap();
        assert!(undo_created_note_inner(&fixture.state(), &created.path, &created.revision).unwrap().removed);
        assert!(!fixture.root().join(&created.path).exists());

        let created = capture_note_inner(&fixture.state(), capture_input("둘째 기록", "내용")).unwrap();
        std::fs::write(fixture.root().join(&created.path), "사용자가 고친 내용").unwrap();
        assert!(!undo_created_note_inner(&fixture.state(), &created.path, &created.revision).unwrap().removed);
        assert!(fixture.root().join(&created.path).exists());
    }
```

  (`revision`은 기존 조건부 저장이 쓰는 내용 해시와 같은 값이다. `capture_note_inner`는 기존 preview+save 경로를 한 함수로 합친 것이다.)

  프런트 `QuickCapture` 테스트(기존 파일)에 추가: 저장 버튼 한 번으로 `capture_note`가 불리고 미리보기 화면이 나오지 않으며, 토스트의 되돌리기가 `undo_created_note`를 부른다.

- [ ] **Step 2: 구현**
  - `capture_note`: 기존 `preview_quick_capture` 검증(`CaptureError` 코드) → 저장을 한 번에. 결과에 root 상대 path와 revision.
  - `create_note_from_template`: 기존 `preview_template` + `save_template`을 한 번에(대상 path가 이미 있으면 `template_target_exists`로 거부 — 덮어쓰지 않음).
  - `open_daily`: 날짜 노트가 있으면 그대로 열기(`created: false`), 없으면 템플릿으로 만들고 `created: true`. 기존 `preview_daily`/`save_daily`/`discard_daily`는 지운다(호출부 교체 후).
  - `undo_created_note`: 파일의 현재 내용 해시가 `revision`과 같을 때만 삭제하고 인덱스를 갱신한다(기존 `delete_file` 내부 경로 재사용). 다르면 `removed: false`.
  - 기존 `preview_quick_capture`/`save_quick_capture`/`discard_quick_capture_preview`는 전역 빠른 기록 창(단축키)도 같은 흐름으로 바꾼 뒤 지운다. 문구: 저장 뒤 토스트 "노트를 만들었습니다." + 되돌리기, 되돌리기 결과 `removed: false`면 "이미 수정되어 되돌리지 않았습니다."
  - P1-12의 enum·생성 타입·카탈로그를 함께 고친다(`check-generated-bindings.sh`).
- [ ] **Step 3: 통과 확인·커밋** — Run: `cargo test -p devbox-knowledge-vault-engine --lib && bash .github/scripts/check-generated-bindings.sh && pnpm --filter @devbox/knowledge-features --filter devbox-knowledge exec vitest run` → PASS. `git add -A && git commit -m "feat(devbox-knowledge): create notes immediately with undo"`

---

### Task 3: Workspace — 판단이 필요 없는 새 프로젝트 등록

**Files:** `apps/devbox-workspace/src/RegistryGate.tsx`와 테스트, 필요하면 `apps/devbox-workspace/src-tauri/src/project_owner.rs`

- [ ] **Step 1: 실패하는 테스트** — `RegistryGate.test.tsx`

```tsx
it("registers a new project in one step and offers undo", async () => {
  native.registryCall.mockImplementation(async (method: string) => {
    if (method === "snapshot") return registryWith([]);
    if (method === "preview_windows") return { previewId: "p", discovery: { kind: "newProject" } };
    if (method === "apply_registration") return { projectId: "proj-1", revision: 4 };
    if (method === "remove") return {};
    throw new Error(method);
  });
  render(<RegistryGate context={null} onContextChanged={async () => {}} onReady={() => {}} editing={false} />);
  fireEvent.change(await screen.findByLabelText("프로젝트 경로"), { target: { value: "C:\\src\\app" } });
  fireEvent.click(screen.getByRole("button", { name: "등록" }));
  expect(await screen.findByRole("status")).toHaveTextContent("프로젝트를 등록했습니다.");
  expect(native.registryCall).toHaveBeenCalledWith("apply_registration", expect.objectContaining({ previewId: "p" }));
  fireEvent.click(screen.getByRole("button", { name: "되돌리기" }));
  await waitFor(() => expect(native.registryCall).toHaveBeenCalledWith("remove", expect.objectContaining({ id: "proj-1", revision: 4 })));
});

it("keeps the review step when the path matches an existing project", async () => {
  native.registryCall.mockImplementation(async (method: string) => {
    if (method === "snapshot") return registryWith([existingProject()]);
    if (method === "preview_windows") return { previewId: "p", discovery: { kind: "aliasOrMove" } };
    throw new Error(method);
  });
  render(<RegistryGate context={null} onContextChanged={async () => {}} onReady={() => {}} editing={false} />);
  fireEvent.change(await screen.findByLabelText("프로젝트 경로"), { target: { value: "D:\\src\\app" } });
  fireEvent.click(screen.getByRole("button", { name: "등록" }));
  expect(await screen.findByText(/이미 등록된 프로젝트/)).toBeInTheDocument();
  expect(native.registryCall).not.toHaveBeenCalledWith("apply_registration", expect.anything());
});
```

  (`registryWith`·`existingProject`는 테스트 파일 안의 fixture 도우미. 입력 label·버튼 이름·discovery 필드 이름은 실제 `RegistryGate`와 `project_owner.rs`의 `Discovery` 직렬화(`kind`: `known`/`aliasOrMove`/`replacedRoot`/`linkedWorktree`/`newProject`)에 맞춘다. `remove`의 인자 이름도 실제 메서드를 따른다.)

- [ ] **Step 2: 구현** — 등록 버튼: `preview_*` 결과의 `discovery.kind === "newProject"`이고 템플릿 선택이 없으면 곧바로 `apply_registration(previewId)`를 부르고 `offer("프로젝트를 등록했습니다.", () => registryCall("remove", {id, revision}))`. 그 밖의 판정은 지금 미리보기 UI를 그대로 보인다. native `remove`가 revision 비교(내용이 바뀌면 거부)를 하지 않으면 `project_owner.rs`의 `remove`에 `expected_revision`을 받게 하고 테스트를 추가한다(다른 변경 뒤에는 되돌리기가 `stale_registry`로 거부된다).
- [ ] **Step 3: 통과 확인·커밋** — Run: `cargo test -p devbox-workspace --lib project_owner && pnpm --filter devbox-workspace exec vitest run src/RegistryGate.test.tsx` → PASS. `git add -A && git commit -m "feat(devbox-workspace): register new projects in one step with undo"`

---

### Task 4: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문에 "확인을 유지한 흐름과 이유" 표(Global Constraints 목록)를 넣는다.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): 전역 단축키 빠른 기록 → 저장 → 되돌리기, 템플릿으로 노트 만들기 → 되돌리기, 오늘 기록 열기, 새 폴더를 Workspace 프로젝트로 한 번에 등록 → 되돌리기, 일부러 오류(없는 경로)를 낸 뒤 "작업 상태 › 최근 오류 › 진단 복사" 내용 확인.
