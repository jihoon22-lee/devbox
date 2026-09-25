# P2-03 웹훅 리스너를 agent로 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4와 ADR 0015를 읽는다.

**Goal:** API Studio의 웹훅 리스너·기록·fixture·응답 규칙·재전송을 `devbox-agent`로 옮겨 API Studio 창을 닫아도 리스너가 계속 받게 하고, API Studio의 "닫을 때 계속 듣기" 트레이 동작을 없앤다(D24).

**Architecture:** `webhook_host` engine이 데이터 폴더를 `app_local_data_dir()`에서 직접 찾던 부분을 인자로 받게 바꾸고(`initialize_at(app, data_dir)`), agent가 API Studio 데이터 폴더(`com.devbox.v08.apistudio.i<접미사>`)로 초기화한다. API Studio host의 `webhooks` command(P1-13)는 engine 메서드(`WebhookCall`)를 agent로 전달하고, host 전용 메서드(모의 응답 초안, API·Log Lens로 보내기, 창 수명)는 API Studio에 남는다. 보내기 메서드는 agent에서 fixture·기록 투영을 받아 온 뒤 기존 handoff 발행을 한다. 닫기 정책: "계속 듣기"가 기본이 되고, "닫을 때 멈추기"를 고르면 API Studio 종료 때 agent에 `stop_server`를 보낸다.

**Tech Stack:** Rust(Tauri v2), `agent-client`

**Spec:** ADR 0015 · `00-roadmap.md` D24

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 리스너 소유자는 하나: agent를 쓸 수 있으면 API Studio는 `webhook_host`를 초기화하지 않는다(P2-02와 같은 소유 판정, 연결 실패 시 로컬 대체 금지).
- 기록(history)은 메모리 보관이라 agent 재시작 때 사라진다(지금 API Studio 재시작과 같은 동작). fixture·규칙은 파일이라 유지된다.
- `api_studio` service worker(런타임이 띄우는 모의 서버)는 바꾸지 않는다.

## Review Focus

1. 리스너를 켠 채 API Studio를 닫고 `curl`로 요청 → agent가 받아 기록하고, 다시 연 API Studio에 그 요청이 보인다. (Task 3 실기)
2. "닫을 때 멈추기" 정책 → API Studio 종료와 함께 리스너가 멈춘다. (Task 2 테스트)
3. 기록을 API 요청으로 보내기·Log Lens로 보내기 → agent에서 투영을 받아 기존과 같은 handoff가 만들어진다. (Task 2)
4. portable → 기존처럼 API Studio 안에서 리스너가 돈다. (Task 2)
5. agent가 죽은 동안 들어온 요청은 받지 못한다(포트가 닫힘) — 다시 시작한 agent가 이전 설정(포트·LAN 허용)으로 리스너를 자동 재시작한다(설정이 "켜짐"이었을 때). (Task 1)

## Branch · PR

- 묶음: **B9** — 브랜치 `feat/suite/devbox-agent`, PR 제목 `feat(suite): run runtime, webhooks and collectors in devbox-agent`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(suite): keep the webhook listener running in devbox-agent`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: engine 데이터 폴더 인자화와 자동 재시작 설정

**Files:** `crates/webhook-host/src/{component.rs,commands.rs}`

**Interfaces (Produces):** `webhook_host::component::initialize_at(app: &AppHandle, data_dir: PathBuf) -> Result<(), String>`(기존 `initialize(app)`는 `initialize_at(app, app_local_data_dir)`의 래퍼), 리스너 설정 파일 `listener.json { enabled, port, allowLan }`(시작·중지 때 저장)과 `resume_listener(app)`(enabled면 같은 설정으로 시작)

- [ ] **Step 1: 실패하는 테스트** — `commands.rs` 테스트 모듈

```rust
    #[test]
    fn listener_settings_round_trip_and_resume_only_when_enabled() {
        let dir = tempfile::tempdir().unwrap();
        save_listener_settings(dir.path(), &ListenerSettings { enabled: true, port: 8787, allow_lan: false }).unwrap();
        assert_eq!(load_listener_settings(dir.path()).unwrap(), Some(ListenerSettings { enabled: true, port: 8787, allow_lan: false }));
        save_listener_settings(dir.path(), &ListenerSettings { enabled: false, port: 8787, allow_lan: false }).unwrap();
        assert!(!should_resume(&load_listener_settings(dir.path()).unwrap()));
        assert!(load_listener_settings(&dir.path().join("missing")).unwrap().is_none());
    }
```

- [ ] **Step 2: 구현** — `fixture_path(app)` 등 `app_local_data_dir()`를 쓰던 곳을 상태에 저장한 `data_dir`로 바꾼다. `start_server` 성공 시 `enabled: true`와 포트·LAN 설정을, `stop_server` 시 `enabled: false`를 원자적으로 저장한다(`devbox_filesystem::atomic_write`). `resume_listener(app)`는 설정이 enabled면 `start_server`와 같은 경로로 시작하고 실패하면 `enabled`를 그대로 둔 채 상태에 오류를 남긴다(다음 `server_status`에서 보인다).
- [ ] **Step 3: 확인·커밋** — Run: `source ~/.cargo/env && cargo test -p devbox-webhook-host --lib` → PASS. `git commit -am "refactor(devbox-api-studio): let the webhook host run from a given data folder"`

---

### Task 2: agent 소유와 API Studio 전달

**Files:** `apps/devbox-agent/src/{webhooks.rs,routes.rs,lib.rs}`, `apps/devbox-api-studio/src-tauri/src/{ipc/webhooks.rs,ipc/mod.rs,lifecycle.rs}`, `packages/api-studio-features/src/webhooks/*`(닫기 정책 문구)

- [ ] **Step 1: 실패하는 테스트**
  - agent `routes.rs`: `assert!(routes.accepts("api-studio", "api-studio.webhooks")); assert!(!routes.accepts("workspace", "api-studio.webhooks"));`
  - API Studio `ipc/webhooks.rs`: 소유 판정 테스트(P2-02 Task 3과 같은 `decide_owner` 표)와 "보내기 메서드는 agent에서 fixture를 받아 온 뒤 로컬에서 발행한다"를 transport 주입으로 확인:

```rust
    #[tokio::test]
    async fn sending_a_history_item_fetches_its_projection_from_the_owner() {
        let owner = FakeOwner::with_history_projection(serde_json::json!({"method":"POST","url":"/hook","headers":[],"body":"{}","receivedAtMs":1}));
        let published = send_history_to_api_with(&owner, 3, |fixture| Ok(fixture.url.clone())).await.unwrap();
        assert_eq!(published, "/hook");
        assert_eq!(owner.calls(), vec![("masked_fixture_projection", 3)]);
    }
```

  (`masked_fixture_projection`은 agent가 노출하는 새 engine 메서드로, 기록 id를 받아 `fixture_from_request` 결과를 돌려준다. 기존 `send_history_to_api` 내부가 로컬 history에서 하던 일을 소유자에게 묻는 형태로 나눈다.)
- [ ] **Step 2: 구현**
  - agent `webhooks.rs`: API Studio 데이터 폴더로 `initialize_at`, 이어서 `resume_listener`. 라우트 `api-studio.webhooks`(API Studio peer만), dispatch는 `webhook_host::api::dispatch` + 새 메서드 `masked_fixture_projection { historyId }`.
  - API Studio `ipc/webhooks.rs`: engine 메서드는 `decide_owner`가 Agent면 agent로 전달, Local이면 기존. host 메서드 `send_history_to_api`·`send_fixture_to_api`·`send_history_to_log_lens`·`send_fixture_to_log_lens`는 fixture 투영을 소유자에게서 받아 오고 handoff 발행은 로컬에서 한다. `ipc/mod.rs` plugin setup에서 Local일 때만 `webhook_host::component::initialize`.
  - `lifecycle.rs`: 닫기 정책을 "닫아도 계속 듣기(기본)"·"닫을 때 멈추기" 두 개로 두고, 앞의 것은 트레이 없이 창을 닫고 제품 종료(리스너는 agent에 남음), 뒤의 것은 종료 전 `stop_server`를 소유자에게 보낸다. API Studio 트레이 아이콘 생성 코드를 지운다(Local 모드에서는 기존처럼 창을 닫으면 리스너도 멈춘다고 안내).
  - 프런트: 닫기 정책 설정 문구를 새 두 가지로 바꾼다.
- [ ] **Step 3: 확인·커밋** — Run: `cargo test -p devbox-agent -p devbox-api-studio --lib && pnpm --filter @devbox/api-studio-features --filter devbox-api-studio exec vitest run` → PASS. `git add -A && git commit -m "feat(suite): serve webhooks from devbox-agent"`

---

### Task 3: PR 완료

- [ ] candidate acceptance에 "리스너 시작 → API Studio 종료 → curl 요청 → API Studio 다시 열기 → 기록 확인" 단계를 추가한다(`windows-api-workflow.mjs` 또는 해당 스크립트).
- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): Review Focus 1·2·5를 설치본에서 확인.
