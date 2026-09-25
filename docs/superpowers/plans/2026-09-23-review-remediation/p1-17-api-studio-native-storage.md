# P1-17 API Studio 저장소를 native SQLite로 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** API Studio의 컬렉션·기록·환경·gRPC 기록·변환 워크플로를 WebView `localStorage`(5–10MB 한도, 프로필 초기화 시 유실)에서 제품 데이터 폴더의 SQLite로 옮기고, 기존 `localStorage` 데이터는 첫 실행 때 자동으로 이전한다(A6, D21).

**Architecture:** 저장 단위는 지금 프런트가 이미 다루는 "문서"(store JSON) 다섯 종류다: `collections`, `history`, `environments`, `grpc_history`, `workflows`. native는 `documents(kind PRIMARY KEY, revision, body, updated_ms)` 표 하나에 문서를 revision 비교(CAS)로 저장한다. 프런트의 파싱·정리(sanitize) 로직은 그대로 두고 저장소 경계만 `DocumentStorage` 인터페이스(async `load`/`save`)로 바꾼다. 브라우저 미리보기에서는 같은 인터페이스를 `localStorage`로 구현한다. 새 catalog component `api-studio.store`(typed command `store`)가 모든 API Studio route에서 이 두 메서드만 허용한다. 파일 기반 컬렉션 import/export는 Phase 2(P2-10)다.

**Tech Stack:** Rust(`rusqlite` bundled, workspace), Tauri v2, TypeScript, Vitest

**Spec:** `review.md` §6 A6 · `00-roadmap.md` D21

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 문서 한도 16MiB(종류마다). 넘으면 `store_document_too_large`로 거부하고 기존 문서는 그대로 둔다.
- 비밀값은 지금처럼 DPAPI로 봉인된 문자열 그대로 문서 안에 둔다(평문 저장 금지 규칙 유지).
- 자동 이전: native에 해당 종류 문서가 없고 `localStorage`에 v2 키가 있을 때만 한다. native 저장 → 다시 읽어 같은지 확인 → `localStorage` 키 삭제 순서. 실패하면 `localStorage`를 지우지 않고 다음 실행에 다시 시도한다. v1 키와 이전 표시(marker) 키는 이미 v2로 옮겨진 옛 형식이므로 이전하지 않고 지운다.
- 옛 키: `apip-collections-v2`, `apip-history-v2`, `apip-environments`, `devbox.api-playground.grpc-history/v1`, `devbox.developer-toolbox.smart-workflows.v1`(값 그대로 문서 body가 된다).

## Review Focus

1. v0.8.x에서 쓰던 컬렉션·환경이 있는 상태로 업데이트 → 첫 실행에 모두 보이고, `localStorage`에서는 키가 지워진다. (Task 3 테스트)
2. 이전 중 native 저장이 실패(디스크 가득) → 기존 `localStorage`가 남아 다음 실행에 다시 시도한다. (Task 3)
3. 저장 사이에 다른 경로(예: 두 번째 창·같은 창 중복 저장)가 문서를 바꿈 → `store_revision_conflict`로 알리고 덮어쓰지 않는다. (Task 1·2)
4. 문서가 16MiB를 넘음(거대한 기록) → 저장 거부 문구가 보이고 기존 문서가 유지된다. (Task 1)
5. WebView2 데이터 폴더를 지워도 컬렉션이 남는다(native 파일). (사용자 실기)

## Branch · PR

- 묶음: **B8** — 브랜치 `refactor/suite/hooks-streaming-store-undo`, PR 제목 `refactor(suite): shared hooks, terminal streaming, native API Studio store, undo and agent protocol`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(devbox-api-studio): keep collections and history in a native store`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: native 문서 저장소

**Files:** Create `apps/devbox-api-studio/src-tauri/src/core/documents.rs`; Modify `apps/devbox-api-studio/src-tauri/src/core/mod.rs`, `Cargo.toml`(`rusqlite = { workspace = true }`)

**Interfaces (Produces):** `DocumentKind { Collections, History, Environments, GrpcHistory, Workflows }`(serde snake_case, ts-rs), `Stored { revision: u64, body: String }`, `DocumentStore::open(path) -> Result<Self, String>`, `load(kind) -> Result<Option<Stored>, String>`, `save(kind, body: &str, expected_revision: Option<u64>) -> Result<u64, String>`; 상수 `MAX_DOCUMENT_BYTES = 16 * 1024 * 1024`

- [ ] **Step 1: 실패하는 테스트**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, DocumentStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = DocumentStore::open(&dir.path().join("api-store.db")).unwrap();
        (dir, store)
    }

    #[test]
    fn documents_are_created_then_updated_with_revisions() {
        let (_dir, store) = store();
        assert!(store.load(DocumentKind::Collections).unwrap().is_none());
        let first = store.save(DocumentKind::Collections, r#"{"version":2,"entries":[]}"#, None).unwrap();
        assert_eq!(first, 1);
        let second = store.save(DocumentKind::Collections, r#"{"version":2,"entries":[1]}"#, Some(first)).unwrap();
        assert_eq!(second, 2);
        assert_eq!(store.load(DocumentKind::Collections).unwrap().unwrap().body, r#"{"version":2,"entries":[1]}"#);
    }

    #[test]
    fn stale_writers_and_oversized_documents_are_refused() {
        let (_dir, store) = store();
        store.save(DocumentKind::History, "{}", None).unwrap();
        assert_eq!(store.save(DocumentKind::History, "{\"a\":1}", Some(7)).unwrap_err(), "store_revision_conflict");
        assert_eq!(store.save(DocumentKind::History, "{}", None).unwrap_err(), "store_revision_conflict");
        let huge = "x".repeat(MAX_DOCUMENT_BYTES + 1);
        assert_eq!(store.save(DocumentKind::History, &huge, Some(1)).unwrap_err(), "store_document_too_large");
        assert_eq!(store.load(DocumentKind::History).unwrap().unwrap().body, "{}");
    }

    #[test]
    fn bodies_must_be_json() {
        let (_dir, store) = store();
        assert_eq!(store.save(DocumentKind::Workflows, "not json", None).unwrap_err(), "store_document_invalid");
    }
}
```

- [ ] **Step 2: 실패 확인** — Run: `source ~/.cargo/env && cargo test -p devbox-api-studio --lib core::documents` → 컴파일 실패.

- [ ] **Step 3: 구현**

```rust
//! One JSON document per API Studio store, replacing WebView localStorage.
//! Writes are compare-and-swap on a revision so a stale writer cannot
//! overwrite newer data.
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

pub const MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Collections,
    History,
    Environments,
    GrpcHistory,
    Workflows,
}

impl DocumentKind {
    fn key(self) -> &'static str {
        match self {
            Self::Collections => "collections",
            Self::History => "history",
            Self::Environments => "environments",
            Self::GrpcHistory => "grpc_history",
            Self::Workflows => "workflows",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Stored {
    pub revision: u64,
    pub body: String,
}

pub struct DocumentStore(Mutex<Connection>);

impl DocumentStore {
    pub fn open(path: &Path) -> Result<Self, String> {
        let connection = Connection::open(path).map_err(|_| "store_unavailable")?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 CREATE TABLE IF NOT EXISTS documents (
                   kind TEXT PRIMARY KEY,
                   revision INTEGER NOT NULL,
                   body TEXT NOT NULL,
                   updated_ms INTEGER NOT NULL
                 );",
            )
            .map_err(|_| "store_unavailable")?;
        Ok(Self(Mutex::new(connection)))
    }

    pub fn load(&self, kind: DocumentKind) -> Result<Option<Stored>, String> {
        let connection = self.0.lock().map_err(|_| "store_unavailable")?;
        connection
            .query_row(
                "SELECT revision, body FROM documents WHERE kind = ?1",
                params![kind.key()],
                |row| Ok(Stored { revision: row.get::<_, i64>(0)? as u64, body: row.get(1)? }),
            )
            .optional()
            .map_err(|_| "store_unavailable".into())
    }

    pub fn save(&self, kind: DocumentKind, body: &str, expected: Option<u64>) -> Result<u64, String> {
        if body.len() > MAX_DOCUMENT_BYTES {
            return Err("store_document_too_large".into());
        }
        serde_json::from_str::<serde_json::Value>(body).map_err(|_| "store_document_invalid")?;
        let mut connection = self.0.lock().map_err(|_| "store_unavailable")?;
        let transaction = connection.transaction().map_err(|_| "store_unavailable")?;
        let current: Option<i64> = transaction
            .query_row("SELECT revision FROM documents WHERE kind = ?1", params![kind.key()], |row| row.get(0))
            .optional()
            .map_err(|_| "store_unavailable")?;
        if current.map(|value| value as u64) != expected {
            return Err("store_revision_conflict".into());
        }
        let next = expected.unwrap_or(0) + 1;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_millis() as i64);
        transaction
            .execute(
                "INSERT INTO documents(kind, revision, body, updated_ms) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(kind) DO UPDATE SET revision = ?2, body = ?3, updated_ms = ?4",
                params![kind.key(), next as i64, body, now],
            )
            .map_err(|_| "store_unavailable")?;
        transaction.commit().map_err(|_| "store_unavailable")?;
        Ok(next)
    }
}
```

- [ ] **Step 4: 통과 확인·커밋** — Run: `cargo test -p devbox-api-studio --lib core::documents` → PASS. `git add -A && git commit -m "feat(devbox-api-studio): add a native document store"`

---

### Task 2: `store` command와 catalog

**Files:** Create `apps/devbox-api-studio/src-tauri/src/ipc/store.rs`; Modify `ipc/mod.rs`(plugin setup에서 `DocumentStore::open(app_local_data_dir/api-store.db)`를 manage), `lib.rs`, `capabilities/*.json`, `apps/products.json`

**Interfaces (Produces):** `StoreCall { Load { kind }, Save { kind, body, expected_revision: Option<u64> } }`(`COMPONENT = "api-studio.store"`, routes `requests, protocols, webhooks, transforms, history`); Tauri command `store`; `StoreIssue { StoreUnavailable, RevisionConflict, TooLarge, Invalid }`

- [ ] **Step 1: 실패하는 테스트** — `ipc/store.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use product_ipc::ComponentCall;

    #[test]
    fn store_calls_parse_and_are_allowed_from_every_route() {
        let call: StoreCall = serde_json::from_str(r#"{"method":"save","args":{"kind":"collections","body":"{}","expectedRevision":null}}"#).unwrap();
        assert_eq!(call.method(), "save");
        assert_eq!(call.routes(), &["requests", "protocols", "webhooks", "transforms", "history"]);
        assert!(serde_json::from_str::<StoreCall>(r#"{"method":"load","args":{"kind":"secrets"}}"#).is_err());
    }
}
```

- [ ] **Step 2: 구현** — P1-13의 host command 모양대로 `store` command를 만든다(`admit` → `lifecycle::require_open` → `DocumentStore` 호출 → `finish`). `load` 결과는 `Option<Stored>`, `save`는 새 revision. catalog `components`에 `{"id": "api-studio.store", "owner": "api-studio", "authority": "local-store", "lifecycle": "product", "protocolVersion": 1}`을 추가하고 `catalogRevision` +1(fixture 갱신). `crates/catalog`가 authority 목록을 검사하면 `local-store`를 추가한다. capability에 `allow-store`를 추가한다.
- [ ] **Step 3: 통과 확인·커밋** — Run: `cargo test -p devbox-api-studio --lib && cargo test -p catalog` → PASS. `git add -A && git commit -m "feat(devbox-api-studio): expose the document store as a typed command"`

---

### Task 3: 프런트 저장소 경계와 자동 이전

**Files:** Create `packages/api-studio-features/src/storage/{documentStorage.ts,documentStorage.test.ts,migrate.ts,migrate.test.ts}`; Modify `requests/lib/{collections,persistence,environments,grpc}.ts`, `requests/HistoryConsole.tsx`, `transforms/workflows/workflowStore.ts`와 각 테스트, 기동 지점(`apps/devbox-api-studio/src/Studio.tsx`)

**Interfaces (Produces):**
- `interface DocumentStorage { load(kind: DocumentKind): Promise<{ revision: number; body: string } | null>; save(kind: DocumentKind, body: string, expectedRevision: number | null): Promise<number> }`
- `nativeDocumentStorage`(typed `store` command), `browserDocumentStorage(storage: Storage = localStorage)`(미리보기), `documentStorage()`(native 여부로 선택)
- `migrateLocalDocuments(target: DocumentStorage, source: Storage = localStorage): Promise<{ migrated: DocumentKind[]; failed: DocumentKind[] }>`
- `LEGACY_KEYS: Record<DocumentKind, string>`

- [ ] **Step 1: 실패하는 테스트** — `storage/migrate.test.ts`

```ts
import { describe, expect, it } from "vitest";
import { migrateLocalDocuments, LEGACY_KEYS } from "./migrate";
import type { DocumentStorage } from "./documentStorage";

function memoryStorage(fail = false): DocumentStorage & { docs: Map<string, { revision: number; body: string }> } {
  const docs = new Map<string, { revision: number; body: string }>();
  return {
    docs,
    async load(kind) { return docs.get(kind) ?? null; },
    async save(kind, body, expected) {
      if (fail) throw Object.assign(new Error("disk full"), { name: "store_unavailable" });
      const current = docs.get(kind);
      if ((current?.revision ?? null) !== expected) throw Object.assign(new Error("conflict"), { name: "store_revision_conflict" });
      const revision = (expected ?? 0) + 1;
      docs.set(kind, { revision, body });
      return revision;
    },
  };
}

describe("migrateLocalDocuments", () => {
  it("moves each v2 document once and clears localStorage", async () => {
    localStorage.clear();
    localStorage.setItem(LEGACY_KEYS.collections, '{"version":2,"entries":[]}');
    localStorage.setItem("apip-collections", "[]");
    localStorage.setItem("apip-collections-v1-migrated", "2");
    const target = memoryStorage();
    const result = await migrateLocalDocuments(target);
    expect(result.migrated).toEqual(["collections"]);
    expect(target.docs.get("collections")?.body).toBe('{"version":2,"entries":[]}');
    expect(localStorage.getItem(LEGACY_KEYS.collections)).toBeNull();
    expect(localStorage.getItem("apip-collections")).toBeNull();
    expect(localStorage.getItem("apip-collections-v1-migrated")).toBeNull();
  });

  it("keeps localStorage when the native save fails", async () => {
    localStorage.clear();
    localStorage.setItem(LEGACY_KEYS.environments, '{"environments":[]}');
    const result = await migrateLocalDocuments(memoryStorage(true));
    expect(result.failed).toEqual(["environments"]);
    expect(localStorage.getItem(LEGACY_KEYS.environments)).toBe('{"environments":[]}');
  });

  it("does not overwrite a native document that already exists", async () => {
    localStorage.clear();
    localStorage.setItem(LEGACY_KEYS.history, '{"version":2,"history":[1]}');
    const target = memoryStorage();
    await target.save("history", '{"version":2,"history":[]}', null);
    const result = await migrateLocalDocuments(target);
    expect(result.migrated).toEqual([]);
    expect(target.docs.get("history")?.body).toBe('{"version":2,"history":[]}');
    expect(localStorage.getItem(LEGACY_KEYS.history)).toBeNull();
  });
});
```

- [ ] **Step 2: 실패 확인** — Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/storage` → FAIL.

- [ ] **Step 3: 구현**
  - `storage/documentStorage.ts`: 인터페이스와 두 구현. native 구현은 P1-13의 `typedCall<StoreCall, StoreResults>("api-studio.store")`를 쓴다. browser 구현은 `localStorage`에 `devbox.api-studio.document.<kind>`(body)와 `….revision` 두 키로 저장하고 같은 CAS 규칙을 흉내 낸다.
  - `storage/migrate.ts`:

```ts
import type { DocumentKind } from "../generated/DocumentKind";
import type { DocumentStorage } from "./documentStorage";

export const LEGACY_KEYS: Record<DocumentKind, string> = {
  collections: "apip-collections-v2",
  history: "apip-history-v2",
  environments: "apip-environments",
  grpc_history: "devbox.api-playground.grpc-history/v1",
  workflows: "devbox.developer-toolbox.smart-workflows.v1",
};
const RETIRED_KEYS = ["apip-collections", "apip-collections-v1-migrated", "apip-history", "apip-history-v1-migrated"];

/** Move legacy WebView documents into the native store once. localStorage is
 * cleared only after the native copy reads back identically. */
export async function migrateLocalDocuments(target: DocumentStorage, source: Storage = localStorage) {
  const migrated: DocumentKind[] = [];
  const failed: DocumentKind[] = [];
  for (const kind of Object.keys(LEGACY_KEYS) as DocumentKind[]) {
    const key = LEGACY_KEYS[kind];
    const body = source.getItem(key);
    if (body === null) continue;
    try {
      if ((await target.load(kind)) === null) {
        await target.save(kind, body, null);
        if ((await target.load(kind))?.body !== body) throw new Error("read-back mismatch");
        migrated.push(kind);
      }
      source.removeItem(key);
    } catch {
      failed.push(kind);
    }
  }
  for (const key of RETIRED_KEYS) source.removeItem(key);
  return { migrated, failed };
}
```

  - 각 store 모듈을 `DocumentStorage`로 바꾼다: `collections.ts`의 `migrateCollections(storage)`·`saveStore(store, sanitize, storage)`는 `load("collections")`·`save("collections", JSON.stringify(safe), revision)`로. v1 이전 로직(`COLLECTION_V1_*`)은 지운다(위 `RETIRED_KEYS`가 정리). 같은 방식으로 `persistence.ts`(history), `environments.ts`, `grpc.ts`, `workflowStore.ts`, `HistoryConsole.tsx`의 직접 `localStorage` 접근을 바꾼다. 각 모듈은 마지막으로 읽은 revision을 들고 있다가 저장에 넘기고, `store_revision_conflict`면 "다른 곳에서 바뀌었습니다. 다시 불러온 뒤 저장해 주세요." 문구로 알린 뒤 다시 읽는다.
  - 기동: `apps/devbox-api-studio/src/Studio.tsx`에서 기능을 mount하기 전에 native 모드면 `await migrateLocalDocuments(nativeDocumentStorage)`를 한 번 부른다(결과의 `failed`가 있으면 상태 줄에 "일부 기존 데이터를 옮기지 못했습니다. 다음 실행에 다시 시도합니다."를 보인다).
  - 기존 테스트는 `localStorage` 대신 메모리 `DocumentStorage`를 주입하도록 고친다.

- [ ] **Step 4: 통과 확인** — Run: `pnpm --filter @devbox/api-studio-features --filter devbox-api-studio exec vitest run && pnpm --filter @devbox/api-studio-features --filter devbox-api-studio exec tsc --noEmit && ! rg -n "localStorage" packages/api-studio-features/src --glob '!**/*.test.*' --glob '!**/storage/**'` → PASS(마지막 명령 0건).
- [ ] **Step 5: 커밋** — `git add -A && git commit -m "feat(devbox-api-studio): read and write stores through the native document store"`

---

### Task 4: 백업·지원 번들 연계와 문서

- [ ] Control Center 데이터 보존(data checkpoint)이 API Studio 데이터 폴더 전체를 복사하므로 `api-store.db`(+`-wal`)도 포함되는지 `core/data_checkpoint.rs`의 대상 목록을 확인한다(폴더 단위면 변경 없음, 파일 목록이면 추가). 
- [ ] `apps/devbox-api-studio/README.md`에 "컬렉션·기록·환경은 `%LOCALAPPDATA%\com.devbox.v08.apistudio.i*\api-store.db`에 저장한다"를 적는다.
- [ ] 커밋: `git commit -am "docs(devbox-api-studio): document the native store location"`

---

### Task 5: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): v0.8.1 설치본에서 컬렉션·환경·기록을 만든 뒤 P3-01 후보 setup으로 업데이트 → 그대로 보인다. API Studio를 닫고 WebView2 데이터 폴더(`…apistudio.i*\EBWebView`)를 지운 뒤 다시 열어도 컬렉션이 남는다.
