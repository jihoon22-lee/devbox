# P0-03 Knowledge 오류 코드·노트 자동 저장·복구 저널 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** (1) Knowledge가 오류를 한국어 문장 일부로 분류하던 것을 코드로 바꿔 사용자에게 정확한 문구가 보이게 한다(B6). (2) 노트 편집을 자동 저장한다(D12). (3) 저장되기 전 편집 내용을 복구 저널에 남겨 강제 종료·저장 실패 뒤 복원할 수 있게 한다.

**Architecture:**
- 엔진(`knowledge-vault-engine`)이 빠른 캡처·미리보기 만료/오래됨 오류를 안정 코드로 돌려준다. host의 `issue()`는 코드만 매핑하고, 프런트는 코드를 기존 한국어 문구로 바꾼다.
- 자동 저장은 `NoteAutosave`(편집이 1.5초 멈추면 `NoteDocument.save()`)와 `NoteDocument.setBeforeSwitch`(다른 노트를 열기 전에 먼저 저장)로 구현한다. 기존 조건부 저장(native revision)과 충돌 처리를 그대로 쓰므로 외부 변경을 덮어쓰지 않는다.
- 복구 저널은 Notes 제품 데이터 폴더의 평문 JSON(`note-journal.json`)이다. 개인용 기준에 따라 암호화하지 않는다(Workspace의 `recovery.json`과 같은 수준). 편집 1초 후 기록하고 저장이 끝나면 해당 폴더·문서의 항목만 지운다. native가 확인한 canonical root로 항목을 구분하고, 루트 캐시로 연결 장애 중에도 이미 열린 문서의 로컬 백업을 유지한다.

**Tech Stack:** Rust, React 19 + TypeScript, Vitest(fake timers)

**Spec:** `review.md` §3 B6, §7 자동 저장 · `00-roadmap.md` D12(보안 조정으로 저널 암호화 제외) · 2026-09-25 사용자 합의(ledger #580): 폴더별 저널·자동 삭제 없음·native 루트 캐시·활성화 경계에서 목록 조회

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 자동 저장 기본값: 켜짐, 지연 1500ms. 설정은 `localStorage` 키 `devbox.knowledge.notes.autosave`(`"off"`면 꺼짐).
- 저널: 모든 폴더 합계 최대 8개, 노트당 4 MiB, 상대 경로 1024자. 파일은 기존 Notes engine `dir` 아래 `note-journal.json`이며 사용자 vault에 새 파일을 만들지 않는다.
- `JournalEntry.vaultRoot`는 `VaultIdentity::inspect(root).canonical_path()`의 문자열이다. 저장 키는 `(vaultRoot, path)`이며 폴더 파일 ID는 저장하지 않는다. 공개 전 형식이므로 schemaVersion=1을 유지하고 변환 코드는 만들지 않는다. 같은 경로의 폴더 교체는 기존 baseRevision 비교로 자동 적용을 막는다.
- 전체 8개일 때 기존 키 갱신은 허용하고 새 키는 `journal_limit`으로 거부한다. 오래되었거나 다른 폴더에 속한다는 이유로 복구본을 자동 삭제하지 않는다. 삭제는 저장 성공 후 해당 문서 정리 또는 사용자 확인을 거친 명시적 버리기로 제한한다.
- `save_note_journal`·`clear_note_journal` 입력은 그대로 두고 native가 현재 루트를 결정한다. load는 `{entries, otherVaultCount}`이며 현재 폴더만 투영한다. `discard_other_vault_journal {}`를 추가한다. 절대 경로인 vaultRoot는 저장 전용이며 IPC 응답·로그·오류에는 넣지 않는다.
- 루트는 성공한 native 확인 결과를 캐시한다. 이미 열린 문서의 저널 저장마다 원격 파일시스템을 다시 확인하지 않는다. 확인한 루트가 없으면 실패를 알리고 기존 저널을 보존한다.
- 폴더 변경은 현재의 다음 시작/활성화 계약을 유지한다. 복구 목록은 활성화 뒤 Notes 첫 mount에서 읽고, 일반 트리·watcher 갱신에는 초기화하지 않는다. 저널 작업은 직렬화하며 늦은 조회·복구 적용은 현재 요청/문서에 속하는지 확인한다.
- 새 의존성·폴더 내부 식별 파일·다중 저장소 관리 기능을 추가하지 않는다. Task 1–3의 오류 코드와 Task 6의 자동 저장 지연/설정 정책은 유지한다.
- 오류 코드 이름은 이 문서의 표 그대로 쓴다(host·프런트가 같은 이름을 공유).

| 코드 | 사용자 문구 |
|---|---|
| `quick_capture_sensitive` | 민감한 정보가 포함되어 있어 저장하지 않았습니다 |
| `quick_capture_body_required` | 빠른 캡처 본문을 입력하세요 |
| `quick_capture_invalid` | 빠른 캡처 입력이 올바르지 않습니다 |
| `quick_capture_title_limit` | 제목은 UTF-8 800바이트·200자 이내로 입력하세요 |
| `quick_capture_body_limit` | 본문은 LF 기준 64 KiB(원문 128 KiB) 이내로 입력하세요 |
| `quick_capture_tag_count` | 태그는 최대 20개까지 입력하세요 |
| `quick_capture_tag_limit` | 태그 하나는 UTF-8 192바이트·48자 이내로 입력하세요 |
| `quick_capture_tags_limit` | 태그 전체는 UTF-8 1 KiB 이내로 입력하세요 |
| `quick_capture_tag_invalid` | 태그에 줄바꿈·쉼표·대괄호·따옴표를 사용할 수 없습니다 |
| `quick_capture_save_failed` | 빠른 캡처를 저장하지 못했습니다 |
| `preview_stale` | (기존) 미리보기가 오래되었습니다. 다시 확인해 주세요. |
| `preview_expired` | (기존) 미리보기가 만료되었습니다. 새로 준비해 주세요. |
| `journal_unavailable` | 복구용 임시 저장을 기록하지 못했습니다. 편집 내용은 유지됩니다. |
| `journal_limit` | 복구용 임시 저장이 8개로 가득 찼습니다. 남은 복구본을 복원하거나 버린 뒤 다시 편집해 주세요. |

## Review Focus

1. 자동 저장 중 디스크 파일이 외부에서 바뀜 → 덮어쓰지 않고 기존 충돌 화면이 뜨며, 충돌이 풀릴 때까지 자동 저장이 멈춘다. (Task 6)
2. 편집 직후 다른 노트를 클릭 → 확인 대화상자 없이 먼저 저장되고 전환된다. 저장이 실패하면 기존처럼 버릴지 묻는다. (Task 6)
3. 강제 종료 후 A→B→A 폴더 활성화 → 현재 폴더 항목만 복구하고 다른 폴더 항목은 보존·개수 안내한다. 비교 중 문서가 바뀌거나 이전 조회가 늦게 끝나도 현재 상태를 덮지 않는다. 같은 폴더의 디스크 변경은 기존 revision 비교를 따른다. (Task 4·7)
4. 저널이 가득 참·연결 장애·손상·비동기 기록/삭제 경합 → 기존 복구본을 자동 삭제하지 않는다. 캐시가 있으면 연결 장애 중에도 로컬 기록하고, 손상 JSON은 보존 후 재시작한다. 늦은 clear/record가 새 초안을 지우거나 이전 초안을 되살리지 않는다. (Task 4·5)
5. 제품 모드에서 빠른 캡처 "민감한 정보" 오류 → 일반 오류가 아니라 정확한 문구가 보인다. (Task 3)

## Branch · PR

- 묶음: **B1** — 브랜치 `fix/suite/bootstrap-privacy-autosave-connection`, PR 제목 `fix(suite): pin toolchains and fix activity privacy, Knowledge autosave and Suite connections`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(devbox-knowledge): autosave notes, keep a recovery journal and return stable error codes`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

## File Structure

| 파일 | 변경 | 책임 |
|---|---|---|
| `crates/knowledge-vault-engine/src/core/capture.rs` | 수정 | `CaptureError::code()` |
| `crates/knowledge-vault-engine/src/commands/docs.rs` | 수정 | 캡처 오류 코드, 성공한 문서 열기에서 native 루트 캐시 갱신 |
| `crates/knowledge-vault-engine/src/core/vault.rs:15` | 수정 | `STALE_ROOT` = `preview_stale` |
| `crates/knowledge-vault-engine/src/commands/templates.rs` | 수정 | 템플릿 미리보기 오래됨 = `preview_stale` |
| `crates/knowledge-vault-engine/src/core/rename.rs:75` | 수정 | `preview_expired` |
| `crates/knowledge-vault-engine/src/core/journal.rs` | 생성 | 폴더·문서 복합 키, 전체 한도, view·명시적 정리 |
| `crates/knowledge-vault-engine/src/core/mod.rs` | 수정 | `pub mod journal;` |
| `crates/knowledge-vault-engine/src/commands/journal.rs` | 생성 | 로컬 저장소·루트 캐시·명령 4개·현재 폴더 응답 투영 |
| `crates/knowledge-vault-engine/src/commands/mod.rs` | 수정 | `pub mod journal;` |
| `crates/knowledge-vault-engine/src/component.rs` | 수정 | COMMANDS·dispatch·`manage` |
| `apps/devbox-knowledge/src-tauri/src/component.rs` | 수정 | `issue()` 코드 매핑, substring 분기 삭제 |
| `apps/devbox-knowledge/src/issues.ts` | 수정 | 문구 추가 |
| `apps/devbox-knowledge/README.md` | 수정 | 자동 저장·저널 설명 |
| `packages/knowledge-features/src/notes/api.ts` | 수정 | 캡처 오류 변환, JournalView·저널 API 4개 |
| `packages/knowledge-features/src/notes/api.journal.test.ts` | 생성 | 저널 IPC 요청·응답 계약 |
| `packages/knowledge-features/src/notes/api.quickCapture.test.ts` | 수정 | 코드 기반 테스트 |
| `packages/knowledge-features/src/notes/noteDocument.ts` | 수정 | `setBeforeSwitch` |
| `packages/knowledge-features/src/notes/noteDocument.test.ts` | 수정 | 전환 전 저장 테스트 |
| `packages/knowledge-features/src/notes/timers.ts` | 생성 | 테스트 가능한 타이머 |
| `packages/knowledge-features/src/notes/autosave.ts`, `autosave.test.ts` | 생성 | 자동 저장 |
| `packages/knowledge-features/src/notes/journal.ts`, `journal.test.ts` | 생성 | 저널 기록기 |
| `packages/knowledge-features/src/notes/components/RecoveryBanner.tsx`, `.test.tsx` | 생성 | 복구 배너 |
| `packages/knowledge-features/src/notes/App.tsx` | 수정 | 활성화 시 조회, 명시적 삭제, 복구 대상·요청 세대 확인 |
| `packages/knowledge-features/src/notes/App.journal.test.tsx` | 생성 | 폴더 활성화·삭제·비교 소유권·늦은 응답 회귀 |

---

### Task 1: 엔진 오류 코드

**Interfaces (Produces):** `CaptureError::code(self) -> &'static str` (위 표의 `quick_capture_*` 9개)

- [ ] **Step 1: 실패하는 테스트로 기존 단언 교체**
  - `crates/knowledge-vault-engine/src/commands/docs.rs:1504` `assert_eq!(error, "민감한 정보가 포함되어 있어 저장하지 않았습니다");` → `assert_eq!(error, "quick_capture_sensitive");`
  - `docs.rs:1559` `assert_eq!(error, "빠른 캡처 미리보기가 오래되어 다시 확인하세요");` → `assert_eq!(error, "preview_stale");`
  - `rg -n "빠른 캡처 미리보기가 오래되어|템플릿 미리보기가 오래되어|이름 변경 미리보기가 만료|빠른 캡처를 저장하지 못했습니다|빠른 캡처 본문을 입력하세요" crates/knowledge-vault-engine/src` 결과 중 테스트 단언은 모두 위 표의 코드로 바꾼다.
  - `capture.rs` 테스트 모듈에 추가:

```rust
    #[test]
    fn every_capture_error_has_a_stable_code() {
        use CaptureError::*;
        let codes: Vec<_> = [EmptyBody, InvalidText, TitleTooLong, BodyTooLarge, TooManyTags, TagTooLong, TagsTooLarge, InvalidTag, SensitiveContent]
            .into_iter()
            .map(CaptureError::code)
            .collect();
        assert_eq!(codes, [
            "quick_capture_body_required", "quick_capture_invalid", "quick_capture_title_limit",
            "quick_capture_body_limit", "quick_capture_tag_count", "quick_capture_tag_limit",
            "quick_capture_tags_limit", "quick_capture_tag_invalid", "quick_capture_sensitive",
        ]);
    }
```

- [ ] **Step 2: 실패 확인** — Run: `source ~/.cargo/env && cargo test -p devbox-knowledge-vault-engine --lib` → FAIL.

- [ ] **Step 3: 구현**
  - `capture.rs`의 `impl CaptureError` 안, `message()` 위에 추가:

```rust
    /// Stable code returned to the product host; the UI maps it to a message.
    pub const fn code(self) -> &'static str {
        match self {
            Self::SensitiveContent => "quick_capture_sensitive",
            Self::EmptyBody => "quick_capture_body_required",
            Self::InvalidText => "quick_capture_invalid",
            Self::TitleTooLong => "quick_capture_title_limit",
            Self::BodyTooLarge => "quick_capture_body_limit",
            Self::TooManyTags => "quick_capture_tag_count",
            Self::TagTooLong => "quick_capture_tag_limit",
            Self::TagsTooLarge => "quick_capture_tags_limit",
            Self::InvalidTag => "quick_capture_tag_invalid",
        }
    }
```

  - `docs.rs`: `capture::normalize(input).map_err(|error| error.to_string())` (494·521·704행)와 `capture::render_markdown(&normalized).map_err(|error| error.to_string())` (714행)의 `error.to_string()`을 `error.code().to_string()`으로 바꾼다. `render_markdown`의 오류 타입이 `CaptureError`가 아니면 그 줄은 `"quick_capture_save_failed".to_string()`으로 바꾼다.
  - `docs.rs`의 문자열 리터럴 교체:
    - `"빠른 캡처 미리보기가 오래되어 다시 확인하세요"` → `"preview_stale"` (전부)
    - `"빠른 캡처를 저장하지 못했습니다"` → `"quick_capture_save_failed"` (전부)
  - `core/vault.rs:15` `const STALE_ROOT: &str = "빠른 캡처 미리보기가 오래되어 다시 확인하세요";` → `const STALE_ROOT: &str = "preview_stale";`
  - `commands/templates.rs`의 `"템플릿 미리보기가 오래되어 다시 확인하세요"` → `"preview_stale"` (전부)
  - `core/rename.rs:75` `Err("이름 변경 미리보기가 만료되었습니다".to_string())` → `Err("preview_expired".to_string())`

```bash
sed -i 's/"빠른 캡처 미리보기가 오래되어 다시 확인하세요"/"preview_stale"/g; s/"빠른 캡처를 저장하지 못했습니다"/"quick_capture_save_failed"/g' crates/knowledge-vault-engine/src/commands/docs.rs
sed -i 's/"템플릿 미리보기가 오래되어 다시 확인하세요"/"preview_stale"/g' crates/knowledge-vault-engine/src/commands/templates.rs
```

- [ ] **Step 4: 통과 확인** — Run: `cargo test -p devbox-knowledge-vault-engine --lib` → PASS.

- [ ] **Step 5: 커밋**

```bash
git add crates/knowledge-vault-engine
git commit -m "fix(devbox-knowledge): return stable codes for capture and stale preview errors"
```

---

### Task 2: host 매핑과 문구

- [ ] **Step 1: 실패하는 테스트** — `apps/devbox-knowledge/src-tauri/src/component.rs` 테스트 모듈(436행 부근 `issue` 단언들 옆)에 추가:

```rust
        assert_eq!(issue("quick_capture_sensitive"), "quick_capture_sensitive");
        assert_eq!(issue("quick_capture_save_failed"), "quick_capture_save_failed");
        assert_eq!(issue("preview_stale"), "preview_stale");
        assert_eq!(issue("preview_expired"), "preview_expired");
        assert_eq!(issue("journal_limit"), "journal_limit");
        // Human-readable text is never classified by substring any more.
        assert_eq!(issue("이 미리보기는 오래되었고 만료되었습니다"), "operation_failed");
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-knowledge --lib component` → FAIL.

- [ ] **Step 3: 구현** — `fn issue`에서 아래 두 줄을 삭제한다.

```rust
        _ if error.contains("만료") => "preview_expired",
        _ if error.contains("미리보기") && error.contains("오래") => "preview_stale",
```

`_ => "operation_failed",` 바로 위에 추가한다.

```rust
        "quick_capture_sensitive" => "quick_capture_sensitive",
        "quick_capture_body_required" => "quick_capture_body_required",
        "quick_capture_invalid" => "quick_capture_invalid",
        "quick_capture_title_limit" => "quick_capture_title_limit",
        "quick_capture_body_limit" => "quick_capture_body_limit",
        "quick_capture_tag_count" => "quick_capture_tag_count",
        "quick_capture_tag_limit" => "quick_capture_tag_limit",
        "quick_capture_tags_limit" => "quick_capture_tags_limit",
        "quick_capture_tag_invalid" => "quick_capture_tag_invalid",
        "quick_capture_save_failed" => "quick_capture_save_failed",
        "preview_stale" => "preview_stale",
        "preview_expired" => "preview_expired",
        "journal_unavailable" => "journal_unavailable",
        "journal_limit" => "journal_limit",
```

`apps/devbox-knowledge/src/issues.ts`의 `messages`에 위 Global Constraints 표의 `quick_capture_*` 10개와 `journal_*` 2개를 표 문구 그대로 추가한다(`preview_*`는 이미 있음).

- [ ] **Step 4: 통과 확인** — Run: `cargo test -p devbox-knowledge --lib component` → PASS.

- [ ] **Step 5: 커밋**

```bash
git add apps/devbox-knowledge
git commit -m "fix(devbox-knowledge): map note errors by code instead of message text"
```

---

### Task 3: 빠른 캡처 오류 변환(프런트)

**Interfaces (Produces):** `export const QUICK_CAPTURE_MESSAGES: Readonly<Record<string, string>>` (notes/api.ts)

- [ ] **Step 1: 실패하는 테스트** — `packages/knowledge-features/src/notes/api.quickCapture.test.ts`의 `describe` 안에 추가:

```ts
  it("turns a native capture code into its fixed message", async () => {
    invokeMock.mockRejectedValueOnce("quick_capture_sensitive");
    await expect(saveQuickCapture("qc-1")).rejects.toThrow("민감한 정보가 포함되어 있어 저장하지 않았습니다");
  });

  it("recognizes a product-transport error by its code name", async () => {
    const stale = new Error("미리보기가 오래되었습니다. 다시 확인해 주세요.");
    stale.name = "preview_stale";
    invokeMock.mockRejectedValueOnce(stale);
    await expect(saveQuickCapture("qc-1")).rejects.toThrow("빠른 캡처 미리보기가 오래되어 다시 확인하세요");
  });
```

- [ ] **Step 2: 실패 확인** — Run: `pnpm --filter @devbox/knowledge-features exec vitest run src/notes/api.quickCapture.test.ts` → 새 테스트 2개 FAIL.

- [ ] **Step 3: 구현** — `notes/api.ts`의 `function safeQuickCaptureError`(715행)를 아래로 교체한다(바로 위에 상수 추가).

```ts
/** Native quick capture codes → the same fixed messages client validation uses. */
export const QUICK_CAPTURE_MESSAGES: Readonly<Record<string, string>> = {
  quick_capture_sensitive: "민감한 정보가 포함되어 있어 저장하지 않았습니다",
  quick_capture_body_required: "빠른 캡처 본문을 입력하세요",
  quick_capture_invalid: "빠른 캡처 입력이 올바르지 않습니다",
  quick_capture_title_limit: "제목은 UTF-8 800바이트·200자 이내로 입력하세요",
  quick_capture_body_limit: "본문은 LF 기준 64 KiB(원문 128 KiB) 이내로 입력하세요",
  quick_capture_tag_count: "태그는 최대 20개까지 입력하세요",
  quick_capture_tag_limit: "태그 하나는 UTF-8 192바이트·48자 이내로 입력하세요",
  quick_capture_tags_limit: "태그 전체는 UTF-8 1 KiB 이내로 입력하세요",
  quick_capture_tag_invalid: "태그에 줄바꿈·쉼표·대괄호·따옴표를 사용할 수 없습니다",
  preview_stale: "빠른 캡처 미리보기가 오래되어 다시 확인하세요",
};
const FIXED_QUICK_CAPTURE_MESSAGES = new Set(Object.values(QUICK_CAPTURE_MESSAGES));

function safeQuickCaptureError(error: unknown, fallback: string): Error {
  // Product transport errors carry the native code in `name`; the legacy
  // runtime rejects with the code string. Client validation throws the fixed
  // message itself. Anything else is replaced by the fallback.
  const code = error instanceof Error ? error.name : typeof error === "string" ? error : "";
  if (Object.prototype.hasOwnProperty.call(QUICK_CAPTURE_MESSAGES, code)) {
    return new Error(QUICK_CAPTURE_MESSAGES[code]);
  }
  const message = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  return new Error(FIXED_QUICK_CAPTURE_MESSAGES.has(message) ? message : fallback);
}
```

`QuickCaptureDialog.tsx`는 바꾸지 않는다(변환된 문구가 기존 `SAFE_ERROR_MESSAGES`에 들어 있다).

- [ ] **Step 4: 통과 확인** — Run: `pnpm --filter @devbox/knowledge-features exec vitest run src/notes` → PASS.

- [ ] **Step 5: 커밋**

```bash
git add packages/knowledge-features/src/notes/api.ts packages/knowledge-features/src/notes/api.quickCapture.test.ts
git commit -m "fix(devbox-knowledge): show quick capture errors from native codes"
```

---

### Task 4: 복구 저널(엔진) — 노트 폴더별 보존

**Files:** Create `core/journal.rs`, `commands/journal.rs`; Modify `core/mod.rs`, `commands/mod.rs`, `component.rs`, `commands/docs.rs` (모두 `crates/knowledge-vault-engine/src/` 아래).

**Interfaces (Produces):**
- 저장 형식: `JournalFile { schemaVersion: 1, entries: JournalEntry[] }`.
- `JournalEntry { vaultRoot, path, content, baseRevision, savedAtMs }`. `vaultRoot`는 native가 `VaultIdentity::inspect(root)`로 확인한 `canonical_path()`의 문자열이다. 항목 키는 `(vaultRoot, path)`다.
- 순수 모델: `JournalFile::{decode, encode, upsert, remove(vault_root, path), view(vault_root), discard_other(vault_root)}`. `view`는 현재 폴더의 `JournalEntryView` 목록과 다른 폴더의 **항목 개수**를 반환한다.
- `JournalEntryView { path, content, baseRevision, savedAtMs }`, `JournalView { entries, otherVaultCount }`. 저장용 절대 경로 `vaultRoot`는 응답·로그·오류에 투영하지 않는다.
- native `NoteJournalStore`는 저널 파일 경로, read-modify-write용 mutex, 현재 설정 루트와 검증된 canonical root의 캐시를 관리한다. 저널에는 폴더 파일 ID를 저장하지 않는다.

| component 메서드 | 입력 | 응답 |
|---|---|---|
| `save_note_journal` | `{path, content, baseRevision}` (기존 그대로) | `null` |
| `clear_note_journal` | `{path}` (기존 그대로) | `null` |
| `load_note_journal` | `{}` | `{entries: JournalEntryView[], otherVaultCount: number}` |
| `discard_other_vault_journal` | `{}` | `null` |

오류 코드는 `journal_unavailable`, `journal_limit`만 쓴다. 공개 전 형식이므로 schemaVersion은 1로 유지하며 옛 저널 변환 코드는 만들지 않는다. 기존 v0.8.1 노트와 DB 형식은 변경하지 않는다.

- [ ] **Step 1: 실패하는 테스트 작성** — `core/journal.rs`와 `commands/journal.rs`에 아래 테스트를 먼저 작성한다. 임시 폴더·합성 본문만 사용하고 실제 WSL 배포판을 조작하지 않는다.

| 테스트 | 준비·동작 | 기대 결과 |
|---|---|---|
| `journal_separates_vaults_and_restores_the_original_view` | A의 `Notes/a.md` 기록 → B로 조회 → A로 조회 | B의 entries는 빈 배열, otherVaultCount=1. A로 돌아오면 원래 본문이 보임 |
| `journal_same_path_in_two_vaults_has_two_entries` | A·B 각각 `Notes/a.md` 기록 | 두 항목 보존, 각 view에는 자기 본문만 나옴 |
| `journal_clear_only_removes_the_current_vault_entry` | A·B 동명 항목 준비 → B에서 clear | B 항목만 삭제, A 항목은 동일하게 유지 |
| `journal_limit_preserves_all_existing_entries` | A 항목 8개 → B 새 항목 저장 | `journal_limit`, A의 가장 오래된 항목을 포함한 8개와 파일 바이트가 그대로 유지됨 |
| `journal_existing_entry_can_update_at_capacity` | 전체 8개 → 동일 `(vaultRoot, path)` 갱신 | 항목 수는 8, 해당 항목만 갱신 |
| `journal_discard_other_keeps_current_entries` | A·B·C 항목 준비 → B에서 discard_other | A·C만 삭제, B 유지. 다시 호출해도 안전하며 마지막 항목 삭제 시 빈 저널 파일 정리 |
| `journal_cached_root_survives_source_disconnect` | native inspector 성공으로 A를 캐시 → inspector를 실패하도록 바꿈 | save·load·clear는 캐시를 사용해 성공하고 inspector를 다시 호출하지 않음 |
| `journal_does_not_guess_an_unknown_or_changed_root` | 캐시 없음 또는 설정 루트가 B로 바뀐 상태 + inspector 실패 | `journal_unavailable`, 이전 A 캐시 재사용·기본 폴더 생성·저널 삭제 없음 |
| `journal_validates_format_and_preserves_damaged_files` | schema 1 round trip, 한도/중복 키/잘못된 필드/미래 schema/손상 JSON | 잘못된 입력 거부. 손상 JSON은 load 오류 후 다음 save에서 별도 파일로 보존하고 새 저널 작성 |

`MAX_ENTRIES = 8`, 본문 한도 `4 * 1024 * 1024` bytes, 상대 경로 1024자, revision 256 bytes를 유지한다. 새 `vaultRoot`는 빈 값·NUL을 거부하고 현재 native 루트 경계에 맞춰 32 KiB로 제한한다. decode는 중복 `(vaultRoot, path)`도 거부한다. 손상 fixture는 복구 파일이 실제로 보존되는지 확인한다.

- [ ] **Step 2: 실패 확인과 모듈 등록**

`core/mod.rs`, `commands/mod.rs`에 `pub mod journal;`을 추가하고 테스트가 컴파일되도록 타입·미구현 함수 경계만 먼저 선언한다. `tempfile`은 기존 dev-dependency를 사용한다. 새 의존성은 추가하지 않는다.

Run: `source ~/.cargo/env && cargo test -p devbox-knowledge-vault-engine --lib journal`
Expected: 아직 구현하지 않은 폴더 구분·보존 정책 때문에 FAIL. 예상과 무관한 오류를 먼저 수정한다.

- [ ] **Step 3: 모델·저장소·native 루트 캐시·component 구현**

저장용 모델과 응답용 모델을 분리한다.

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalEntry {
    pub vault_root: String,
    pub path: String,
    pub content: String,
    pub base_revision: String,
    pub saved_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntryView {
    pub path: String,
    pub content: String,
    pub base_revision: String,
    pub saved_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JournalView {
    pub entries: Vec<JournalEntryView>,
    pub other_vault_count: usize,
}
```

`upsert`는 입력 검증 → 같은 `(vaultRoot, path)` 항목 갱신 → 새 항목이면 전체 8개 한도 검사 → 추가 순서다. **다른 폴더의 오래된 항목도 자동으로 지우지 않는다.** `remove`는 두 키가 모두 일치하는 항목만 삭제한다. `discard_other`는 현재 vaultRoot와 다른 항목만 삭제한다. 오류가 나면 기존 파일을 쓰지 않는다.

파일 저장은 기존 `devbox_filesystem::atomic_write`를 사용한다. read-modify-write 전체를 하나의 mutex로 직렬화하고 읽기 크기를 전체 한도 및 JSON escape의 상한으로 제한한다. load·clear·discard는 읽기 실패를 성공으로 숨기거나 파일을 초기화하지 않는다. 손상 JSON을 옆으로 치우는 기존 복구 동작은 save에서만 수행하고 원본을 보존한다. 권한·I/O 실패나 미래 schema를 손상으로 간주해 초기화하지 않는다.

루트 캐시는 아래 순서로 구현한다.

1. `component::initialize`에서 `Arc<NoteJournalStore>`를 `dir.join("note-journal.json")`로 만들고 Tauri managed state와 `commands::docs::AppState`가 같은 인스턴스를 공유하게 한다. 문서 열기 함수의 IPC 인자는 바꾸지 않는다. AppState를 만드는 기존 테스트 fixture도 임시 저널 경로로 맞춘다. 이 초기화만으로 WSL/원격 루트에 접근하지 않는다.
2. `commands/docs.rs`의 성공한 `read_file`·`open_inbound_note` 경로에서 이미 검사한 `VaultIdentity`의 canonical root를 현재 설정 루트와 함께 캐시한다. 다른 루트의 늦은 read가 캐시를 덮지 않도록 설정 값이 여전히 같은지 재확인한다.
3. 저널 명령은 `resolve_configured_root`로 이미 설정된 루트를 읽는다(`resolve_root`의 기본 폴더 생성 경로를 쓰지 않는다). 일치하는 캐시가 있으면 source 파일시스템을 다시 검사하지 않는다. 따라서 문서를 연 뒤 WSL이 끊겨도 로컬 초안을 기록할 수 있다.
4. 캐시가 없으면 기존 notes blocking worker 경계에서 `VaultIdentity::inspect`를 한 번 수행하고, 설정 루트를 재확인한 뒤 캐시한다. 실패하면 `journal_unavailable`이며 폴더를 추정하지 않는다. 설정 루트가 달라진 경우에는 예전 캐시를 재사용하지 않는다.
5. 캐시는 노트 폴더 전체의 native 상태다. 새 루트는 다음 시작의 활성화 뒤 새 상태에서 확인한다. 프런트가 전달하는 `vaultRoot`는 허용하지 않는다(`deny_unknown_fields`). 루트 확인을 UI/IPC 스레드나 DB 잠금 아래의 원격 I/O로 옮기지 않는다.

각 명령은 확인된 현재 vaultRoot를 내부 저장소 메서드에 전달한다. `load` 응답은 현재 폴더 항목만 투영하고 다른 폴더 본문·경로는 반환하지 않는다. `savedAtMs`는 native에서 생성하며 시간 순서로 자동 삭제하는 정책은 없다.

`component.rs`의 `COMMANDS`와 `dispatch`에 다음 네 메서드를 함께 등록한다.

```rust
"save_note_journal" => crate::commands::journal::__component_save_note_journal(app, args).await,
"clear_note_journal" => crate::commands::journal::__component_clear_note_journal(app, args).await,
"load_note_journal" => crate::commands::journal::__component_load_note_journal(app, args).await,
"discard_other_vault_journal" => crate::commands::journal::__component_discard_other_vault_journal(app, args).await,
```

`apps/devbox-knowledge/src-tauri/src/component.rs`의 notes allowlist·blocking dispatch는 engine `COMMANDS` 경계를 유지한다. 폴더 변경은 현재처럼 엔진 활성화 전에만 허용하며 실행 중 전환 기능을 새로 추가하지 않는다.

- [ ] **Step 4: 통과 확인**

Run: `source ~/.cargo/env && cargo test -p devbox-knowledge-vault-engine --lib journal`
Run: `cargo test -p devbox-knowledge --lib component`
Expected: PASS. 연결 장애는 주입한 inspector/임시 fixture로 검증하며 Windows 실기는 마지막 점검표에 남긴다.

- [ ] **Step 5: 커밋**

```bash
git add crates/knowledge-vault-engine
git commit -m "feat(devbox-knowledge): scope recovery journals to their native vault roots"
```

---

### Task 5: 저널 API와 순서가 보장되는 `NoteJournal` 기록기

**Interfaces (Produces):**
- `notes/api.ts`: `NoteJournalEntry`, `NoteJournalView`, `saveNoteJournal(path, content, baseRevision)`, `clearNoteJournal(path)`, `loadNoteJournal()`, `discardOtherVaultJournal()`.
- `notes/timers.ts`: `Timers`, `browserTimers` (기존 지연 정책 유지).
- `notes/journal.ts`: `NoteJournal(target, api, onError?, timers?, delayMs?)`, `adoptRestored(path)`, `settled(): Promise<void>`, `dispose()`, `JOURNAL_DELAY_MS = 1000`.
- 기록기 오류 콜백: `"journal_unavailable" | "journal_limit"`. host Error의 `name`과 native 문자열 코드 모두 인식하고 임의 오류 문구는 반향하지 않는다.

- [ ] **Step 1: API와 타이머 추가** — `notes/api.ts` 끝에 아래 응답 계약을 추가한다.

```ts
export interface NoteJournalEntry {
  path: string;
  content: string;
  baseRevision: string;
  savedAtMs: number;
}
export interface NoteJournalView {
  entries: NoteJournalEntry[];
  otherVaultCount: number;
}

export async function saveNoteJournal(path: string, content: string, baseRevision: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("save_note_journal", { path, content, baseRevision });
}
export async function clearNoteJournal(path: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("clear_note_journal", { path });
}
export async function loadNoteJournal(): Promise<NoteJournalView> {
  if (!isTauri()) return { entries: [], otherVaultCount: 0 };
  return invoke<NoteJournalView>("load_note_journal");
}
export async function discardOtherVaultJournal(): Promise<void> {
  if (!isTauri()) return;
  await invoke("discard_other_vault_journal", {});
}
```

저장용 `vaultRoot`는 이 응답 타입에 포함하지 않는다. 위 함수는 IPC wrapper이며 기록/삭제의 호출 순서는 아래 기록기가 소유한다. 복구 화면의 명시적 삭제·목록 재조회도 진행 중 기록을 `settled()`로 기다리고, 작업 중 중복 입력을 막는다.

`notes/timers.ts`:

```ts
export interface Timers {
  set(callback: () => void, ms: number): number;
  clear(handle: number): void;
}
export const browserTimers: Timers = {
  set: (callback, ms) => window.setTimeout(callback, ms),
  clear: (handle) => window.clearTimeout(handle),
};
```

- [ ] **Step 2: 실패하는 테스트 작성** — `notes/journal.test.ts`, `notes/api.journal.test.ts`.

`NoteView`를 노출하는 FakeDocument와 fake timer를 사용한다. `content`·`path` 변경은 `sourceVersion`을 증가시키고, 비동기 순서 검사는 직접 resolve/reject할 수 있는 Promise로 고정한다. 즉시 완료 mock만으로 순서를 검증하지 않는다.

| 테스트 | 기대 결과 |
|---|---|
| 입력 중 타이머 재시작 | 1초 쉬면 마지막 path/content/revision만 한 번 기록 |
| 기록 완료 전에 노트 저장 완료 | record가 끝난 다음 clear가 실행되어 최종 저널은 없음 |
| clear 대기 중 같은 노트 재편집 | 기존 clear 완료 후 새 record가 실행되어 최신 초안이 남음 |
| 기록 실패 후 clean 전환 | 성공적으로 소유하지 못한 이전 실행의 항목을 자동 삭제하지 않음 |
| 저장 실패·문서 충돌 | dirty 버퍼와 저널 유지 |
| 사용자 승인으로 다른 노트 전환 | 이 실행에서 기록·인수한 이전 노트 항목만 순서대로 정리 |
| `journal_limit` 거부 | 오류 콜백이 한도 초과를 구분하고 버퍼·이전 저널을 유지 |
| 복구본 적용 직후 1초 전에 수동 저장 | `adoptRestored`로 인수한 항목도 저장 성공 뒤 정리 |
| 명시적 작업 전 대기 | `settled()`가 남은 record 타이머를 먼저 접수하고 record/clear가 모두 끝난 뒤 반환 |
| dispose 뒤 다시 같은 NoteDocument에 연결 | 대기 중 이전 record/clear와 새 작업이 역전되지 않음. 늦은 오류는 unmount된 화면 상태를 바꾸지 않음 |
| API 계약 | load의 객체 응답·빈 browser fixture·새 discard 메서드 및 기존 save/clear 인자 유지 |

이전 실행의 복구본은 단순히 노트를 열었다는 이유로 자동 삭제하지 않는다. 복구를 적용했거나 이 실행의 기록이 성공한 경우에만 자동 정리 대상으로 취급한다.

- [ ] **Step 3: 실패 확인**

Run: `pnpm --filter @devbox/knowledge-features exec vitest run src/notes/journal.test.ts src/notes/api.journal.test.ts`
Expected: 미구현 기록기·직렬화·새 응답 계약 때문에 FAIL.

- [ ] **Step 4: 기록기 구현** — `notes/journal.ts`.

```ts
export interface JournalTarget {
  snapshot(): NoteView;
  subscribe(listener: () => void): () => void;
}
export interface JournalApi {
  save(path: string, content: string, baseRevision: string): Promise<void>;
  clear(path: string): Promise<void>;
}
export type JournalIssue = "journal_unavailable" | "journal_limit";
```

구현 순서와 소유권:

1. 편집 이벤트에서 타이머를 재시작하고, 만료 시 path/content/revision을 캡처한 record 작업을 FIFO에 넣는다. native save Promise가 끝나기 전에는 다음 저널 작업을 보내지 않는다.
2. FIFO와 성공적으로 기록한 path 집합은 같은 NoteDocument 수명에 연결한다(예: `WeakMap<JournalTarget, QueueState>`). React StrictMode·화면 재시도로 기록기가 재생성되어도 기존 큐가 살아 있는 동안 새 큐를 병렬로 만들지 않는다. 폴더 전환은 다음 시작이므로 다른 폴더와 큐를 공유하지 않는다.
3. record 성공 뒤에만 소유 집합에 넣는다. clean 상태(`!dirty && !saving`)나 승인된 문서 전환은 clear 작업을 같은 FIFO에 넣고, 실행 시점에 소유 여부를 확인한다. clear 성공 뒤에만 집합에서 제거한다. clear 실패를 성공으로 취급하지 않는다.
4. `adoptRestored(path)`는 사용자가 현재 문서에 복구본을 적용한 시점에 호출한다. 적용만으로 저널을 삭제하지 않고 이후의 저장 성공·승인된 버리기에서 정리한다. 복구 비교 화면을 열기만 한 경우에는 호출하지 않는다.
5. `settled()`는 남은 record 타이머를 취소한 뒤 그 최신 기록을 큐에 먼저 접수하고, 대기 중 추가된 clean 정리까지 큐가 안정될 때까지 기다린다. 명시적 삭제 화면은 이 구간에 새 편집/복구 입력을 막는다. `dispose()`는 타이머·구독을 정리하되 이미 접수한 작업 순서를 무효화하지 않는다. dispose 후 결과로 이전 UI 콜백을 호출하지 않는다.
6. native/host의 `journal_limit`은 그대로 구분하고 다른 오류는 `journal_unavailable`로 보고한다. 실패한 작업 뒤에도 큐는 이어지며 unhandled rejection을 남기지 않는다.

저널 I/O 대기 때문에 `NoteDocument.edit()`나 실제 문서 저장을 막지 않는다. 대기 순서를 보장할 대상은 저널의 record/clear다. 새 폴더를 추정하거나 프런트에서 절대 경로를 붙이지 않는다.

- [ ] **Step 5: 통과 확인**

Run: `pnpm --filter @devbox/knowledge-features exec vitest run src/notes/journal.test.ts src/notes/api.journal.test.ts`
Expected: PASS. 순서 테스트는 `advanceTimersByTimeAsync`·`settled()`로 완료를 확인한다.

- [ ] **Step 6: 커밋**

```bash
git add packages/knowledge-features/src/notes/api.ts packages/knowledge-features/src/notes/api.journal.test.ts packages/knowledge-features/src/notes/timers.ts packages/knowledge-features/src/notes/journal.ts packages/knowledge-features/src/notes/journal.test.ts
git commit -m "feat(devbox-knowledge): serialize recovery journal writes and cleanup"
```

---

### Task 6: 자동 저장(`NoteAutosave`)과 전환 전 저장

**Interfaces:**
- `NoteDocument.setBeforeSwitch(fn: () => Promise<unknown>): () => void` — dirty 상태에서 다른 노트를 열기 전에 `fn`을 기다린다.
- `notes/autosave.ts`: `class NoteAutosave(target, enabled, onSaved?, timers?, delayMs?)` + `setEnabled(boolean)`, `pause()`, `flush(): Promise<boolean>`, `dispose()`; `AUTOSAVE_DELAY_MS = 1500`; `readAutosavePreference(): boolean`, `writeAutosavePreference(boolean)`; `AUTOSAVE_KEY = "devbox.knowledge.notes.autosave"`

- [ ] **Step 1: 실패하는 테스트**

`notes/noteDocument.test.ts`의 `describe` 안에 추가:

```ts
  it("saves through the switch hook before asking to discard", async () => {
    const { note, write } = await fixture();
    const discard = vi.fn(() => false);
    note.setBeforeSwitch(() => note.save());
    note.edit("changed");
    expect(await note.openPath("B.md", discard)).toBe(true);
    expect(write).toHaveBeenCalledWith("A.md", "changed", "disk-1");
    expect(discard).not.toHaveBeenCalled();
  });
```

`notes/autosave.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NoteAutosave } from "./autosave";
import type { NoteView } from "./noteDocument";

class FakeDocument {
  view: NoteView = { sourceVersion: 0, path: "a.md", content: "", revision: "r1", dirty: false, saving: false, conflict: null, error: null };
  private listeners = new Set<() => void>();
  save = vi.fn(async () => { this.set({ saving: false, dirty: false }); return true; });
  snapshot = () => this.view;
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; };
  set(change: Partial<NoteView>) {
    const bump = "content" in change || "path" in change ? 1 : 0;
    this.view = { ...this.view, ...change, sourceVersion: this.view.sourceVersion + bump };
    this.listeners.forEach((listener) => listener());
  }
}
const timers = { set: (cb: () => void, ms: number) => window.setTimeout(cb, ms), clear: (h: number) => window.clearTimeout(h) };

describe("NoteAutosave", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("saves once after edits pause", () => {
    const doc = new FakeDocument();
    const autosave = new NoteAutosave(doc, true, undefined, timers, 1500);
    doc.set({ content: "a", dirty: true });
    vi.advanceTimersByTime(1000);
    doc.set({ content: "ab", dirty: true });
    vi.advanceTimersByTime(1499);
    expect(doc.save).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(doc.save).toHaveBeenCalledTimes(1);
    autosave.dispose();
  });

  it("never saves over a conflict or when disabled", () => {
    const doc = new FakeDocument();
    const autosave = new NoteAutosave(doc, true, undefined, timers, 1500);
    doc.set({ content: "a", dirty: true, conflict: { content: "disk", revision: "r9" } });
    vi.advanceTimersByTime(5000);
    autosave.setEnabled(false);
    doc.set({ content: "b", dirty: true, conflict: null });
    vi.advanceTimersByTime(5000);
    expect(doc.save).not.toHaveBeenCalled();
    autosave.dispose();
  });

  it("stays paused after a restored buffer until the note is saved by hand", async () => {
    const doc = new FakeDocument();
    const autosave = new NoteAutosave(doc, true, undefined, timers, 1500);
    autosave.pause();
    doc.set({ content: "restored", dirty: true });
    vi.advanceTimersByTime(5000);
    expect(doc.save).not.toHaveBeenCalled();
    doc.set({ dirty: false });
    doc.set({ content: "next", dirty: true });
    vi.advanceTimersByTime(1500);
    expect(doc.save).toHaveBeenCalledTimes(1);
    autosave.dispose();
  });

  it("flush saves immediately and calls onSaved", async () => {
    const doc = new FakeDocument();
    const onSaved = vi.fn();
    const autosave = new NoteAutosave(doc, true, onSaved, timers, 1500);
    doc.set({ content: "a", dirty: true });
    expect(await autosave.flush()).toBe(true);
    expect(doc.save).toHaveBeenCalledTimes(1);
    expect(onSaved).toHaveBeenCalled();
    autosave.dispose();
  });
});
```

(`NoteSnapshot`은 `{ content, revision }`이므로 `conflict` 값으로 `{ content: "disk", revision: "r9" }`를 쓴다. 타입에 필드가 더 있으면 필수 필드만 채운다.)

- [ ] **Step 2: 실패 확인** — Run: `pnpm --filter @devbox/knowledge-features exec vitest run src/notes/autosave.test.ts src/notes/noteDocument.test.ts` → FAIL.

- [ ] **Step 3: `NoteDocument` 수정** — `noteDocument.ts`
  - 필드 추가: `private beforeSwitch: (() => Promise<unknown>) | null = null;`
  - 메서드 추가:

```ts
  /** Run `hook` (e.g. autosave flush) before a dirty buffer is replaced. */
  setBeforeSwitch(hook: () => Promise<unknown>): () => void {
    this.beforeSwitch = hook;
    return () => { if (this.beforeSwitch === hook) this.beforeSwitch = null; };
  }
```

  - `async open(load, discard)`의 첫 줄 앞에 추가:

```ts
    if (this.view.dirty && this.beforeSwitch) {
      try { await this.beforeSwitch(); } catch { /* keep the buffer; the dirty check below asks */ }
    }
```

- [ ] **Step 4: `notes/autosave.ts` 구현**

```ts
import type { NoteView } from "./noteDocument";
import { browserTimers, type Timers } from "./timers";

export const AUTOSAVE_DELAY_MS = 1500;
export const AUTOSAVE_KEY = "devbox.knowledge.notes.autosave";

export function readAutosavePreference(storage: Pick<Storage, "getItem"> = localStorage): boolean {
  try { return storage.getItem(AUTOSAVE_KEY) !== "off"; } catch { return true; }
}
export function writeAutosavePreference(enabled: boolean, storage: Pick<Storage, "setItem"> = localStorage): void {
  try { storage.setItem(AUTOSAVE_KEY, enabled ? "on" : "off"); } catch { /* preference only */ }
}

export interface AutosaveTarget {
  snapshot(): NoteView;
  subscribe(listener: () => void): () => void;
  save(): Promise<boolean>;
}

/** Saves the note after typing pauses. Conflicts and errors are left to NoteDocument. */
export class NoteAutosave {
  private timer: number | null = null;
  private paused = false;
  private lastSource = -1;
  private readonly stop: () => void;

  constructor(
    private readonly target: AutosaveTarget,
    private enabled: boolean,
    private readonly onSaved: () => void = () => {},
    private readonly timers: Timers = browserTimers,
    private readonly delayMs = AUTOSAVE_DELAY_MS,
  ) {
    this.stop = target.subscribe(() => this.onChange());
  }

  setEnabled(enabled: boolean) {
    this.enabled = enabled;
    this.cancel();
    if (enabled) this.schedule();
  }

  /** Keep a restored buffer unsaved until the user saves it explicitly. */
  pause() {
    this.paused = true;
    this.cancel();
  }

  async flush(): Promise<boolean> {
    this.cancel();
    if (!this.eligible()) return !this.target.snapshot().dirty;
    const saved = await this.target.save();
    if (saved) this.onSaved();
    return saved;
  }

  dispose() {
    this.cancel();
    this.stop();
  }

  private eligible(): boolean {
    const view = this.target.snapshot();
    return this.enabled && !this.paused && !!view.path && view.dirty && !view.saving && view.conflict === null;
  }

  private onChange() {
    const view = this.target.snapshot();
    if (!view.dirty) {
      this.paused = false;
      this.cancel();
      return;
    }
    if (view.sourceVersion === this.lastSource) return;
    this.lastSource = view.sourceVersion;
    this.schedule();
  }

  private schedule() {
    this.cancel();
    if (!this.eligible()) return;
    this.timer = this.timers.set(() => {
      this.timer = null;
      if (this.eligible()) void this.target.save().then((saved) => { if (saved) this.onSaved(); });
    }, this.delayMs);
  }

  private cancel() {
    if (this.timer !== null) {
      this.timers.clear(this.timer);
      this.timer = null;
    }
  }
}
```

- [ ] **Step 5: 통과 확인** — Run: `pnpm --filter @devbox/knowledge-features exec vitest run src/notes` → PASS.

- [ ] **Step 6: 커밋**

```bash
git add packages/knowledge-features/src/notes/noteDocument.ts packages/knowledge-features/src/notes/noteDocument.test.ts packages/knowledge-features/src/notes/autosave.ts packages/knowledge-features/src/notes/autosave.test.ts
git commit -m "feat(devbox-knowledge): autosave notes after typing pauses"
```

---

### Task 7: 복구 배너와 App 연결 — 현재 폴더만 복구

**Interfaces (Produces):**
- `RecoveryBanner({ entries, otherVaultCount, busy, onOpen, onDiscard, onDiscardOther })` (`components/RecoveryBanner.tsx`). `entries`는 현재 폴더 항목만 받는다.
- `App.tsx`는 `NoteJournalView`, `pendingRestore`, 명시적 삭제 확인 상태, 요청 세대를 관리한다. 폴더 활성화 뒤 첫 Notes mount에서 load하고 일반 트리 갱신과는 분리한다.
- `NoteJournal.adoptRestored(path)`는 복구본이 현재 문서에 적용된 뒤의 정리 소유권을 전달한다. 문서 revision 기반 조건부 저장은 그대로 유지한다.

- [ ] **Step 1: 실패하는 테스트 작성** — `components/RecoveryBanner.test.tsx`, `App.journal.test.tsx`.

| 테스트 | 동작과 단언 |
|---|---|
| 현재 폴더 복구 목록 | entries의 상대 경로와 시각을 표시하며 열기·버리기 콜백에 선택한 항목을 전달 |
| 다른 폴더 안내 | entries=[]이어도 otherVaultCount=1이면 안내 줄과 `다른 폴더 복구본 버리기` 버튼이 보임. 다른 폴더의 항목 열기·적용은 없음 |
| 일괄 삭제 확인 | 첫 클릭에는 native 호출 없음 → 화면 안 확인 취소 시 보존 → 확인 시 한 번 호출 → 성공 후 재조회하여 count 갱신 |
| 삭제 실패 | 기존 목록·개수를 유지하고 고정 오류 문구 표시. 오류를 삼키거나 성공으로 숨기지 않음 |
| A→B 활성화 | A 화면 unmount → 새 B 활성화 후 mount에서 B의 view를 읽음. A의 비교·확인 상태와 늦은 A 조회 결과가 B에 반영되지 않음 |
| 일반 메타데이터 갱신 | watcher/저장 후 트리 갱신이 진행 중 복구 비교를 지우거나 load를 반복하지 않음 |
| 같은 폴더 revision 일치 | 복구본을 편집기에 적용하고 기존 조건부 저장으로 저장. 저장 전에는 저널 보존 |
| 같은 폴더 revision 불일치 | 기존 비교 문구를 표시하고, 적용하면 수동 저장까지 자동 저장 일시 정지 |
| 비교 중 문서 변경 | 다른 노트 열기·동일 경로 재열기·추가 편집 후 이전 비교의 적용 버튼을 눌러도 현재 버퍼를 바꾸지 않음 |
| 오래된 조회 | 삭제/재조회 뒤 늦게 끝난 이전 load 성공·실패가 목록·개수·오류를 덮지 않음 |
| 접근성 | 현재 항목, 다른 폴더 안내, 삭제 확인을 각각 렌더하여 axe 위반 0 |

Run: `pnpm --filter @devbox/knowledge-features exec vitest run src/notes/components/RecoveryBanner.test.tsx src/notes/App.journal.test.tsx`
Expected: 아직 없는 컴포넌트·연결 때문에 FAIL.

- [ ] **Step 2: 복구 배너 구현** — `components/RecoveryBanner.tsx`.

- entries와 otherVaultCount가 모두 0이면 아무것도 렌더하지 않는다. 나머지는 이름 `저장하지 않은 노트 복구`의 region으로 제공한다.
- 현재 폴더 목록 **위**에 다음 안내를 둔다. 다른 폴더의 본문·경로·적용 버튼은 노출하지 않는다.

```tsx
{otherVaultCount > 0 && <div>
  <p>다른 노트 폴더에서 저장하지 않은 복구본 {otherVaultCount}개가 있습니다. 그 폴더를 다시 열면 복구할 수 있습니다.</p>
  <button disabled={busy} onClick={onDiscardOther}>다른 폴더 복구본 버리기</button>
</div>}
```

- 현재 폴더 항목은 `path`, `savedAtMs`를 표시하고 `열어서 확인`, `버리기` 버튼을 둔다. accessible name은 `${path} 열어서 확인`, `${path} 복구본 버리기`다.
- 일괄/개별 버리기는 되돌릴 수 없으므로 App에서 화면 안 확인을 한 번 거친다. 일괄 확인 문구는 `다른 노트 폴더의 복구본 N개를 영구 삭제합니다. 이 작업은 되돌릴 수 없습니다.`로 하고 확인·취소 버튼을 제공한다. 작업 중 중복 요청을 막는다.

- [ ] **Step 3: App 연결과 복구 소유권 구현** — `packages/knowledge-features/src/notes/App.tsx`.

`NoteAutosave`, `NoteJournal`, `RecoveryBanner`, 네 저널 API와 타입을 import한다. `editorDocument`별 autosave/journal 인스턴스와 ref를 만들고 다음을 연결한다.

- `NoteAutosave`의 저장 성공 콜백은 최신 `loadMetaRef.current()`를 사용한다. `setBeforeSwitch(() => autosave.flush())`, window blur, visibilitychange를 연결하고 cleanup한다. 타이머 1500ms·기본 켬·설정 키는 Task 6 그대로다.
- `NoteJournal` 오류는 코드로 표시한다. `journal_limit`이면 `복구용 임시 저장이 8개로 가득 찼습니다. 남은 복구본을 복원하거나 버린 뒤 다시 편집해 주세요.`를 보여 준다. 그 외에는 `복구용 임시 저장을 기록하지 못했습니다. 편집 내용은 유지됩니다.`를 쓴다.
- 상태는 `recovery: NoteJournalEntry[]`, `otherVaultCount`, `pendingRestore`, 삭제 확인 대상, `busy`로 둔다. 비교 대상에는 항목 외에 열린 문서의 `path`, `sourceVersion`, `revision`을 함께 캡처한다.
- 현재 제품의 폴더 변경은 `VaultSettings`에서 예약 → 다음 시작의 `VaultSetup`에서 적용 → `Startup`이 Notes를 mount하는 순서다. 이 활성화 경계를 유지하고 Notes 첫 mount에서 `loadNoteJournal()`을 호출한다. `loadMeta`, `metadataRevision`, watcher 콜백에 복구 초기화를 붙이지 않는다. 일반 트리 갱신은 폴더 변경이 아니다.
- load 요청마다 증가하는 세대를 부여하고 최신 세대의 결과만 반영한다. unmount와 명시적 삭제 시작 시 이전 요청을 무효화한다. 새 활성화에서는 목록·개수·비교·삭제 확인 상태를 새로 시작한다. 컴포넌트가 재사용되는 테스트에서는 활성화 경계를 명시적으로 재현하며 트리 refresh를 그 대용으로 쓰지 않는다.
- 같은 폴더 안에서도 path/sourceVersion/revision이 바뀌면 기존 pendingRestore를 무효화한다. 클릭 handler에서도 다시 검사해 effect 실행 전의 클릭을 막는다. `note.saving` 중에는 복구 적용을 허용하지 않는다.

복구 동작은 다음 순서로 구현한다.

1. `openRecovered(entry)`는 현재 목록의 항목인지 확인하고, 해당 경로를 `editorDocument.openPath(entry.path, confirmDiscard)`로 연다. 비동기 open 뒤에도 요청 세대와 현재 문서 소유권을 확인한다. 다른 복구 클릭·unmount·문서 편집에 뒤진 결과는 적용하지 않는다.
2. revision이 baseRevision과 같으면 해당 문서에만 `edit(entry.content)`를 적용하고 journal에 `adoptRestored(entry.path)`를 전달한다. 복구본을 읽거나 적용했다는 이유로 native clear를 먼저 호출하지 않는다. 화면 목록에서 숨기더라도 저장 실패·강제 종료 시 native 복구본은 남는다.
3. revision이 다르면 현재 문서의 path/sourceVersion/revision을 고정한 `pendingRestore`를 만든다. 비교 화면 문구는 기존대로 유지한다: `복구본을 저장한 뒤 디스크의 노트가 바뀌었습니다. 복구본을 적용하면 직접 저장할 때까지 자동 저장을 멈춥니다.`
4. `applyPendingRestore()`는 캡처한 문서가 여전히 현재 문서이고 saving이 아닌지 재확인한다. 통과하면 autosave를 pause한 뒤 edit하고 journal 소유권을 전달한다. 실패하면 비교 상태를 해제하고 현재 편집 내용을 유지한다.
5. 개별 버리기와 `discardOtherVaultJournal()`는 화면 안 확인 → busy 설정·이전 load 무효화 → `journal.settled()` 대기 → 최신 view 대조 → 해당 native 삭제 → 최신 view 재조회 순서다. 개별 버리기는 확인한 항목의 본문·revision·시각이 최신 entries와 같은지 확인하고, 일괄 버리기는 확인한 otherVaultCount가 유지되는지 확인한다. 달라졌으면 자동 삭제하지 않고 갱신된 목록에서 다시 확인하게 한다. 다른 폴더 항목을 본문까지 조회하는 API는 추가하지 않는다. 삭제 실패 시 기존 목록을 유지하고 오류를 표시한다. 새 편집/복구 작업이 이 절차와 경합하지 않도록 UI 작업 구간을 잠그며, 이미 시작된 기록/삭제를 버리지 않는다.
6. 명시적 삭제 성공 후 재조회가 실패하면 삭제 성공과 목록 재조회 실패를 구분해 알리고 다시 조회할 수 있게 한다. 이전 항목을 복원된 것으로 표시하거나 재조회 실패를 삭제 실패로 오인해 자동 재삭제하지 않는다.

복구 목록을 갱신할 때 `result.entries`와 `result.otherVaultCount`를 같은 응답에서 함께 반영한다. 개수는 폴더 수가 아니라 항목 수다. 저장 버튼 옆 자동 저장 toggle·저장 중/미저장 표시는 기존 계획대로 둔다.

- [ ] **Step 4: 통과 확인**

Run: `pnpm --filter @devbox/knowledge-features exec vitest run src/notes`
Expected: PASS. 기존 App 테스트의 API mock도 `loadNoteJournal: vi.fn().mockResolvedValue({ entries: [], otherVaultCount: 0 })`, `saveNoteJournal`, `clearNoteJournal`, `discardOtherVaultJournal`에 맞춘다.
Run: `pnpm --filter @devbox/knowledge-features exec tsc --noEmit`
Expected: 타입 오류 없음. 전체 build·verify는 B1 끝에 한다.

- [ ] **Step 5: 커밋**

```bash
git add packages/knowledge-features/src/notes
git commit -m "feat(devbox-knowledge): recover current-vault notes and explicitly discard other drafts"
```

---

### Task 8: 문서

- [ ] `apps/devbox-knowledge/README.md`에서 "강제 종료·전원 손실 후 메모리의 미저장 내용 복구는 보장하지 않으며 미저장 메모리를 상시 평문 백업하지 않는다." 문장을 아래로 바꾼다.

```markdown
  노트는 입력이 1.5초 멈추거나 창을 벗어나면 자동 저장한다(끌 수 있음). 저장 전 편집 내용은
  제품 데이터 폴더의 `note-journal.json`에 평문으로 기록했다가 저장이 끝나면 지운다. 강제 종료 뒤에는
  다음 실행 때 현재 노트 폴더의 복구 배너에서 복원하거나 버릴 수 있다. 외부에서 바뀐 파일은 자동 저장하지 않고 비교 화면을 띄운다.
  다른 노트 폴더의 복구본은 개수를 안내하며 그 폴더를 다시 열어 복구한다. 다른 폴더 복구본을 버릴 때는
  화면 안에서 한 번 확인한다. 전체 8개 한도에 도달해도 기존 복구본을 자동 삭제하지 않으며 새 임시 저장의 실패를 알린다.
  이미 문서를 열어 폴더를 확인한 뒤 연결이 끊긴 경우에도 캐시한 폴더 정보로 로컬 저널을 기록한다.
  아직 폴더를 확인하지 못한 상태나 저널 기록 전의 강제 종료까지 복구를 보장하지 않는다.
```

- [ ] 커밋: `git commit -am "docs(devbox-knowledge): describe note autosave and recovery journal"`

---

### Task 9: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9. PR 본문 "계획과 다르게 한 점"에는 2026-09-25 합의에 따라 상대 경로만으로 생기는 복구본 충돌을 막기 위해 vaultRoot 복합 키·현재 폴더 조회·명시적 일괄 버리기·한도 초과 보존·루트 캐시·활성화 시점 조회·비동기 소유권을 보완했음을 기록한다. P1-12의 메서드 목록과 결과 타입에도 네 저널 명령을 반영했는지 확인한다.
- [ ] Windows 실기 체크리스트(사용자 확인 대기로 기록):
  1. 노트를 편집하고 손을 뗀 뒤 2초 안에 "● 저장되지 않음"이 사라지는지 확인한다.
  2. 편집 직후 다른 노트를 클릭하면 확인창 없이 저장·전환되는지 확인한다.
  3. 다른 편집기로 같은 파일을 바꾼 뒤 Knowledge에서 편집하면 충돌 화면이 뜨고 자동 저장이 덮어쓰지 않는지 확인한다.
  4. 자동 저장을 끄고 편집한 뒤 작업 관리자로 강제 종료 → 재실행 시 복구 배너가 뜨고 "열어서 확인"으로 내용이 돌아오는지 확인한다.
  5. 빠른 캡처에 `token=secret-value`를 넣으면 "민감한 정보가 포함되어 있어 저장하지 않았습니다"가 보이는지 확인한다.
  6. A 폴더에 복구본을 남긴 뒤 재시작하여 B로 변경 → A 항목 대신 다른 폴더 안내만 표시되는지, A로 돌아가면 복구본이 다시 보이는지 확인한다.
  7. 다른 폴더 복구본 버리기의 확인·취소·삭제를 확인한다. 합계 8개일 때 새 저널 저장은 실패를 알리고 기존 복구본은 유지되는지 확인한다.
  8. 이미 연 WSL 노트의 연결을 사용할 수 없는 상황에서 실제 저장 실패와 로컬 저널 기록을 구분한다. 기존 서비스·네트워크를 검증 목적으로 변경하지 않고 사용자가 독립 환경에서 확인한다.
