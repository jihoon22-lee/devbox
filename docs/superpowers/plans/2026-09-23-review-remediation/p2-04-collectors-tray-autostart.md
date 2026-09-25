# P2-04 활동 수집·검색 색인을 agent로, 트레이 하나, 로그인 자동 시작 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4와 ADR 0015를 읽는다.

**Goal:** Knowledge의 활동 기록 수집기와 검색 색인·파일 감시를 `devbox-agent`로 옮겨 Knowledge 창 없이도 돌게 하고, 제품마다 있던 트레이 아이콘(Workspace·API Studio·Knowledge)을 agent 트레이 하나로 합치며, "로그인할 때 백그라운드 서비스 시작" 설정 하나로 자동 시작을 정리한다(D24, 리뷰 §7 트레이, P3).

**Architecture:**
- Knowledge 저장소 선택(`core/stores.rs`의 manifest: notes·activity·search 세대 폴더)을 `crates/knowledge-stores`로 옮겨 agent도 읽는다. agent는 Knowledge 데이터 폴더(`com.devbox.v08.knowledge.i<접미사>`)의 manifest로 activity·search 저장소 폴더를 찾아 `activity_engine::component::initialize`, `content_index_engine::component::initialize`를 부른다. 노트(vault)·노트 폴더 변경·빠른 기록은 Knowledge UI에 남는다.
- Knowledge host의 `activity`(P1-11)·`search`·`search_settings`·`opener`(P1-12) command는 engine 메서드를 agent로 전달한다(소유 판정은 P2-02와 같음). Knowledge가 아직 저장소를 만들지 않았으면 agent는 수집기를 시작하지 않고 `knowledge_store_missing`을 돌려준다.
- 트레이: agent가 트레이 아이콘 하나를 띄운다(메뉴: 제품 네 개 열기, 활동 기록 일시중지/재개, 모든 백그라운드 작업 멈추고 종료). 제품의 트레이 코드와 "닫으면 트레이로" 정책을 지운다(창을 닫으면 제품 UI는 종료, 백그라운드 일은 agent에 남음).
- 자동 시작: HKCU `Run`의 `Devbox Agent` 값 하나(`"<generation>\products\control-center\resources\suite\devbox-agent.exe" --autostart`). 설정 위치는 Control Center(설정 화면 한 줄)와 Knowledge 활동 설정(같은 설정을 보여 줌). activity engine의 제품 자동 시작 코드(`commands/autostart.rs`)는 agent용으로 옮기고 Knowledge 제품 자동 시작 값은 지운다.

**Tech Stack:** Rust(Tauri v2 tray, winreg 또는 기존 `windows` 레지스트리 코드), TypeScript

**Spec:** ADR 0015 · `00-roadmap.md` D24 · `review.md` §5 P3, §7

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 활동 수집 동의(collection consent)는 그대로 Knowledge 설정에 저장되고 agent가 읽는다. 동의가 없으면 agent도 수집하지 않는다.
- 업데이트 때 기존 Knowledge 자동 시작 값(`Devbox Knowledge` 등 activity engine이 만든 이름)이 있으면 지우고 agent 값 하나만 남긴다(한 번만, 업데이트 뒤 첫 agent 시작 때).
- 트레이 메뉴 문구: "Workspace 열기", "API Studio 열기", "Knowledge 열기", "Control Center 열기", "활동 기록 일시중지"/"활동 기록 다시 시작", "백그라운드 작업 모두 멈추고 종료".

## Review Focus

1. Knowledge를 닫아도 활동 기록이 계속 쌓이고, 다시 연 Knowledge의 오늘 화면에 닫힌 동안의 세션이 보인다. (Task 4 실기)
2. 트레이 "활동 기록 일시중지" → 수집이 멈추고 Knowledge 화면에 "일시중지됨"이 보인다. (Task 3 테스트)
3. 로그인 자동 시작을 켠 뒤 재부팅 → 제품을 열지 않아도 agent가 떠 예약 작업·웹훅·활동 기록이 동작한다. (Task 4 실기)
4. Knowledge 저장소가 아직 없는 새 설치 → agent는 수집·색인을 시작하지 않고 오류 없이 기다리며, Knowledge가 저장소를 만든 뒤 다음 호출에서 시작한다. (Task 2 테스트)
5. 트레이 "모두 멈추고 종료" → 런타임 작업·리스너·수집기가 정리되고 agent가 끝난다. 제품이 열려 있으면 "백그라운드 서비스 연결 안 됨"이 보인다. (Task 3)

## Branch · PR

- 묶음: **B9** — 브랜치 `feat/suite/devbox-agent`, PR 제목 `feat(suite): run runtime, webhooks and collectors in devbox-agent`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(suite): move collectors to devbox-agent with one tray and one autostart`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: `crates/knowledge-stores`

- [ ] `apps/devbox-knowledge/src-tauri/src/core/stores.rs`(P1-03에서 `StoreKind`·`validate_store`가 들어간 파일)를 `crates/knowledge-stores/src/lib.rs`로 옮기고 Knowledge는 이 crate를 쓴다. 기존 테스트를 함께 옮긴다. 활동 수집 동의 설정을 읽는 함수(`collection_consent(root) -> Result<bool, String>`; Knowledge lifecycle·activity 설정이 저장하는 위치를 읽음)를 이 crate에 추가하고 테스트한다:

```rust
    #[test]
    fn consent_defaults_to_off_and_reads_the_saved_value() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!collection_consent(dir.path()).unwrap());
        save_collection_consent(dir.path(), true).unwrap();
        assert!(collection_consent(dir.path()).unwrap());
    }
```

  (동의가 지금 activity DB 설정 표에 있다면, crate는 activity 저장소 DB를 읽기 전용으로 열어 그 값을 읽는다. 저장 위치는 `rg -n "consent" crates/activity-engine apps/devbox-knowledge`로 확인한다.)
- [ ] Run: `source ~/.cargo/env && cargo test -p knowledge-stores && cargo test -p devbox-knowledge --lib` → PASS. 커밋: `git commit -am "refactor(devbox-knowledge): share store selection with devbox-agent"`

---

### Task 2: agent 수집기와 Knowledge 전달

**Files:** `apps/devbox-agent/src/{collectors.rs,routes.rs,lib.rs}`, `apps/devbox-knowledge/src-tauri/src/ipc/{activity.rs,search.rs,mod.rs}`, `apps/devbox-knowledge/src-tauri/src/startup.rs`

- [ ] **Step 1: 실패하는 테스트** — agent `collectors.rs`

```rust
    #[test]
    fn collectors_wait_for_a_knowledge_store_and_consent() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(plan(dir.path()).unwrap(), CollectorPlan::WaitForStore);
        let manifest = knowledge_stores::create_empty(dir.path()).unwrap();
        assert_eq!(plan(dir.path()).unwrap(), CollectorPlan::IndexOnly { search: knowledge_stores::directory(dir.path(), &manifest, "search").unwrap() });
        knowledge_stores::save_collection_consent(dir.path(), true).unwrap();
        assert!(matches!(plan(dir.path()).unwrap(), CollectorPlan::Full { .. }));
    }
```

- [ ] **Step 2: 구현**
  - agent `collectors.rs`: `plan(knowledge_root)`로 시작할 것을 정하고, `Full`이면 activity·content index 둘 다, `IndexOnly`면 content index만 초기화한다. 저장소가 없으면 5분마다가 아니라 **Knowledge가 요청을 보낼 때** 다시 계획한다(Knowledge의 `setup.start_empty` 뒤 첫 activity·search 요청이 계획을 다시 돌린다).
  - agent 라우트: `knowledge.activity`, `knowledge.search`, `knowledge.search-settings`, `knowledge.opener`의 engine 메서드(Knowledge peer만). host 전용 메서드(활동 digest를 Knowledge 초안으로 보내기, 검색 결과를 Workspace로 보내기, `project_provider` 갱신이 필요한 호출)는 Knowledge host에 남는다: Knowledge host가 먼저 자기 쪽 준비(`project_provider::refresh`)를 한 뒤 engine 호출만 agent로 보낸다.
  - Knowledge `startup.rs`: Agent 소유일 때 `activate_with_owner`에서 `activity_engine`·`content_index_engine` 초기화를 건너뛴다(notes engine은 그대로 Knowledge에서 초기화).
- [ ] **Step 3: 확인·커밋** — Run: `cargo test -p devbox-agent -p devbox-knowledge --lib` → PASS. `git add -A && git commit -m "feat(suite): collect activity and index search in devbox-agent"`

---

### Task 3: 트레이 하나와 제품 트레이 제거

**Files:** `apps/devbox-agent/src/tray.rs`, `apps/devbox-agent/Cargo.toml`(`tauri` `tray-icon` feature — workspace 합집합에 이미 있음), `apps/devbox-workspace/src-tauri/src/component.rs`(`setup_runtime_tray`), `apps/devbox-knowledge/src-tauri/src/lifecycle.rs`, `apps/devbox-api-studio/src-tauri/src/lifecycle.rs`, 관련 프런트 설정 화면

- [ ] **Step 1: 실패하는 테스트** — `tray.rs`의 메뉴 구성·동작 매핑을 순수 함수로

```rust
    #[test]
    fn tray_menu_lists_products_pause_and_quit() {
        let items = menu_items(ActivityState::Recording);
        let labels: Vec<_> = items.iter().map(|item| item.label).collect();
        assert_eq!(labels, vec!["Workspace 열기", "API Studio 열기", "Knowledge 열기", "Control Center 열기", "활동 기록 일시중지", "백그라운드 작업 모두 멈추고 종료"]);
        assert_eq!(menu_items(ActivityState::Paused)[4].label, "활동 기록 다시 시작");
        assert_eq!(action_for("open-knowledge"), Some(TrayAction::Open("knowledge")));
    }
```

- [ ] **Step 2: 구현** — 제품 열기는 같은 generation의 `products/<p>/devbox-<p>.exe`를 실행한다(이미 떠 있으면 단일 인스턴스 plugin이 창을 앞으로 가져온다). 일시중지는 activity engine의 추적 중지·재개(`stop_tracking`/`start_tracking`)를 부른다. 종료는 런타임 종료(P2-02) → 리스너 중지(P2-03) → 수집기 중지 → `app.exit(0)`. Workspace `setup_runtime_tray`, Knowledge·API Studio lifecycle의 트레이 생성·"닫으면 트레이로" 정책·관련 설정 UI를 지운다. 창을 닫으면 제품 프로세스가 끝난다(Knowledge는 기존 종료 전 저장 확인 흐름 유지).
- [ ] **Step 3: 확인·커밋** — Run: `cargo test -p devbox-agent --lib && cargo test -p devbox-workspace -p devbox-knowledge -p devbox-api-studio --lib && pnpm -r --workspace-concurrency 2 exec vitest run` → PASS. `git add -A && git commit -m "feat(suite): one tray icon owned by devbox-agent"`

---

### Task 4: 로그인 자동 시작

**Files:** `apps/devbox-agent/src/autostart.rs`(activity engine `commands/autostart.rs`의 Run 키 코드를 옮김), agent 라우트 `agent.settings`(`autostart_status`, `set_autostart`), Control Center 설정 화면(`Commands.tsx` 옆 또는 `Tools`의 환경 화면에 한 줄), Knowledge 활동 설정의 자동 시작 토글

- [ ] **Step 1: 실패하는 테스트** — `autostart.rs`(레지스트리를 trait으로 추상화)

```rust
    #[test]
    fn enabling_writes_one_agent_value_and_removes_the_old_knowledge_value() {
        let registry = FakeRunKey::with([("Devbox Knowledge", "\"C:\\old\\devbox-knowledge.exe\" --background")]);
        set_autostart(&registry, Path::new("C:\\suite\\generations\\g\\products\\control-center\\resources\\suite\\devbox-agent.exe"), true).unwrap();
        assert_eq!(registry.values(), vec![("Devbox Agent".to_string(), "\"C:\\suite\\generations\\g\\products\\control-center\\resources\\suite\\devbox-agent.exe\" --autostart".to_string())]);
        set_autostart(&registry, Path::new("C:\\x.exe"), false).unwrap();
        assert!(registry.values().is_empty());
    }
```

  (옛 값 이름은 activity engine `product_owner`가 쓰던 이름으로 맞춘다.)

- [ ] **Step 2: 구현** — `--autostart`로 시작한 agent는 런타임 예약·리스너 재시작·수집기를 시작하고 제품 창은 열지 않는다. 업데이트로 generation 경로가 바뀌면 새 agent가 시작할 때 Run 값의 경로를 자기 경로로 고친다(값이 있을 때만). Control Center·Knowledge 설정은 `agent.settings`를 부른다(agent가 없으면 띄운다). activity engine의 `autostart_status`·`set_autostart` 메서드를 지우고 Knowledge 화면은 agent 설정을 쓴다(P1-11 enum·생성 타입 갱신).
- [ ] **Step 3: 확인·커밋** — Run: `cargo test -p devbox-agent -p devbox-activity-engine --lib && bash .github/scripts/check-generated-bindings.sh && pnpm --filter devbox-control-center --filter @devbox/knowledge-features exec vitest run` → PASS. `git add -A && git commit -m "feat(suite): start devbox-agent at sign-in from one setting"`

---

### Task 5: PR 완료

- [ ] `docs/windows-guide.md`에 "백그라운드 서비스" 절을 추가한다: 제품 창을 닫아도 작업·서비스·예약 실행, 웹훅 리스너, 활동 기록과 검색 색인이 계속 돌고, 알림 영역의 Devbox 아이콘에서 제품 열기·활동 기록 일시중지·모든 백그라운드 작업 종료를 할 수 있으며, 로그인 자동 시작은 Control Center 설정에서 켠다(기본 꺼짐).
- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): Review Focus 1·3·5, 작업 표시줄 알림 영역에 Devbox 아이콘이 하나만 있음, 각 제품을 닫으면 프로세스가 끝남(작업 관리자).
