# devbox 심층 코드 리뷰

> 기준 `main@f28f08a4` (v0.8.1 공개 후, 2026-09-23) · 대상: 앱 4개, crates 34개, packages 12개, CI·릴리스 스크립트

## 0. 총평

6주 만에 약 45만 줄(Rust·TS)을 쌓은 코드베이스치고 기초가 탄탄합니다. CSP·ammonia 살균·mermaid strict, Job Object와 WSL supervisor 기반 프로세스 수명 관리, 원본을 보존하는 마이그레이션, Git 실행 설정(fsmonitor·hook·filter) 검토는 상용 도구보다 엄격합니다.

문제는 균형입니다. 같은 사용자 권한의 악성 프로세스처럼 드문 위협은 촘촘히 막았는데, 업데이트 서명·로그·자동 저장·OneDrive 호환처럼 사용자가 먼저 부딪히는 부분은 비어 있습니다. 문자열 디스패처와 v0.7 잔재가 변경 비용을 키웠고, v0.8 전환(#562) 뒤 3일 동안 fix PR이 15개 나왔습니다.

| 지표 | 값 |
|---|---|
| Rust + TS/TSX | 약 451K줄 (TS 테스트 42K줄, Rust 테스트 모듈 517개) |
| 문서 + workthrough | 4.4MB, md 약 65K줄 (workthrough 178개) |
| CI 스크립트 | 139개 파일, 18.5K줄 |
| legacy/migration/import 이름 파일 | 27K줄, "legacy" 언급 파일 253개 |
| 로컬 Cargo target | 423GB (`~/.cache/targets/devbox`) |

### 우선순위 Top 10

| # | 분류 | 내용 | 심각도 |
|---|---|---|---|
| 1 | 보안 | 업데이트 체인 무서명 + 바이너리 코드서명 없음 (S1) | 높음 |
| 2 | 버그·개인정보 | Activity 개인정보 규칙을 입력할 수 없고, 잘못된 정규식은 수집 허용으로 동작 (B1) | 높음 |
| 3 | 호환성 | OneDrive 폴더·Dev Drive(ReFS)에서 vault·프로젝트 거부 또는 식별 오류 가능 (B7) | 높음 (실기 확인 필요) |
| 4 | 운영성 | 로깅·panic hook·심볼 없음 → 현장 오류 원인 추적 불가 (A7) | 높음 |
| 5 | 구조 | 문자열 component/method + JSON args 디스패처 (A1) | 높음 (변경 비용) |
| 6 | 버그 | 업데이트 캐시 정리 코드 없음 → 다운로드 16회 뒤 업데이트 불가 (B2) | 중 |
| 7 | 버그·UX | 업데이트할 때마다 제품 연결이 조용히 끊김 (B3) | 중 |
| 8 | 버그 | Control Center '단축키' 메뉴가 개발용 placeholder 표시 (B4) | 중 |
| 9 | 성능 | 호출마다 describe() 재호출 (메인 스레드 동기 + 파일 IO) (P1) | 중 |
| 10 | 데이터 | API Studio 컬렉션·환경·기록이 WebView localStorage에 저장 (A6) | 중 |

## 1. 검토 방법과 한계

- 문서(README·AGENTS·CONVENTIONS·아키텍처), 네 host의 IPC 진입점, product-shell·suite-runtime 경계, 업데이트, 파일시스템·비밀·프로세스 계층, 주요 feature UI를 표본으로 읽었습니다. 45만 줄 전체를 읽지는 않았습니다.
- 실행한 것은 `pnpm -F @devbox/product-shell test`(5 files, 24 tests 통과) 하나입니다. Rust 테스트·clippy와 Windows 실기(OneDrive/ReFS, 메모리, SmartScreen)는 실행하지 않았습니다.
- 아래에서 "확인"은 코드로 재현 경로를 따라간 것, "추정"은 실기 확인이 필요한 것입니다.

## 2. 잘 된 점 — 리팩토링해도 지킬 것

- **웹뷰 경계:** `default-src 'self'`로 inline script 차단. Markdown은 Rust ammonia로 살균하고 raster data URI만 허용하며 원격 이미지는 막습니다. mermaid `securityLevel: "strict"`는 테스트로 고정돼 있습니다.
- **프로세스 수명:** Job Object, WSL `--supervise` subreaper, marker 기반 그룹 종료, PTY 출력의 UTF-8 경계 carry 처리(`crates/terminal-engine/src/commands/terminal.rs:241`), DST를 고려한 cron.
- **Git 신뢰:** `core.fsmonitor`, `hooksPath`, `filter.*`, `diff.external` 같은 실행 키를 분류해 검토합니다(`apps/devbox-workspace/native/src/git_config.rs:50`).
- **데이터 보존:** WAL-consistent snapshot, native revision 조건부 저장, 원본을 바꾸지 않는 이전.
- **Suite bus:** 피어를 OS가 알려주는 pipe client PID와 이미지 경로로 식별하고 `first_pipe_instance`, `reject_remote_clients`를 적용합니다.
- **품질 인프라:** cargo-deny, pnpm audit, affected-scope CI, axe 접근성 검사, 번들 예산, exact-main 후보만 승격하는 릴리스.

## 3. 확인된 결함

### B1. Activity 개인정보 규칙: 입력할 수 없고, 실패하면 수집을 허용 (확인)

- `packages/knowledge-features/src/activity/App.tsx:1391-1417`: 입력값이 `list.join(", ")`이고 onChange마다 `split(",").map(trim).filter(Boolean)`을 적용합니다. 제어 컴포넌트라 쉼표나 끝 공백을 치는 즉시 지워집니다. 두 번째 항목도, `InPrivate - Microsoft Edge`처럼 공백이 든 패턴도 타이핑으로는 넣을 수 없고 붙여넣기만 됩니다.
- 정규식을 쉼표로 자르므로 `\d{1,3}`은 `\d{1`과 `3}`으로 깨집니다.
- `crates/activity-engine/src/core/privacy.rs:53-64`: 잘못된 정규식은 조용히 "불일치" 또는 "원문 유지"가 되어 제외·치환이 무효가 됩니다(fail-open). `parse_rules`(67행)도 JSON 오류면 빈 규칙을 씁니다.
- `crates/activity-engine/src/commands/privacy.rs:21`: 저장할 때 정규식을 검증하지 않고, `db::set_setting`(`crates/activity-engine/src/core/db.rs:239`)은 쓰기 오류를 버립니다. UI도 `void setPrivacyRules(...)`로 오류를 무시하고 키 입력마다 IPC를 보냅니다.
- `redact_existing`은 트랜잭션 없이 행 단위로 UPDATE/DELETE하면서 DB mutex를 끝까지 쥡니다.
- **수정:** 원문 텍스트를 state로 두고 줄 단위 textarea + 저장 버튼에서 파싱. 저장 시 `Regex::new`로 검증해 오류를 돌려주고 컴파일한 `RegexSet`을 캐시. 규칙을 읽지 못하면 제목 저장을 멈추는 fail-closed. 소급 적용은 트랜잭션 하나로.

### B2. 업데이트 캐시가 쌓이기만 하다가 업데이트를 막음 (확인)

- `apps/devbox-control-center/src-tauri/src/updates.rs:232-262`: 다운로드한 릴리스마다 `%LOCALAPPDATA%\com.devbox.v08.suite-downloads.i<key>\<review-id>`를 만듭니다. 저장소 전체에서 이 경로를 참조하는 곳은 여기 하나이고 삭제 코드는 없습니다.
- 폴더가 16개가 되면 새 릴리스는 `update_cache_review_required`로 계속 실패합니다. 중단된 `.partial-*`가 쌓여 한 폴더의 파일이 4개를 넘어도 같습니다. UI 문구는 "정리한 뒤 다시 시도"인데 경로도 정리 버튼도 없습니다.
- **수정:** activation commit 후 이전 캐시 삭제(현재와 직전만 보존), 시작 시 `.partial-*` 정리, "다운로드 캐시 비우기" 버튼.

### B3. 업데이트할 때마다 제품 연결이 조용히 풀림 (확인)

- `crates/suite-runtime/src/platform/connection_preference.rs:8-26`: 기억된 연결이 `generation`까지 비교합니다. 업데이트로 generation이 바뀌면 `crates/suite-runtime/src/lib.rs:299-306`에서 `suite_review_required`로 끝나고 아무것도 표시하지 않습니다.
- 결과적으로 업데이트마다 네 제품 각각에서 연결을 다시 승인해야 하고, 그 전까지 handoff·단축키·launcher 명령이 이유 없이 실패합니다.
- **수정:** installation key가 같고 Control Center가 커밋한 generation이면 자동 승계. 불일치하면 셸 배너로 알리기.

### B4. Control Center '단축키' 메뉴가 개발용 placeholder (확인)

- `apps/devbox-control-center/src/main.tsx:50`의 route 분기에 `shortcuts`가 없어 `RouteView`로 떨어집니다. 그래서 v0.8.1 사용자에게 B01 문구 "기능 이전을 준비하고 있습니다… 기존 작업과 데이터는 기존 앱에서 계속 사용할 수 있습니다"(`packages/product-shell/src/RouteView.tsx:19`)가 보입니다. 실제 단축키 설정은 '제품 연결' 토글 안(`SuiteConnection` → `ShortcutSettings`)에만 있습니다.
- `apps/devbox-control-center/src/App.test.tsx:8`은 `renderContent` 없이 셸만 렌더하고 이 placeholder 문구를 기대값으로 삼습니다. 실제 화면 조합은 검증하지 못하는 테스트입니다.
- **수정:** `shortcuts`에 `ShortcutSettings` 연결, 릴리스 빌드에서 `RouteView` 제거, "카탈로그의 모든 route가 실제 뷰를 렌더한다"는 테스트 추가.

### B5. 조기 거부 오류는 항상 "작업 상태를 확인할 수 없습니다"로 보임 (확인)

- 세 host의 `rejected` Problem은 `request_id: "rejected"`, `revision: 1`로 고정입니다(`apps/devbox-workspace/src-tauri/src/component.rs:1569`, `apps/devbox-api-studio/src-tauri/src/component.rs:153`, `apps/devbox-knowledge/src-tauri/src/component.rs:194`). 현재 catalogRevision은 13입니다.
- 프런트 `matchesProvenance`(`packages/product-shell/src/operation.ts:22`)는 완전 일치를 요구합니다. 그래서 인자 크기 초과, 허용되지 않은 메서드, 종료 중 같은 원인이 모두 `unavailable` 문구로 뭉개집니다.

### B6. 오류 분류가 한국어 문장에 의존 (확인)

- `apps/devbox-knowledge/src-tauri/src/component.rs:156-157`: `error.contains("만료")`, `contains("미리보기") && contains("오래")`로 분류합니다. `"snapshot timed out; quiesce source and retry"` 같은 영어 문장도 코드처럼 매칭합니다. 문구를 고치면 동작이 바뀝니다. 원인은 A2의 문자열 오류 모델입니다.

### B7. OneDrive·Dev Drive 호환성 (코드 확인, 실기 확인 필요)

- `crates/filesystem/src/lib.rs:141, 231-269`: 대상과 모든 상위 경로에 `FILE_ATTRIBUTE_REPARSE_POINT`가 있으면 거부합니다. [Microsoft 문서](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-rtlsetprocessplaceholdercompatibilitymode)에 따르면 대부분의 앱은 Cloud Files placeholder를 reparse point 그대로 봅니다. OneDrive 동기화 폴더의 파일·폴더가 여기에 해당하므로 Knowledge vault·노트(`crates/knowledge-vault-engine/src/core/document.rs:41,57`)와 Workspace 프로젝트가 거부될 가능성이 높습니다.
- 엔진 기본 vault는 `Documents\Knowledge`(`crates/knowledge-vault-engine/src/commands/docs.rs:128`)입니다. Windows 11에서 OneDrive 폴더 백업을 켜면 Documents가 OneDrive 아래로 옮겨집니다. junction으로 옮긴 개발 폴더(`C:\src → D:\src`)도 같은 이유로 거부됩니다.
- 파일 identity는 32비트 볼륨 serial + 64비트 file index입니다(`crates/filesystem/src/lib.rs:127-152`). [문서상](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/ns-fileapi-by_handle_file_information) ReFS에서는 이 64비트 ID가 유일하다는 보장이 없고 128비트 `FILE_ID_INFO`를 써야 합니다. Dev Drive는 ReFS 기반이라, 이 코드베이스가 TOCTOU 방어에 쓰는 identity 비교가 틀릴 수 있습니다.
- **수정:** `FILE_ATTRIBUTE_TAG_INFO`로 reparse tag를 읽어 cloud placeholder는 허용하고 symlink·mount point만 차단(또는 대상을 해석해 identity를 고정한 뒤 허용). identity는 `GetFileInformationByHandleEx(FileIdInfo)`로 교체. OneDrive·Dev Drive·junction 실기 fixture 추가.

### B8. Webhook Lab이 흔한 전송 형식을 거부 (확인)

- `crates/webhook-core/src/core/http.rs:249`: `Transfer-Encoding`과 `Expect` 헤더는 501, `:334`는 UTF-8이 아닌 본문을 400으로 거부합니다.
- 길이를 모르는 스트림 본문(Node stream, Go `io.Pipe`)의 chunked 전송, `Expect: 100-continue`를 기본으로 보내는 .NET Framework `HttpWebRequest`, gzip·protobuf 같은 바이너리 웹훅을 받을 수 없습니다.
- **수정:** 총량 제한을 유지한 chunked 디코딩, `100 Continue` 응답, 본문은 바이트로 저장하고 표시할 때만 UTF-8/hex로 분기.

그 밖에: `docs/windows-guide.md`는 `Devbox_0.8.0_x64-setup.exe`와 "Windows 11 필요"라고 적어 README(0.8.1)·CONVENTIONS(Windows 10/11)와 어긋납니다.

## 4. 보안

### S1. [높음] 업데이트 체인에 서명이 없음

- `apps/devbox-control-center/src-tauri/src/updates.rs:127-217`: 신뢰의 뿌리가 GitHub API 응답의 `digest` 필드입니다. manifest와 setup의 SHA-256이 모두 같은 GitHub Release에서 오므로, 릴리스를 올릴 수 있는 토큰·워크플로·계정 하나만 탈취되면 모든 설치본이 검증을 통과한 악성 setup을 실행합니다(`:466`).
- 워크플로에 Authenticode 서명과 artifact attestation 단계도 없습니다. SmartScreen 경고 때문에 첫 인상도 나빠집니다.
- **권고:**
  1. `release-manifest.json`을 오프라인 Ed25519(minisign) 키로 서명하고, 공개키를 Control Center에 내장해 검증. Tauri updater가 서명을 강제하는 이유와 같습니다.
  2. Authenticode 서명 + 실행 전 `WinVerifyTrust`.
  3. GitHub artifact attestation은 보조 근거로.

### S2. [중·설계] 모든 승인이 렌더러에서 이뤄짐

- Rust 쪽 네이티브 대화상자는 전부 파일 선택기이고 확인 대화상자는 하나도 없습니다. preview·apply·approve·cancel 계열 메서드 이름이 약 100개(신뢰 부여, task 실행, legacy 정리, 업데이트 실행, Suite 연결 등)인데, 같은 렌더러가 두 단계를 모두 호출할 수 있습니다. handshake 값은 `describe`로 누구나 얻으므로 XSS 방어선이 아닙니다(아키텍처 문서도 인정).
- 즉 XSS 한 건이면 임의 task 실행과 파일 조작이 가능합니다. 지금은 CSP 덕분에 가능성이 낮지만 방어가 한 겹뿐입니다.
- **권고:** 위험 상위 동작(신뢰 부여, 신뢰 전 manifest task 실행, 삭제·정리, 업데이트 실행, 외부 전송)에만 `tauri-plugin-dialog`로 네이티브 확인을 받아 사용자 존재를 증명. 저위험 동작은 오히려 확인 단계를 줄이기(§7).
- **CSP 보강:** `base-uri 'none'; form-action 'none'; object-src 'none'; frame-ancestors 'none'` 추가. `require-trusted-types-for 'script'`(WebView2는 Chromium이라 지원)로 `innerHTML` sink를 정책 하나로 모으는 방안 검토(서드파티 호환 확인 필요). 가능하면 `style-src 'unsafe-inline'` 제거.

### S3. [중] multipart가 렌더러가 준 경로를 그대로 읽어 전송

- `crates/http-client-engine/src/commands/request.rs:2487-2519`: `file_path`는 존재와 크기만 확인합니다. 같은 제품의 MCP stdio는 네이티브 dialog가 발급한 만료 opaque ID를 쓰고, Workspace Files도 선택 grant를 씁니다. 여기만 예외라 XSS가 생기면 임의 파일 유출 경로가 됩니다.
- **권고:** `pick_multipart_file`이 grant ID를 돌려주고, 전송은 grant만 허용.

### S4. [낮음] Suite named pipe가 기본 DACL로 생성됨

- `crates/suite-runtime/src/platform/component_bus.rs:56-60, 74`: 보안 설명자를 지정하지 않아 [기본 DACL](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)(Everyone·anonymous 읽기 허용)을 씁니다. 쓰기 권한이 없고 피어 이미지도 검증하므로 실제 위험은 낮습니다.
- **권고:** Microsoft 권고대로 현재 사용자(또는 logon) SID만 허용하는 명시 DACL.

### S5. [낮음] 비밀값 처리의 일관성

- DPAPI Sealer가 세 벌입니다(`crates/http-client-engine/src/platform.rs`, `crates/projects-engine/src/platform.rs`, `crates/runtime-engine/src/platform/environment.rs`). zeroize 방식과 빈 결과 처리가 제각각입니다. `crates/secrets` 주석의 "crates에 Windows 코드 금지" 전제는 engine이 crates로 옮겨오면서 이미 깨졌습니다.
- `secrets::mask("abc", 3)`은 짧은 비밀을 통째로 보여줍니다.
- Workspace 편집기 복구 버퍼는 평문 JSON인데 Knowledge는 평문 백업을 금지합니다. 제품마다 정책이 다르니 DPAPI로 암호화한 복구 저널 하나로 통일하는 편이 낫습니다(§7 자동 저장과 연결).
- Webhook LAN bind(`allow_lan`)에는 인증이 없습니다. 공유 비밀 헤더 옵션을 권합니다.

## 5. 성능·자원

### P1. 호출마다 describe() + 파일 IO (확인)

- API Studio(`apps/devbox-api-studio/src/transport.ts:15`), Knowledge(`apps/devbox-knowledge/src/transport.ts:14`), Workspace `nativeCall`(`apps/devbox-workspace/src/native.ts:205`)은 매 호출 전에 `describe()`를 다시 부릅니다.
- `describe`는 동기 command라 [Tauri v2 규칙](https://v2.tauri.app/develop/calling-rust/)상 **메인 스레드**에서 돕니다(`crates/product-shell-tauri/src/lib.rs:48`). 설치본에서는 `installation::activation`이 상위 경로 reparse 검사, manifest(최대 64KiB)와 activation 파일 열기·파싱·identity 재검증을 합니다(`crates/product-shell-tauri/src/installation.rs:13-64`). 이어지는 `authorize`도 같은 IO를 반복합니다(`crates/product-shell-tauri/src/lib.rs:203`).
- 호출 한 번에 IPC 2회와 파일 IO 2세트가 들고, 그중 일부가 UI 스레드에서 일어납니다. Knowledge 인덱싱 중의 0.5초 폴링은 초당 IPC 4회가 됩니다.
- **권고:** description은 셸이 한 번 받아 context 변경 이벤트로만 갱신(Workspace `componentCall`이 이미 이 방식). `describe`를 async로. activation 상태는 Control Center가 전이를 알리고 짧은 TTL로 캐시.

### P2. 폴링 중심 UI

- 터미널 출력은 pane마다 50ms 폴링입니다(`packages/workspace-features/src/terminal/lib/terminalReplay.ts:19`). 유휴 pane 하나가 초당 약 20회의 IPC·인증·blocking worker를 씁니다. 그 밖에 0.5–5초 `setInterval`이 20여 곳 있습니다. 상당수는 route 활성 여부로 멈추지만, 창 최소화(`document.hidden`)를 보는 곳은 `Operations.tsx` 하나입니다.
- **권고:** PTY 출력·로그 tail·작업 상태는 Tauri v2 `ipc::Channel`(공식 문서가 child process 출력 스트리밍 용도로 소개)로 push. 남는 폴링은 `usePolling({ active, visible })` 하나로.

### P3. 메모리

- v0.7 앱 기준 자체 측정에서 Tauri 앱 하나가 7프로세스, 327–354MB였습니다(`docs/architecture/v0.8-foundation.md`). v0.8 제품도 구조가 같으니 네 제품을 함께 쓰면 1.3GB 안팎으로 추정합니다. 트레이 아이콘도 제품마다 따로여서 최대 3개(Workspace·Knowledge·API Studio)입니다. §8 C안에서 다룹니다.

### P4. 빌드와 개발 생산성

- 로컬 target 423GB. `windows` crate가 0.58·0.61·0.62 세 버전이고(activity-engine·editor-engine·git이 0.58), 버전이 겹치는 크레이트가 66종이며 `[workspace.dependencies]`를 쓰지 않습니다. reqwest feature 조합도 crate마다 다릅니다.
- **권고:** workspace.dependencies로 버전·feature 통일, windows 0.61로 단일화, `cargo sweep` 주기 실행, `debug = "line-tables-only"`, worktree별 feature 조합 차이 줄이기.

### P5. 작은 것들

- 크기 검사를 위해 args를 `serde_json::to_vec`로 다시 직렬화합니다(최대 64MB, `apps/devbox-workspace/src-tauri/src/component.rs:1576-1593`). 메모리를 두 배로 씁니다.
- Webhook accept 루프가 10ms sleep 폴링이라(`crates/webhook-host/src/commands.rs:615`) 리스너가 켜져 있는 동안 초당 100번 깹니다.
- 개인정보 정규식을 창 전환 이벤트마다 새로 컴파일합니다.

## 6. 구조·설계 — 리팩토링 제안

### A1. 문자열 디스패처를 타입 IPC로

- 제품마다 Tauri command 하나(`execute`)에 `{component, method: String, args: Value}`를 실어 보내고, host의 `matches!` 허용목록 → if/else 체인 → engine의 `match method` → 수동 shim 순으로 분기합니다.
- Workspace `execute`는 약 465줄이고(`apps/devbox-workspace/src-tauri/src/component.rs:1557-2021`), 허용목록은 `migration_method`(255행)와 `allowed`(319행)에 나뉘어 있습니다.
- 메서드 하나를 추가하려면 engine 함수, `__component_*` shim, engine dispatch, host 허용목록, 프런트 문자열, 오류 사전까지 4–6곳을 고쳐야 합니다.
- 같은 저장소의 `suite-runtime`은 이미 serde tag 기반 `enum Method`를 씁니다. 좋은 선례가 안에 있습니다.
- **권고:**
  1. component별 `#[derive(Deserialize, specta::Type)] enum XxxRequest`와 응답 타입을 두고, `tauri-specta`(또는 ts-rs)로 TS를 생성해 프런트 문자열과 수기 타입을 없애기.
  2. `RouteRequest` 헤더를 꺼내 공통 admission을 수행하는 커스텀 `CommandArg`(예: `Admitted<C>`)를 두고 component별 command로 나누기. capability 파일에서 메서드 단위 ACL이 다시 살아납니다.
  3. `Pool` + Semaphore 조합과 매직 넘버(2/4/8/10/24/32/64)를 `AdmissionClass { Interactive, Control, Background }` 같은 선언형 정책으로. 프런트의 "이 component면 deadline +24초" 목록(`apps/devbox-workspace/src/native.ts:212`)도 이 정책으로 백엔드에서 결정.

### A2. 오류 모델

- `Result<T, String>`과 `&'static str`가 섞여 있어 Knowledge `issue()` 같은 재매핑 체인, 수백 줄짜리 프런트 오류 사전(`apps/devbox-workspace/src/native.ts`), 한국어 문장 매칭(B6)이 생깁니다.
- **권고:** crate별 `#[derive(thiserror::Error, Serialize, specta::Type)] enum`과 안정 코드. 문구는 프런트 i18n 카탈로그에서 코드로 찾고, "모든 코드에 문구가 있다"를 타입으로 보장.

### A3. v0.7 잔재 걷어내기

- engine에 `#[tauri::command]`가 395개 있지만 `generate_handler!`에 등록된 것은 없습니다(등록은 host 쪽 9곳뿐). 실제로는 일반 함수 + 수동 shim 385개입니다. 선언형 macro나 A1의 enum으로 바꾸고 attribute를 지우세요.
- engine에 legacy 앱 실행 경로가 남아 있습니다. 예를 들어 `crates/activity-engine/src/commands/handoff.rs:161`의 `launch_open("knowledge-base")`는 `crates/activity-engine/src/component.rs:136`에서 여전히 dispatch됩니다. Knowledge host가 앞에서 가로채서 막혀 있을 뿐이라 match arm 순서에 기대는 셈입니다. "legacy exe fallback 금지"를 코드로 보장하려면 `crates/launch`·`crates/integration`·applink argv 경로를 `legacy-v07` feature 뒤로 보내고 제품 빌드에서 끄세요.
- v0.7 이름이 그대로 남아 있습니다: `code_pad_lib`, `everything_plus_lib`, `life_log_lib`, `workbench_lib`, `run_manager_lib`, `log_lens_lib` 등. `applink` 모듈 문서도 여전히 "수신 앱 13개"를 전제합니다.
- 앱 간 통합 수단이 다섯 겹입니다: launch(외부 exe), integration snapshot, applink argv·handoff store, suite component bus, product-contract transport. suite bus와 typed reference 하나로 모으세요.
- 마이그레이션 코드는 이름에 legacy/migration/import가 든 파일만 27K줄이고, 제품 host의 핵심 경로에 섞여 있습니다. `crates/migration-v07`로 모아 Control Center만 소유하게 하고 제거 버전(예: v0.10)을 정하세요.

### A4. 중복과 의존 방향

- Windows Job Object 구현이 10곳(ports, editor, launch, runtime, git, logs, http-client, installation-tools 2곳, projects), ProcessTree가 2벌, handle wrapper가 여러 벌(`std::os::windows::io::OwnedHandle`로 대체 가능), DPAPI가 3벌입니다. `crates/win32`(Job 기반 child, 출력 상한, process identity, DPAPI, file identity) 하나로 모으면 `unsafe` 459곳 대부분이 한곳에 모입니다.
- 의존 방향이 거꾸로입니다: `crates/knowledge-vault-engine/Cargo.toml:25`가 `apps/devbox-workspace/native`에 의존합니다. WSL helper를 `crates/wsl-helper`로 올리고 두 제품이 같은 artifact를 쓰게 하세요.
- Markdown + mermaid 미리보기가 두 벌입니다(`packages/workspace-features/src/files/components/PreviewPane.tsx`, `packages/knowledge-features/src/notes/components/MarkdownPreview.tsx`). `packages/markdown-view` 하나로.

### A5. 프런트엔드

- 2,000줄 넘는 컴포넌트가 4개(terminal·files·tasks·overview의 `App.tsx`)이고, terminal `App.tsx` 한 파일에 hook 호출이 117개입니다. 전체 `useState`는 약 1,240개인데 custom hook은 11개뿐이고, 손으로 쓴 `setBusy(true)` 패턴이 133곳입니다.
- ESLint·Prettier·Biome·`.editorconfig`가 없어 서식이 섞이고 250자 넘는 줄이 많습니다. `react-hooks/exhaustive-deps` 없이 의존성 배열을 손으로 관리합니다.
- CSS는 17K줄에 hex 색상이 578회, 토큰 사용은 354회입니다. 다크 테마 전용이고 `forced-colors`(고대비)는 product-shell 한 곳만 대응합니다.
- **권고:** Biome(또는 ESLint + react-hooks)과 포맷터를 CI에 넣기. `useOperation`(busy·issue·cancel), `usePolling`, `useReviewFlow`(preview→apply→cancel 상태 머신) 공통 hook. 도메인 store(Zustand 또는 useReducer). `apps/devbox-workspace/src/Workspace.tsx:32-46`의 모듈 수준 가변 상태(`displayed`, `connected`) 제거. 토큰을 넓혀 라이트·고대비 테마.

### A6. API Studio 저장소

- 컬렉션·환경·기록이 `localStorage`에 있습니다(`packages/api-studio-features/src/requests/lib/collections.ts:95`, `environments.ts:31`, `persistence.ts`). 5–10MB 할당량, WebView 프로필 초기화 시 유실, 백업·diff·공유 불가 문제가 있고, v0.7→v0.8 때 export 전용 창까지 만들어야 했던 원인입니다. 네 제품이 같은 origin(`tauri.localhost`)을 쓰기 때문에 WebView2 데이터 폴더를 합쳐 메모리를 줄이는 방법도 막힙니다.
- **권고:** 메타데이터는 native SQLite, 컬렉션은 선택적으로 **요청당 파일**(Bruno 방식 YAML/JSON)로 저장해 Git으로 공유. 비밀은 DPAPI 환경에만.
- 모델도 정리하세요. `AuthConfig`는 모든 인증 방식의 필드를 한 구조체에 담았고(`crates/http-client-engine/src/commands/request.rs:120`), `body_kind`·`kind`·`method`가 문자열입니다. tagged enum으로 바꾸세요.

### A7. 관측성

- `log`·`tracing`·`tauri-plugin-log` 의존성과 panic hook이 하나도 없습니다(CONVENTIONS §3에는 `log`/`env_logger`가 표준으로 적혀 있음). 릴리스는 GUI subsystem이라 `eprintln!` 29곳도 아무 데도 남지 않고, `strip = true`라 심볼도 없습니다.
- 사용자는 "작업을 완료하지 못했습니다"만 보고 원인 기록은 어디에도 없습니다. spawn된 작업의 panic은 `worker_unavailable`로 삼켜집니다.
- **권고:** `tracing` + 일 단위 rolling 파일(내용·경로·비밀은 빼고 코드·component·method·지연시간만), panic hook(버전과 최근 이벤트 N개), 릴리스 PDB 보관, Control Center 진단 번들에 포함.

### A8. 문서와 개발 프로세스

- 6주 동안 md 약 65K줄, workthrough 178개가 쌓였습니다. 앱 README는 사용법보다 계약·수용 조건 문장이 중심이고, 에이전트 컨텍스트 비용도 큽니다.
- AGENTS/CONVENTIONS의 "구현이 끝나기 전 test·clippy 실행 금지" 정책은 자원을 아끼려는 의도는 이해되지만, 전환(#562) 직후 3일 동안 fix PR이 15개 나왔습니다. 바꾼 파일 단위의 빠른 테스트(`cargo test -p <crate> <module>`, `vitest related`)는 허용하고 전체 affected 검증만 PR 끝에 두면 결함을 더 일찍 잡습니다.
- **권고:** workthrough·history는 별도 저장소나 Wiki로 옮기고 결정은 ADR 10–20개로 남기기. 스크린샷과 기능 목록이 있는 사용자 가이드 신설.

## 7. 사용성(UX)

- **승인 피로:** preview·apply·approve·cancel 계열이 약 100개입니다. 위험도로 나누세요. 되돌릴 수 있는 동작(프로젝트 선택, 설정 저장)은 바로 실행하고 Undo를 주고, 파괴적이거나 외부에 영향이 있는 동작만 확인받기(S2의 네이티브 확인과 함께).
- **제품 연결:** 설치 직후 네 제품에서 각각 수동 승인해야 하고, 업데이트마다 다시 해야 합니다(B3). 같은 Suite 설치가 만든 제품은 기본으로 연결하고 설정에서 끌 수 있게.
- **첫 실행:** Workspace는 "빈 Workspace 시작" 버튼과 legacy 가져오기 패널부터 보여줍니다. v0.7 데이터가 없으면 자동으로 시작하고, 마이그레이션은 감지됐을 때만 배너로.
- **자동 저장:** Knowledge Notes에는 자동 저장도 크래시 복구도 없습니다(README에 명시). 노트 앱의 기본 기대치이니, debounce 자동 저장(원본 파일에 조건부 저장)과 DPAPI 복구 저널을 권합니다.
- **오류 문구:** 두루뭉술한 문구가 많고(예: SuiteConnection의 "설치 폴더와 제품 버전을 확인해 주세요…"), B5 때문에 더 뭉개집니다. 코드별 원인과 다음 행동, "진단 복사" 버튼을 주세요.
- **트레이:** 최대 3개를 Devbox 아이콘 하나와 제품 메뉴로.
- **테마·언어:** 다크 전용, 한국어 하드코딩입니다. 공개 저장소라면 영어 UI(i18n 카탈로그)와 라이트·고대비 테마를 권합니다.

## 8. 제품 구성 — 더 쪼갤까, 합칠까

GUI 앱을 더 쪼개는 것은 권하지 않습니다. v0.7의 15개 앱에서 겪은 설치·이전·연결·중복 비용이 되돌아오고, 앱 하나마다 WebView2 프로세스 묶음이 붙습니다. 대신 **수명(lifecycle)을 기준으로 하나를 떼어내기**를 권합니다.

| 안 | 내용 | 장점 | 단점 |
|---|---|---|---|
| A. 현행 4개 유지 | §3–7만 개선 | 위험 최소 | 수명·메모리·연결 문제는 남음 |
| B. 단일 프로세스 | exe 1개, 창 4개 | 메모리 절감, 연결 절차 불필요 | 장애 격리 약화, 현재 설계 상당 부분 폐기 |
| **C. Agent + UI (권장)** | headless `devbox-agent`(사용자별) + UI 4개 | 터미널·작업·서비스가 UI 재시작·크래시에도 유지, UI를 닫아도 스케줄 실행, 권한 주체 하나, 연결 승인 불필요, Channel 스트리밍 | agent 설계와 업데이트 경로 필요 |

```
devbox-agent  (트레이 1개, 자동 시작, 비정상 종료 시 재시작)
 ├─ PTY 세션 · Runtime(Tasks/Services/Scheduler) · Webhook 리스너
 ├─ Activity 수집 · 검색 인덱서 · 파일 watcher
 └─ Secrets · 권한 · Suite bus (단일 authority)
        ▲ 타입 RPC (named pipe, 사용자 SID DACL, 스트리밍)
Workspace UI · API Studio UI · Knowledge UI · Control Center UI   (닫아도 작업 유지)
```

이미 있는 "UI 숨김 ≠ PTY 종료", `--background`, 트레이, supervisor 설계가 이 방향과 맞습니다. Terminal → Runtime → Webhook 순으로 옮기면 됩니다. Control Center는 쓰는 빈도가 낮으니 **Launcher(명령 팔레트·빠른 호출)를 agent가 소유하는 상시 진입점**으로 올리고, Control Center는 설치·업데이트·진단 전용으로 가볍게 두세요.

### 신규 기능·앱 후보 (가치 대비 비용 순)

| 순위 | 후보 | 이유 |
|---|---|---|
| 1 | **Agent Hub** (Workspace route) | 작업마다 worktree → Claude Code/Codex 터미널 → diff 리뷰 → 병합. worktree·터미널·diff·Git이 이미 있어 조립 비용이 낮습니다. 에이전트 프로세스 트리의 CPU·RAM·PSI와 토큰 사용량 위젯(`wsl-resource-guard`, `llm-usage-dashboard`와 같은 관심사) |
| 2 | **Devbox MCP 서버** | 노트 검색, task·log·problem 조회, 승인된 task 실행을 AI 에이전트 도구로 노출. 읽기가 기본, 변경은 네이티브 승인. API Studio는 이미 MCP 클라이언트 |
| 3 | **WSL Control** (Control Center) | `.wslconfig` 편집, 메모리·swap·OOM 모니터, vhdx 압축, 배포판 백업·복원, mirrored 네트워킹 포트 보기 |
| 4 | **Secrets & Environments** | DPAPI 3벌을 한 저장소로, `.env` 가져오기, 터미널·작업·API 환경에 참조 주입, 사용처 목록 |
| 5 | API Studio 보강 | curl 붙여넣기 import, Postman·Insomnia·Bruno·HAR import, 파일 컬렉션, assertion과 collection runner, 요청 체이닝, OAuth2(PKCE, client credentials)·NTLM/Negotiate, HTTP mTLS·사용자 CA·검증 끄기(지금은 gRPC에만 인증서 설정), 코드 생성, 웹훅용 터널(cloudflared 선택 연동) |
| 6 | Workspace Git 보강 | 지금은 status·diff·stage·commit·fetch/pull/push·worktree까지. branch 생성·전환, stash, hunk 단위 discard, amend, blame, 3-way 충돌 해결, `gh` PR 연동 |
| 7 | Knowledge 보강 | 자동 저장, OneDrive vault, 전역 quick capture, 백링크·그래프, 프로젝트별 노트 자동 연결, ADR 템플릿 |

## 9. 단계별 로드맵

- **Phase 0 (1–2주, 결함·고위험):** B1–B5 수정, manifest 서명(S1-1), multipart grant(S3), describe 1회화(P1), `tracing` + panic hook(A7), OneDrive·Dev Drive 실기 조사(B7), Webhook chunked/Expect(B8).
- **Phase 1 (1–2개월, 구조):** 가장 작은 Knowledge에서 typed IPC + 오류 enum + TS 생성(A1·A2)을 시범 적용한 뒤 확대. engine attribute·shim 제거와 legacy feature gate(A3). `crates/win32`·`crates/wsl-helper`(A4). workspace.dependencies·lint·공통 hook(A5·P4). Channel 스트리밍(P2).
- **Phase 2 (분기, 제품):** API Studio native 저장소(A6), `devbox-agent` 분리(§8 C안), Agent Hub·MCP 서버, Authenticode.
