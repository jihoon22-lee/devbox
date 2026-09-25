# P0-03 Knowledge 오류 코드·노트 자동 저장·복구 저널 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** (1) Knowledge가 오류를 한국어 문장 일부로 분류하던 것을 코드로 바꿔 사용자에게 정확한 문구가 보이게 한다(B6). (2) 노트 편집을 자동 저장한다(D12). (3) 저장되기 전 편집 내용을 복구 저널에 남겨 강제 종료·저장 실패 뒤 복원할 수 있게 한다.

**Architecture:**
- 엔진(`knowledge-vault-engine`)이 빠른 캡처·미리보기 만료/오래됨 오류를 안정 코드로 돌려준다. host의 `issue()`는 코드만 매핑하고, 프런트는 코드를 기존 한국어 문구로 바꾼다.
- 자동 저장은 `NoteAutosave`(편집이 1.5초 멈추면 `NoteDocument.save()`)와 `NoteDocument.setBeforeSwitch`(다른 노트를 열기 전에 먼저 저장)로 구현한다. 기존 조건부 저장(native revision)과 충돌 처리를 그대로 쓰므로 외부 변경을 덮어쓰지 않는다.
- 복구 저널은 제품 데이터 폴더의 평문 JSON(`note-journal.json`)이다. 개인용 기준에 따라 암호화하지 않는다(Workspace의 `recovery.json`과 같은 수준). 편집 1초 후 기록하고 저장이 끝나면 지운다.

**Tech Stack:** Rust, React 19 + TypeScript, Vitest(fake timers)

**Spec:** `review.md` §3 B6, §7 자동 저장 · `00-roadmap.md` D12(보안 조정으로 저널 암호화 제외)

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 자동 저장 기본값: 켜짐, 지연 1500ms. 설정은 `localStorage` 키 `devbox.knowledge.notes.autosave`(`"off"`면 꺼짐).
- 저널: 최대 8개 노트, 노트당 4 MiB, 경로 1024자. 파일은 `<Knowledge 제품 데이터 폴더>/note-journal.json`.
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
| `journal_limit` | 복구용 임시 저장 항목이 너무 많습니다. 열린 노트를 저장해 주세요. |

## Review Focus

1. 자동 저장 중 디스크 파일이 외부에서 바뀜 → 덮어쓰지 않고 기존 충돌 화면이 뜨며, 충돌이 풀릴 때까지 자동 저장이 멈춘다. (Task 6)
2. 편집 직후 다른 노트를 클릭 → 확인 대화상자 없이 먼저 저장되고 전환된다. 저장이 실패하면 기존처럼 버릴지 묻는다. (Task 6)
3. 강제 종료 후 재실행 → 복구 배너가 뜨고, 디스크가 그대로면 복구본이 적용된다. 디스크가 바뀌었으면 비교 화면에서 고르게 한다. (Task 7)
4. 저널 파일이 손상됨 → 불러오기는 오류 문구, 다음 기록 때 손상 파일을 옆으로 치우고 새로 시작한다. (Task 4)
5. 제품 모드에서 빠른 캡처 "민감한 정보" 오류 → 일반 오류가 아니라 정확한 문구가 보인다. (Task 3)

## Branch · PR

- 묶음: **B1** — 브랜치 `fix/suite/bootstrap-privacy-autosave-connection`, PR 제목 `fix(suite): pin toolchains and fix activity privacy, Knowledge autosave and Suite connections`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(devbox-knowledge): autosave notes, keep a recovery journal and return stable error codes`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

## File Structure

| 파일 | 변경 | 책임 |
|---|---|---|
| `crates/knowledge-vault-engine/src/core/capture.rs` | 수정 | `CaptureError::code()` |
| `crates/knowledge-vault-engine/src/commands/docs.rs` | 수정 | 캡처 오류를 코드로, 오래된 미리보기 코드 |
| `crates/knowledge-vault-engine/src/core/vault.rs:15` | 수정 | `STALE_ROOT` = `preview_stale` |
| `crates/knowledge-vault-engine/src/commands/templates.rs` | 수정 | 템플릿 미리보기 오래됨 = `preview_stale` |
| `crates/knowledge-vault-engine/src/core/rename.rs:75` | 수정 | `preview_expired` |
| `crates/knowledge-vault-engine/src/core/journal.rs` | 생성 | 저널 모델·검증 |
| `crates/knowledge-vault-engine/src/core/mod.rs` | 수정 | `pub mod journal;` |
| `crates/knowledge-vault-engine/src/commands/journal.rs` | 생성 | 저널 파일 저장소와 shim |
| `crates/knowledge-vault-engine/src/commands/mod.rs` | 수정 | `pub mod journal;` |
| `crates/knowledge-vault-engine/src/component.rs` | 수정 | COMMANDS·dispatch·`manage` |
| `apps/devbox-knowledge/src-tauri/src/component.rs` | 수정 | `issue()` 코드 매핑, substring 분기 삭제 |
| `apps/devbox-knowledge/src/issues.ts` | 수정 | 문구 추가 |
| `apps/devbox-knowledge/README.md` | 수정 | 자동 저장·저널 설명 |
| `packages/knowledge-features/src/notes/api.ts` | 수정 | 캡처 오류 변환, 저널 API |
| `packages/knowledge-features/src/notes/api.quickCapture.test.ts` | 수정 | 코드 기반 테스트 |
| `packages/knowledge-features/src/notes/noteDocument.ts` | 수정 | `setBeforeSwitch` |
| `packages/knowledge-features/src/notes/noteDocument.test.ts` | 수정 | 전환 전 저장 테스트 |
| `packages/knowledge-features/src/notes/timers.ts` | 생성 | 테스트 가능한 타이머 |
| `packages/knowledge-features/src/notes/autosave.ts`, `autosave.test.ts` | 생성 | 자동 저장 |
| `packages/knowledge-features/src/notes/journal.ts`, `journal.test.ts` | 생성 | 저널 기록기 |
| `packages/knowledge-features/src/notes/components/RecoveryBanner.tsx`, `.test.tsx` | 생성 | 복구 배너 |
| `packages/knowledge-features/src/notes/App.tsx` | 수정 | 연결, 토글, 복구 흐름 |

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

### Task 4: 복구 저널(엔진)

**Interfaces (Produces):**
- Rust `core::journal::{JournalEntry, JournalFile, JournalError, MAX_ENTRIES, MAX_CONTENT_BYTES}`
- Rust `commands::journal::NoteJournalStore::{new(PathBuf), save(JournalEntry), clear(&str), load()}` (Tauri state로 관리)
- component 메서드: `save_note_journal {path, content, baseRevision}` → `null`, `clear_note_journal {path}` → `null`, `load_note_journal {}` → `[{path, content, baseRevision, savedAtMs}]`
- 오류 코드: `journal_unavailable`, `journal_limit`

- [ ] **Step 1: 실패하는 테스트** — `crates/knowledge-vault-engine/src/core/journal.rs`

```rust
//! Crash-recovery journal for the single Notes editor. Plain JSON in the
//! product data directory. Entries are removed after a successful save.
use serde::{Deserialize, Serialize};

pub const MAX_ENTRIES: usize = 8;
pub const MAX_CONTENT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PATH_CHARS: usize = 1024;
const MAX_REVISION_BYTES: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalEntry {
    pub path: String,
    pub content: String,
    pub base_revision: String,
    pub saved_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalFile {
    pub schema_version: u32,
    pub entries: Vec<JournalEntry>,
}

impl Default for JournalFile {
    fn default() -> Self {
        Self { schema_version: 1, entries: Vec::new() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalError {
    Invalid,
    Limit,
}

pub fn validate_entry(entry: &JournalEntry) -> Result<(), JournalError> {
    if entry.path.trim().is_empty()
        || entry.path.chars().count() > MAX_PATH_CHARS
        || entry.path.contains('\0')
        || entry.base_revision.len() > MAX_REVISION_BYTES
    {
        return Err(JournalError::Invalid);
    }
    if entry.content.len() > MAX_CONTENT_BYTES {
        return Err(JournalError::Limit);
    }
    Ok(())
}

impl JournalFile {
    pub fn decode(bytes: &[u8]) -> Result<Self, JournalError> {
        let file: Self = serde_json::from_slice(bytes).map_err(|_| JournalError::Invalid)?;
        if file.schema_version != 1 || file.entries.len() > MAX_ENTRIES {
            return Err(JournalError::Invalid);
        }
        for entry in &file.entries {
            validate_entry(entry)?;
        }
        Ok(file)
    }

    pub fn encode(&self) -> Result<Vec<u8>, JournalError> {
        serde_json::to_vec(self).map_err(|_| JournalError::Invalid)
    }

    /// Replace the entry for the same path, or add one within the limit.
    pub fn upsert(&mut self, entry: JournalEntry) -> Result<(), JournalError> {
        validate_entry(&entry)?;
        if let Some(existing) = self.entries.iter_mut().find(|item| item.path == entry.path) {
            *existing = entry;
            return Ok(());
        }
        if self.entries.len() >= MAX_ENTRIES {
            return Err(JournalError::Limit);
        }
        self.entries.push(entry);
        Ok(())
    }

    pub fn remove(&mut self, path: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|item| item.path != path);
        before != self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, content: &str) -> JournalEntry {
        JournalEntry { path: path.into(), content: content.into(), base_revision: "r1".into(), saved_at_ms: 1 }
    }

    #[test]
    fn upsert_replaces_the_same_path_and_limits_new_paths() {
        let mut file = JournalFile::default();
        file.upsert(entry("a.md", "one")).unwrap();
        file.upsert(entry("a.md", "two")).unwrap();
        assert_eq!(file.entries.len(), 1);
        assert_eq!(file.entries[0].content, "two");
        for n in 1..MAX_ENTRIES {
            file.upsert(entry(&format!("n{n}.md"), "x")).unwrap();
        }
        assert_eq!(file.upsert(entry("extra.md", "x")), Err(JournalError::Limit));
    }

    #[test]
    fn invalid_and_oversized_entries_are_rejected() {
        let mut file = JournalFile::default();
        assert_eq!(file.upsert(entry(" ", "x")), Err(JournalError::Invalid));
        assert_eq!(file.upsert(entry("a\0b", "x")), Err(JournalError::Invalid));
        let big = "x".repeat(MAX_CONTENT_BYTES + 1);
        assert_eq!(file.upsert(entry("a.md", &big)), Err(JournalError::Limit));
    }

    #[test]
    fn encode_decode_round_trip_and_reject_future_schema() {
        let mut file = JournalFile::default();
        file.upsert(entry("노트/한글.md", "본문")).unwrap();
        assert_eq!(JournalFile::decode(&file.encode().unwrap()).unwrap(), file);
        assert_eq!(JournalFile::decode(br#"{"schemaVersion":2,"entries":[]}"#), Err(JournalError::Invalid));
    }
}
```

`crates/knowledge-vault-engine/src/commands/journal.rs`:

```rust
//! Note journal file store. One mutex serializes read-modify-write.
use crate::core::journal::{JournalEntry, JournalError, JournalFile};
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct NoteJournalStore {
    path: PathBuf,
    lock: Mutex<()>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

impl NoteJournalStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path, lock: Mutex::new(()) }
    }

    fn read(&self) -> Result<JournalFile, String> {
        match std::fs::read(&self.path) {
            Ok(bytes) => JournalFile::decode(&bytes).map_err(|_| "journal_unavailable".to_string()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(JournalFile::default()),
            Err(_) => Err("journal_unavailable".into()),
        }
    }

    /// Writers never fail forever on a damaged file: it is moved aside first.
    fn read_or_reset(&self) -> Result<JournalFile, String> {
        match self.read() {
            Ok(file) => Ok(file),
            Err(_) if self.path.exists() => {
                let aside = self.path.with_extension(format!("corrupt-{}.json", now_ms()));
                std::fs::rename(&self.path, aside).map_err(|_| "journal_unavailable".to_string())?;
                Ok(JournalFile::default())
            }
            Err(error) => Err(error),
        }
    }

    fn write(&self, file: &JournalFile) -> Result<(), String> {
        if file.entries.is_empty() {
            return match std::fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
                Err(_) => Err("journal_unavailable".into()),
            };
        }
        let bytes = file.encode().map_err(|_| "journal_unavailable".to_string())?;
        devbox_filesystem::atomic_write(&self.path, &bytes).map_err(|_| "journal_unavailable".into())
    }

    pub fn save(&self, path: String, content: String, base_revision: String) -> Result<(), String> {
        let _guard = self.lock.lock().map_err(|_| "journal_unavailable".to_string())?;
        let mut file = self.read_or_reset()?;
        file.upsert(JournalEntry { path, content, base_revision, saved_at_ms: now_ms() })
            .map_err(|error| match error {
                JournalError::Limit => "journal_limit".to_string(),
                JournalError::Invalid => "journal_unavailable".to_string(),
            })?;
        self.write(&file)
    }

    pub fn clear(&self, path: &str) -> Result<(), String> {
        let _guard = self.lock.lock().map_err(|_| "journal_unavailable".to_string())?;
        let mut file = self.read_or_reset()?;
        if file.remove(path) {
            self.write(&file)
        } else {
            Ok(())
        }
    }

    pub fn load(&self) -> Result<Vec<JournalEntry>, String> {
        let _guard = self.lock.lock().map_err(|_| "journal_unavailable".to_string())?;
        Ok(self.read()?.entries)
    }
}

/// Typed product adapters; the host enforces owner/session authorization.
pub(crate) async fn __component_save_note_journal(
    app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        path: String,
        content: String,
        base_revision: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    app.state::<NoteJournalStore>().save(input.path, input.content, input.base_revision)?;
    Ok(serde_json::Value::Null)
}

pub(crate) async fn __component_clear_note_journal(
    app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        path: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    app.state::<NoteJournalStore>().clear(&input.path)?;
    Ok(serde_json::Value::Null)
}

pub(crate) async fn __component_load_note_journal(
    app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {}
    let Input {} = serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let entries = app.state::<NoteJournalStore>().load()?;
    serde_json::to_value(entries).map_err(|_| "component_response_invalid".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_clear_round_trip_and_remove_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = NoteJournalStore::new(dir.path().join("note-journal.json"));
        store.save("a.md".into(), "draft".into(), "r1".into()).unwrap();
        let entries = store.load().unwrap();
        assert_eq!((entries[0].path.as_str(), entries[0].content.as_str()), ("a.md", "draft"));
        store.clear("a.md").unwrap();
        assert!(store.load().unwrap().is_empty());
        assert!(!dir.path().join("note-journal.json").exists());
    }

    #[test]
    fn damaged_file_fails_load_but_next_save_moves_it_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note-journal.json");
        std::fs::write(&path, b"{broken").unwrap();
        let store = NoteJournalStore::new(path.clone());
        assert_eq!(store.load().unwrap_err(), "journal_unavailable");
        store.save("a.md".into(), "draft".into(), "r1".into()).unwrap();
        assert_eq!(store.load().unwrap().len(), 1);
        let aside = std::fs::read_dir(dir.path()).unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().contains("corrupt-"));
        assert!(aside);
    }
}
```

- [ ] **Step 2: 모듈 등록** — `core/mod.rs`에 `pub mod journal;`, `commands/mod.rs`에 `pub mod journal;`. `crates/knowledge-vault-engine/Cargo.toml`의 `[dev-dependencies]`에 `tempfile`이 없으면 `tempfile = "3"`을 추가한다.

- [ ] **Step 3: component 등록** — `crates/knowledge-vault-engine/src/component.rs`
  - `COMMANDS`의 `"knowledge_watcher_status",` 다음에 `"save_note_journal", "clear_note_journal", "load_note_journal",` 추가.
  - `dispatch`의 match에 추가:

```rust
        "save_note_journal" => crate::commands::journal::__component_save_note_journal(app, args).await,
        "clear_note_journal" => crate::commands::journal::__component_clear_note_journal(app, args).await,
        "load_note_journal" => crate::commands::journal::__component_load_note_journal(app, args).await,
```

  - `initialize`의 `std::fs::create_dir_all(dir)?;` 바로 다음에 추가:

```rust
    app.manage(crate::commands::journal::NoteJournalStore::new(dir.join("note-journal.json")));
```

- [ ] **Step 4: 통과 확인** — Run: `cargo test -p devbox-knowledge-vault-engine --lib journal` → PASS. `cargo test -p devbox-knowledge --lib` → PASS(host 허용목록은 `COMMANDS`를 그대로 쓴다).

- [ ] **Step 5: 커밋**

```bash
git add crates/knowledge-vault-engine
git commit -m "feat(devbox-knowledge): keep a crash recovery journal for unsaved notes"
```

---

### Task 5: 저널 API와 `NoteJournal` 기록기

**Interfaces (Produces):**
- `notes/api.ts`: `interface NoteJournalEntry { path: string; content: string; baseRevision: string; savedAtMs: number }`, `saveNoteJournal(path, content, baseRevision)`, `clearNoteJournal(path)`, `loadNoteJournal()`
- `notes/timers.ts`: `interface Timers { set(callback: () => void, ms: number): number; clear(handle: number): void }`, `browserTimers`
- `notes/journal.ts`: `class NoteJournal(target, api, onError?, timers?, delayMs?)` + `dispose()`, `JOURNAL_DELAY_MS = 1000`

- [ ] **Step 1: API 추가** — `notes/api.ts` 끝에:

```ts
export interface NoteJournalEntry { path: string; content: string; baseRevision: string; savedAtMs: number }

export async function saveNoteJournal(path: string, content: string, baseRevision: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("save_note_journal", { path, content, baseRevision });
}

export async function clearNoteJournal(path: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("clear_note_journal", { path });
}

export async function loadNoteJournal(): Promise<NoteJournalEntry[]> {
  if (!isTauri()) return [];
  return invoke<NoteJournalEntry[]>("load_note_journal");
}
```

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

- [ ] **Step 2: 실패하는 테스트** — `notes/journal.test.ts`

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NoteJournal } from "./journal";
import type { NoteView } from "./noteDocument";

class FakeDocument {
  view: NoteView = { sourceVersion: 0, path: "a.md", content: "saved", revision: "r1", dirty: false, saving: false, conflict: null, error: null };
  private listeners = new Set<() => void>();
  snapshot = () => this.view;
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; };
  set(change: Partial<NoteView>) {
    const bump = "content" in change || "path" in change ? 1 : 0;
    this.view = { ...this.view, ...change, sourceVersion: this.view.sourceVersion + bump };
    this.listeners.forEach((listener) => listener());
  }
}
const timers = { set: (cb: () => void, ms: number) => window.setTimeout(cb, ms), clear: (h: number) => window.clearTimeout(h) };

describe("NoteJournal", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("records the latest dirty content once the typing pauses", async () => {
    const doc = new FakeDocument();
    const api = { save: vi.fn().mockResolvedValue(undefined), clear: vi.fn().mockResolvedValue(undefined) };
    const journal = new NoteJournal(doc, api, () => {}, timers, 1000);
    doc.set({ content: "a", dirty: true });
    vi.advanceTimersByTime(500);
    doc.set({ content: "ab", dirty: true });
    vi.advanceTimersByTime(1000);
    expect(api.save).toHaveBeenCalledTimes(1);
    expect(api.save).toHaveBeenCalledWith("a.md", "ab", "r1");
    journal.dispose();
  });

  it("clears the entry after the note is saved", async () => {
    const doc = new FakeDocument();
    const api = { save: vi.fn().mockResolvedValue(undefined), clear: vi.fn().mockResolvedValue(undefined) };
    const journal = new NoteJournal(doc, api, () => {}, timers, 1000);
    doc.set({ content: "draft", dirty: true });
    vi.advanceTimersByTime(1000);
    doc.set({ dirty: false, revision: "r2" });
    expect(api.clear).toHaveBeenCalledWith("a.md");
    journal.dispose();
  });

  it("drops the journal of a buffer the user discarded by switching notes", () => {
    const doc = new FakeDocument();
    const api = { save: vi.fn().mockResolvedValue(undefined), clear: vi.fn().mockResolvedValue(undefined) };
    const journal = new NoteJournal(doc, api, () => {}, timers, 1000);
    doc.set({ content: "draft", dirty: true });
    vi.advanceTimersByTime(1000);
    doc.set({ path: "b.md", content: "other", dirty: false, revision: "rb" });
    expect(api.clear).toHaveBeenCalledWith("a.md");
    journal.dispose();
  });

  it("reports a failed write without throwing", async () => {
    const doc = new FakeDocument();
    const onError = vi.fn();
    const api = { save: vi.fn().mockRejectedValue(new Error("x")), clear: vi.fn().mockResolvedValue(undefined) };
    const journal = new NoteJournal(doc, api, onError, timers, 1000);
    doc.set({ content: "draft", dirty: true });
    await vi.advanceTimersByTimeAsync(1000);
    expect(onError).toHaveBeenCalledWith("journal_unavailable");
    journal.dispose();
  });
});
```

- [ ] **Step 3: 실패 확인** — Run: `pnpm --filter @devbox/knowledge-features exec vitest run src/notes/journal.test.ts` → FAIL(모듈 없음).

- [ ] **Step 4: 구현** — `notes/journal.ts`

```ts
import type { NoteView } from "./noteDocument";
import { browserTimers, type Timers } from "./timers";

export const JOURNAL_DELAY_MS = 1000;

export interface JournalTarget {
  snapshot(): NoteView;
  subscribe(listener: () => void): () => void;
}
export interface JournalApi {
  save(path: string, content: string, baseRevision: string): Promise<void>;
  clear(path: string): Promise<void>;
}

/**
 * Mirrors the unsaved editor buffer into the native recovery journal.
 * Only entries written in this session are cleared automatically; entries
 * left by a previous run stay until the user restores or discards them.
 */
export class NoteJournal {
  private timer: number | null = null;
  private written = new Set<string>();
  private lastPath: string | null;
  private lastSource = -1;
  private readonly stop: () => void;

  constructor(
    private readonly target: JournalTarget,
    private readonly api: JournalApi,
    private readonly onError: (code: "journal_unavailable") => void = () => {},
    private readonly timers: Timers = browserTimers,
    private readonly delayMs = JOURNAL_DELAY_MS,
  ) {
    this.lastPath = target.snapshot().path;
    this.stop = target.subscribe(() => this.onChange());
  }

  dispose() {
    this.cancel();
    this.stop();
  }

  private clear(path: string) {
    this.written.delete(path);
    void this.api.clear(path).catch(() => this.onError("journal_unavailable"));
  }

  private onChange() {
    const view = this.target.snapshot();
    if (view.path !== this.lastPath) {
      this.cancel();
      if (this.lastPath !== null && this.written.has(this.lastPath)) this.clear(this.lastPath);
      this.lastPath = view.path;
    }
    if (!view.path) return;
    if (!view.dirty && !view.saving) {
      this.cancel();
      if (this.written.has(view.path)) this.clear(view.path);
      return;
    }
    if (!view.dirty || view.sourceVersion === this.lastSource) return;
    this.lastSource = view.sourceVersion;
    this.cancel();
    const { path, content, revision } = view;
    this.timer = this.timers.set(() => {
      this.timer = null;
      this.written.add(path);
      void this.api.save(path, content, revision).catch(() => this.onError("journal_unavailable"));
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

- [ ] **Step 5: 통과 확인** — Run: `pnpm --filter @devbox/knowledge-features exec vitest run src/notes/journal.test.ts` → PASS.

- [ ] **Step 6: 커밋**

```bash
git add packages/knowledge-features/src/notes/api.ts packages/knowledge-features/src/notes/timers.ts packages/knowledge-features/src/notes/journal.ts packages/knowledge-features/src/notes/journal.test.ts
git commit -m "feat(devbox-knowledge): mirror unsaved note buffers into the recovery journal"
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

### Task 7: 복구 배너와 App 연결

**Interfaces (Produces):** `RecoveryBanner({ entries, onOpen, onDiscard })` (`components/RecoveryBanner.tsx`)

- [ ] **Step 1: 실패하는 테스트** — `components/RecoveryBanner.test.tsx`

```tsx
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { afterEach, expect, it, vi } from "vitest";
import RecoveryBanner from "./RecoveryBanner";

afterEach(cleanup);
const entry = { path: "notes/a.md", content: "draft", baseRevision: "r1", savedAtMs: Date.UTC(2026, 8, 23, 3, 0) };

it("lists unsaved notes and routes open/discard", async () => {
  const onOpen = vi.fn(), onDiscard = vi.fn();
  const { container } = render(<RecoveryBanner entries={[entry]} onOpen={onOpen} onDiscard={onDiscard} />);
  expect(screen.getByRole("region", { name: "저장하지 않은 노트 복구" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "notes/a.md 열어서 확인" }));
  fireEvent.click(screen.getByRole("button", { name: "notes/a.md 복구본 버리기" }));
  expect(onOpen).toHaveBeenCalledWith(entry);
  expect(onDiscard).toHaveBeenCalledWith(entry);
  await assertNoA11yViolations(container);
});

it("renders nothing without entries", () => {
  const { container } = render(<RecoveryBanner entries={[]} onOpen={vi.fn()} onDiscard={vi.fn()} />);
  expect(container.firstChild).toBeNull();
});
```

- [ ] **Step 2: 구현** — `components/RecoveryBanner.tsx`

```tsx
import type { NoteJournalEntry } from "../api";

export default function RecoveryBanner({ entries, onOpen, onDiscard }: {
  entries: NoteJournalEntry[];
  onOpen: (entry: NoteJournalEntry) => void;
  onDiscard: (entry: NoteJournalEntry) => void;
}) {
  if (entries.length === 0) return null;
  return (
    <section className="recovery-banner" role="region" aria-label="저장하지 않은 노트 복구">
      <p>저장하지 않은 편집 {entries.length}개가 남아 있습니다.</p>
      <ul>
        {entries.map((entry) => (
          <li key={entry.path}>
            <span>{entry.path}</span>{" "}
            <time dateTime={new Date(entry.savedAtMs).toISOString()}>{new Date(entry.savedAtMs).toLocaleString("ko-KR")}</time>{" "}
            <button onClick={() => onOpen(entry)} aria-label={`${entry.path} 열어서 확인`}>열어서 확인</button>{" "}
            <button onClick={() => onDiscard(entry)} aria-label={`${entry.path} 복구본 버리기`}>버리기</button>
          </li>
        ))}
      </ul>
    </section>
  );
}
```

- [ ] **Step 3: App.tsx 연결** — `packages/knowledge-features/src/notes/App.tsx`
  - import 추가: `NoteAutosave, readAutosavePreference, writeAutosavePreference`(`./autosave`), `NoteJournal`(`./journal`), `RecoveryBanner`(`./components/RecoveryBanner`), `saveNoteJournal, clearNoteJournal, loadNoteJournal, type NoteJournalEntry`(`./api`).
  - 155행(`const { path: selected, … } = note;`) 아래에 state·효과를 추가:

```tsx
  const [autosaveEnabled, setAutosaveEnabled] = useState(() => readAutosavePreference());
  const autosaveRef = useRef<NoteAutosave | null>(null);
  const [recovery, setRecovery] = useState<NoteJournalEntry[]>([]);
  const [pendingRestore, setPendingRestore] = useState<{ entry: NoteJournalEntry; disk: string } | null>(null);
  useEffect(() => {
    const autosave = new NoteAutosave(editorDocument, readAutosavePreference(), () => { void loadMetaRef.current(); });
    autosaveRef.current = autosave;
    const release = editorDocument.setBeforeSwitch(() => autosave.flush());
    const journal = new NoteJournal(editorDocument, { save: saveNoteJournal, clear: clearNoteJournal },
      () => setError("복구용 임시 저장을 기록하지 못했습니다. 편집 내용은 유지됩니다."));
    const onBlur = () => { void autosave.flush(); };
    const onVisibility = () => { if (document.hidden) void autosave.flush(); };
    window.addEventListener("blur", onBlur);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      window.removeEventListener("blur", onBlur);
      document.removeEventListener("visibilitychange", onVisibility);
      release(); journal.dispose(); autosave.dispose(); autosaveRef.current = null;
    };
  }, [editorDocument]);
  useEffect(() => {
    autosaveRef.current?.setEnabled(autosaveEnabled);
    writeAutosavePreference(autosaveEnabled);
  }, [autosaveEnabled]);
  useEffect(() => {
    let active = true;
    void loadNoteJournal().then((entries) => { if (active) setRecovery(entries); })
      .catch(() => { if (active) setError("복구 정보를 읽지 못했습니다."); });
    return () => { active = false; };
  }, []);
```

  - `loadMeta`가 선언된 뒤에 최신 참조를 유지한다(효과가 `loadMeta` 변경마다 다시 만들어지지 않도록): `const loadMetaRef = useRef(loadMeta); loadMetaRef.current = loadMeta;` — `loadMeta` 선언 위치(`rg -n "const loadMeta" packages/knowledge-features/src/notes/App.tsx`) 바로 아래에 두고, 위 효과는 그보다 아래로 옮긴다.
  - 복구 동작 함수(같은 컴포넌트 안):

```tsx
  const openRecovered = async (entry: NoteJournalEntry) => {
    if (!(await editorDocument.openPath(entry.path, confirmDiscard))) return;
    const view = editorDocument.snapshot();
    if (view.revision === entry.baseRevision) {
      editorDocument.edit(entry.content);
      setRecovery((list) => list.filter((item) => item.path !== entry.path));
    } else {
      setPendingRestore({ entry, disk: view.content });
    }
  };
  const applyPendingRestore = () => {
    if (!pendingRestore) return;
    editorDocument.edit(pendingRestore.entry.content);
    autosaveRef.current?.pause();
    setRecovery((list) => list.filter((item) => item.path !== pendingRestore.entry.path));
    setPendingRestore(null);
  };
  const discardRecovered = async (entry: NoteJournalEntry) => {
    await clearNoteJournal(entry.path).catch(() => undefined);
    setRecovery((list) => list.filter((item) => item.path !== entry.path));
    if (pendingRestore?.entry.path === entry.path) setPendingRestore(null);
  };
```

  - JSX: 노트 편집 영역 최상단(1190행 부근 toolbar를 감싸는 요소 앞)에 추가:

```tsx
      <RecoveryBanner entries={recovery} onOpen={(entry) => void openRecovered(entry)} onDiscard={(entry) => void discardRecovered(entry)} />
      {pendingRestore && <section aria-label="복구본 비교">
        <p>복구본을 저장한 뒤 디스크의 노트가 바뀌었습니다. 복구본을 적용하면 직접 저장할 때까지 자동 저장을 멈춥니다.</p>
        <ChangeSetPreview selectable={false} approveLabel="복구본으로 바꾸기" onApprove={applyPendingRestore}
          items={[{ path: pendingRestore.entry.path, before: pendingRestore.disk, after: pendingRestore.entry.content, meta: "디스크 내용 → 복구본" }]} />
        <button onClick={() => void discardRecovered(pendingRestore.entry)}>디스크 내용 유지</button>
      </section>}
```

  - 저장 버튼 옆(1198–1201행)을 아래로 바꾼다.

```tsx
              <label className="row">
                <input type="checkbox" checked={autosaveEnabled} onChange={(event) => setAutosaveEnabled(event.currentTarget.checked)} />
                자동 저장
              </label>
              {note.saving ? <span role="status">저장 중…</span> : dirty ? <span className="dirty">● 저장되지 않음</span> : null}
              <button className="btn" disabled={note.saving} onClick={() => void save()}>
                저장
              </button>
```

- [ ] **Step 4: 통과 확인**

Run: `pnpm --filter @devbox/knowledge-features exec vitest run src/notes` → PASS (기존 App 테스트가 `loadNoteJournal` 등 새 API를 mock하지 않아 실패하면, 해당 테스트 파일의 `vi.mock("./api", …)` 목록에 `loadNoteJournal: vi.fn().mockResolvedValue([])`, `saveNoteJournal: vi.fn()`, `clearNoteJournal: vi.fn()`을 추가한다).
Run: `pnpm --filter @devbox/knowledge-features build` → 타입 오류 없음.

- [ ] **Step 5: 커밋**

```bash
git add packages/knowledge-features/src/notes
git commit -m "feat(devbox-knowledge): offer recovery of unsaved notes and an autosave toggle"
```

---

### Task 8: 문서

- [ ] `apps/devbox-knowledge/README.md`에서 "강제 종료·전원 손실 후 메모리의 미저장 내용 복구는 보장하지 않으며 미저장 메모리를 상시 평문 백업하지 않는다." 문장을 아래로 바꾼다.

```markdown
  노트는 입력이 1.5초 멈추거나 창을 벗어나면 자동 저장한다(끌 수 있음). 저장 전 편집 내용은
  제품 데이터 폴더의 `note-journal.json`에 평문으로 기록했다가 저장이 끝나면 지운다. 강제 종료 뒤에는
  다음 실행 때 복구 배너에서 복원하거나 버릴 수 있다. 외부에서 바뀐 파일은 자동 저장하지 않고 비교 화면을 띄운다.
```

- [ ] 커밋: `git commit -am "docs(devbox-knowledge): describe note autosave and recovery journal"`

---

### Task 9: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] Windows 실기 체크리스트(사용자 확인 대기로 기록):
  1. 노트를 편집하고 손을 뗀 뒤 2초 안에 "● 저장되지 않음"이 사라지는지 확인한다.
  2. 편집 직후 다른 노트를 클릭하면 확인창 없이 저장·전환되는지 확인한다.
  3. 다른 편집기로 같은 파일을 바꾼 뒤 Knowledge에서 편집하면 충돌 화면이 뜨고 자동 저장이 덮어쓰지 않는지 확인한다.
  4. 자동 저장을 끄고 편집한 뒤 작업 관리자로 강제 종료 → 재실행 시 복구 배너가 뜨고 "열어서 확인"으로 내용이 돌아오는지 확인한다.
  5. 빠른 캡처에 `token=secret-value`를 넣으면 "민감한 정보가 포함되어 있어 저장하지 않았습니다"가 보이는지 확인한다.
