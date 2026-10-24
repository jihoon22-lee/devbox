# S5 개발 도구 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

- 상태: **구현·필수 CI·영향 설치 수용 완료, 최종 근거 감사 APPROVE·PR #645 a55907a2 머지 완료** · 최종 제품 `b18bc740`, 브랜치 `v1/s5-tools`, worktree `/home/jihoon/projects/devbox-wt/v1-s5-tools`. 현재 소스 설치1·2 PASS, 과거429 소스3·4 PASS는 역사 근거, 5·6 NOT_RUN 유지. 정상 리뷰3회·차단 예외1건 수정과 독립 승인 완료. 진행 원장은 PROGRESS.md다.
- 상세화 수준: S2까지 실제로 머지된 코드의 이름·시그니처로 S5-2–S5-9의 실패 테스트 → 실패 확인 → 구현 → 통과 → 별도 검토·커밋 순서를 확정했다. 코드 조각은 시작점이며 각 과제의 "핵심 테스트" 전부가 완료 조건이다. 계획 코드는 컴파일해 본 것이 아니다. 같은 의도로 고친 것은 PROGRESS §5에 적는다(00 §3.5).
- 원래 과제 수준 계획은 [07-s4-s6.md](07-s4-s6.md)의 S5 절이었다. 이 문서가 S5의 실행 원장이고 07에는 작업 묶음 표와 링크만 남긴다.

**Goal:** 도구 섹션에서 API 요청(HTTP·GraphQL·WebSocket·SSE·gRPC·MCP), 웹훅·모의 서버, 변환 도구 21종, 개발 환경 점검·도구 설치·지원 번들을 제공한다. v0.9.0 API Studio의 검증된 TS 로직과 테스트를 이식하고 CV-F04·F05, AR-F03·F06·F07·F08·F09, AU-AS-01·02·07을 회귀 시험으로 고정한다.

**Architecture:**
- `crates/api`(새 도메인 crate): HTTP·GraphQL 송신, WS·SSE, gRPC, MCP 클라이언트, 인증·쿠키·TLS, 변수·비밀 해석, 기록(`api-history/` 파일 + `index.db`의 `api` 도메인), 컬렉션 파일 읽기·쓰기·감시. 송신은 `stream` 메서드 하나가 소유하고 화면이 스트림을 닫으면 취소된다.
- `crates/webhooks`(새 도메인 crate): hyper 1 리스너, 수신 기록, 응답 규칙(순서 응답·지연), fixture. 재전송은 화면이 받은 요청을 `RequestSnapshot`으로 바꿔 `api.send`로 보낸다.
- `crates/tools`(새 도메인 crate): 개발 환경 점검(기존 `system.doctor` 선언을 그대로 쓰고 로직만 이 crate로 옮김), 설치 명령 표, 지원 번들.
- 화면 `app/src/features/tools/**`: 요청 편집·응답 보기·컬렉션 트리·실행기·가져오기·코드 생성(이식한 TS lib), 웹훅, 변환 도구 21종(TS, 해시·HMAC는 WebCrypto), 개발 환경 점검. 실행기·assertion·캡처·가져오기·코드 생성은 TS에 둔다(01 §9.3 이식 표와 07의 결정을 따른다).
- 동결 경로(`crates/core`·`crates/protocol`·`app/src/ui`·`app/src/rpc`(gen 밖)·`.github`·`scripts`)는 고치지 않는다. 아래 "공유 파일과 동결 경계" 참고.

**Tech Stack:** S2 스택 + `reqwest`(rustls·ring 공급자 고정, `stream`·`multipart`·`gzip`·`brotli`), `url`, `cookie_store`, `aho-corasick`(응답 가림), `nix`(`O_NOFOLLOW`), `notify`(컬렉션 감시, cli와 같은 8.2), `hyper` 1·`hyper-util`·`http-body-util`(웹훅 리스너와 테스트 서버, 이미 lock에 있음), `tokio-tungstenite`(workspace 0.27 + rustls 기능), `eventsource-stream`, `tonic` 0.14·`prost` 0.14·`prost-reflect` 0.16·`tonic-reflection` 0.14·`protox` 0.9(protoc 없이 proto 해석), `rmcp`(MCP 클라이언트: child-process·streamable HTTP·auth), `zip`(지원 번들), 테스트 전용 `rcgen`(TLS fixture를 실행 중 생성). 화면: `jsonc-parser` 3.3.1, `yaml` 2.9.0, `entities` 8.0.0, `qrcode-generator` 2.0.4(v0.9.0과 같은 버전).

**Spec:** [01-design.md](01-design.md) §1 성공 기준(SC5·SC7·SC8)·§2.4·§2.5·§4.4·§4.5·§5.1(TCP 예외·흐름 제어)·§5.2·§5.3·§6.3·§7·§8.2 도구·§8.6·§9 `api`·`webhooks`·`tools`·§9.1-4·14·15·§9.3 이식 표·§10·§11·§12, [00-roadmap.md](00-roadmap.md) §3·§3.4·§3.6·§7

## Global Constraints

- S0b–S2의 Global Constraints를 따른다. 버전은 `1.0.0-dev` 그대로다.
- **트랙 2.** S3·S4의 타입·기능(`documents`·`DocumentStore`·`notes`·`search`·`EditorHost`)을 쓰지 않는다. 노트에 붙이기·코드 편집기 선택 보내기는 S6-1b다.
- Rust 명령은 이 트랙 전용 target을 쓴다: `CARGO_TARGET_DIR=$HOME/.cache/targets/devbox-s5`(아래 "검증 명령").
- **동결 경로를 고치지 않는다.** 데몬 등록·생성기 등록·`Shell.tsx`의 `pages`·`messages.ts`·workspace/`app` 의존성 목록은 줄 추가만 한다. `app/src/rpc/gen/rpc.ts`·`Cargo.lock`·`pnpm-lock.yaml`은 손으로 합치지 않고 다시 만든다(00 §3.6).
- `.github`+`scripts`는 1,500/1,500줄·워크플로 2개다. **새 스크립트·워크플로·CI 설치 단계를 만들지 않는다.** 그래서 protoc 대신 `protox`, TLS PEM 커밋 대신 `rcgen`, E2E용 fixture 서버는 `app/e2e/` 안에 둔다.
- 본문·헤더 값·URL·변수 값은 로그(`tracing`)에 남기지 않는다(01 §4.5). 기록하는 것은 메서드 이름·소요 시간·오류 코드뿐이다. reqwest·tungstenite·tonic 오류의 `Display`에는 URL이 들어가므로 typed 오류로 바꾼 뒤 `Fail::Internal`에 원문을 넣지 않는다(`reqwest::Error::without_url`).
- 비밀(01 §6.3): 파일·기록·로그·인자·MCP 응답에 평문이 없다. 요청·환경 파일과 기록에는 `{{secret:NAME}}` 참조만 둔다. 송신 때 데몬이 `SecretsService::resolve`로 풀고, 그 요청에서 푼 값이 응답 스트림·기록에 다시 나오면 S2와 같은 `[REDACTED]`로 바꾼다.
- 네트워크 시험은 `127.0.0.1:0`의 소유 테스트 서버(hyper·tungstenite·tonic)만 쓴다. 외부 네트워크·방화벽·포트 전달·Docker를 시험에서 쓰거나 바꾸지 않는다. 시험은 `0.0.0.0`에 실제로 묶지 않는다(주소 선택은 순수 함수로 시험). TCP를 여는 것은 01 §5.1의 예외(사용자가 켠 웹훅 리스너, OAuth PKCE 콜백 127.0.0.1·인증 중에만)뿐이다.
- 개인 컬렉션 폴더는 `Paths.config_dir.join("api")`다. `~/.config/devbox/api`를 직접 쓰지 않는다. accept 채널은 `config_dir`이 `devbox-accept`라 수용 스모크가 prod 설정을 건드리지 않는다. 기록은 `Paths.data_dir.join("api-history")`, 웹훅 본문은 `Paths.data_dir.join("webhooks")`다. `Paths`에 필드를 더하지 않는다.
- RPC 경계 타입에 `serde_json::Value`를 쓰지 않는다. 사용자가 쓴 JSON(GraphQL 변수, gRPC 메시지, MCP 인자, 컬렉션 파일 전체)은 JSON 텍스트 `String`으로 주고받고 해석은 TS 또는 그 기능의 Rust 모듈 안에서 한다.
- 저장소 내용의 실행: 프로젝트 컬렉션(`.devbox/api/`) 파일이 MCP stdio 명령·`{{secret:…}}`·multipart 파일 경로를 쓰면, 그 파일 digest를 이 PC에서 검토하기 전까지 실행기·연결이 쓰지 않는다("비밀 재검토", 아래 S5-3). 개인 컬렉션과 저장하지 않은 초안은 사용자가 쓴 것이라 검토 대상이 아니다.
- v0.9.0 데이터는 가져오지 않는다(CONVENTIONS §9). 가져오기는 01 §2.4의 cURL·Postman·Insomnia·Bruno·HAR·OpenAPI만이다.
- 파일 600줄·컴포넌트 300줄(시험 파일 포함). 이식 원본이 넘으면 이식할 때 나눈다(아래 분할 표).
- 화면 문구는 한국어이며 `docs/ui-terms.md`를 따른다. 새 용어(요청·컬렉션·환경·웹훅·변환 도구)는 그 표에 행만 더한다.
- E2E는 S5 전체 3개 이하다(SC7: 전체 40개 이하, 현재 11개).

## Review Focus

| 상황 | 기대 동작 | 시험 위치 |
|---|---|---|
| 헤더 이름 `X API Key`, 값에 개행, 변수 치환 뒤 잘못된 값, GraphQL 사용자 헤더(CV-F05) | 전송 전에 `api.invalid_header{index,field,reason}`, 테스트 서버 연결 0회. `index`는 화면 행 번호, 세부에 값이 없음 | S5-2 |
| 이전 토큰이 세션에 있는데 생산 요청이 비밀 재검토·파일 누락으로 건너뛰어지거나 취소됨(CV-F04·AR-F03) | 소비 요청 송신 0회, "출처 요청 실패: <생산 요청>". `stopOnFailure` 참·거짓 모두. 세션 값은 지우지 않음 | S5-3 |
| 토큰 갱신 요청이 이전 실행의 토큰을 입력으로 씀 | 이번 실행에 앞선 생산 요청이 없으면 이전 값 사용 가능 | S5-3 |
| 본문 종류를 GraphQL→없음→GraphQL, JSON→GraphQL→JSON(AR-F06) | 각 종류 초안이 그대로. 보내기·저장에는 활성 종류만, 비활성 초안의 없는 비밀 참조가 송신을 막지 않음 | S5-3 |
| 응답을 받은 뒤 요청을 고침 / 늦은 gRPC 응답(AR-F08) | 응답 머리에 만든 요청(이름·메서드·URL·시각), 다르면 "편집 중인 요청과 다름". A의 늦은 응답은 "A의 결과", 기록·취소도 A | S5-3·S5-5 |
| gRPC 필터로 다른 메서드가 자동 선택됨(AR-F07) | 이전 메서드의 수정 초안 보존, 필터 자동 선택 동작은 유지 | S5-5 |
| 정규식 `a`를 `가a나`·`😀a`에 적용, 너비 0 일치(AR-F09) | 1..2·2..3(UTF-16), 조각을 이으면 원문, 대리 쌍을 가르지 않음, 파국적 패턴은 1초 뒤 중단 | S5-7 |
| 보내는 중 [취소] | 스트림 닫힘 → 데몬이 요청 future를 버림, 테스트 서버가 연결 종료를 봄, 기록 결과 `cancelled` | S5-2 |
| 응답이 보낸 비밀을 그대로 되돌려 줌 | 화면 스트림·기록 파일에 `[REDACTED]`, 기록의 요청은 `{{secret:NAME}}` | S5-2 |
| 느린 화면에 WebSocket 메시지가 몰림 | 데몬은 계속 읽고 상한 뒤 버림, 재개 때 `dropped n` 표시, 다른 RPC 응답 유지 | S5-4 |
| 웹훅 리스너를 LAN 끔으로 시작 | 127.0.0.1에만 묶임. LAN 켬은 사용자가 켤 때만, 방화벽은 바꾸지 않고 안내만 | S5-6 |
| 직접 입력한 Bearer·client_secret이 있는 요청, 웹훅에서 만든 요청을 저장 | 파일 text에 값 없음(`[REDACTED]`, 참조는 유지), 저장 전 "직접 입력한 비밀 N개는 파일에 저장하지 않습니다" | S5-3·S5-6 |
| Basic·OAuth·캡처 토큰을 쓴 요청의 기록 | 기록 JSON·`.body`에 password·Base64 자격·access token·client_secret·토큰 값 없음 | S5-2 |
| 지원 번들 | zip 안에서 비밀 값·노트 vault 경로·홈 절대 경로 문자열이 검색되지 않음 | S5-8 |
| accept 채널 설치본에서 개인 컬렉션 저장 | `devbox-accept` 설정 폴더에만 쓰임, prod 설정 bytes·mtime 불변 | S5-3·S5-9 |


## 결정 구분 (S5-1)

**사용자 결정(00 §3.2: 사용자 PC·네트워크 변경)** — 결정 전에는 아래 기본안까지만 하고 나머지는 "사용자 결정 대기"로 기록한다.

| 항목 | 기본안(결정 없이 하는 것) | 결정이 필요한 부분 |
|---|---|---|
| ③ 수용 3 웹훅 외부 수신(S5-9) | Windows 쪽 소유 `curl.exe`로 `http://127.0.0.1:<port>`에 보내 WSL 리스너(LAN 끔) 수신·기록·재전송(소유 로컬 대상)을 확인. Windows에서 127.0.0.1 전달이 안 되면 그 항목을 FAIL·근거로 기록하고 **`lan: true`로 바꾸거나 portproxy·방화벽을 만지지 않는다** | 외부 서비스에서 받기(LAN 노출·포트 전달·방화벽) |
| ④ 수용 6 실제 도구 설치(S5-8·S5-9) | 빠진 도구의 [설치] 버튼과 `tools.install_command` 결과(명령·패키지 id)만 확인하고 버튼을 누르지 않음 | `sudo apt`·`npm -g`·`winget` 실제 실행 |

**에이전트 결정(근거와 함께 PROGRESS §5에 기록, 사용자 결정 아님)**

| 항목 | 결정 | 근거 |
|---|---|---|
| ① 해시 MD5(S5-7) | 유지. 의존성 없는 작은 TS `md5.ts`(RFC 1321 벡터), SHA는 WebCrypto | v0.9.0 도구 이름 "해시 (MD5/SHA)"·21종 유지, WebCrypto에 MD5 없음 |
| ② 수용 1 API 요청(S5-9) | 소유 로컬 서버로 확인하고, 네트워크가 되면 공개 echo 엔드포인트에도 fixture 비밀로 보냄(사용자 비밀 아님). 네트워크가 안 되면 그 부분만 미실행·이유 기록 | 01 §11 수용, fixture 비밀은 스모크 소유 |
| ⑤ 프로젝트 컬렉션 비밀 재검토(`api_reviews`, S5-3) | 비밀 참조·multipart 파일·stdio 명령이 있는 프로젝트 파일은 digest 검토 전 실행 안 함 | 01 §6.3·§12(저장소 내용이 비밀·명령을 끌어내지 않음), v0.9.0 `requiresSecretReview` |
| ⑥ 재전송은 화면 → `api.send`(`webhooks.replay` 없음, loopback 한정 해제)(S5-6) | 받은 요청을 API 편집기로 열어 사용자가 보냄 | 07 S5-6 "재전송", 01 §2.7(제품 간 전달 없음)·§12(실행은 사용자 행동) |
| ⑦ `devbox uninstall`·Tauri `install_windows_tool`·`cleanup_wsl`·`redeploy_wsl`(S5-8) | 허용 목록·이 채널 소유 자원만 | 07 S5-8 [설치]·[다시 설정]·[WSL 쪽 정리], 01 §10 |

---

## 작업 묶음

S5 전체가 브랜치 `v1/s5-tools` 하나, PR 하나다(00-roadmap §3). 묶음은 로컬 커밋·세션 인계 지점이다. 과제별 좁은 검증과 별도 검토 뒤 `PROGRESS.md`의 S5 묶음 행·"트랙 2(S5) 위치" 줄을 갱신한다. 묶음별 push·전체 검사는 하지 않는다. 마지막에 브랜치 전체 검토·최종 상세 검증·Biome CI 뒤 push·PR을 한 번 한다.

| 묶음 | 과제 | 끝나면 |
|---|---|---|
| 상세화 | S5-1 | 커밋(이 브랜치의 첫 커밋) |
| AA | S5-2–S5-3 | 로컬 커밋·인계 |
| AB | S5-4 | 로컬 커밋·인계 |
| AC | S5-5 | 로컬 커밋·인계 |
| AD | S5-6 | 로컬 커밋·인계 |
| AE | S5-7–S5-8 | 로컬 커밋·인계 |
| 마무리 | S5-9 | **S5 PR 열기** → 필수 CI 3개·자동 리뷰 3회 상한 판정(CONVENTIONS §8) → squash 머지 |

S5-3·S5-5·S5-7은 크므로 과제 안에서 커밋을 나눈다(각 과제의 "커밋" 단계). 묶음 경계는 그대로다.

---

## 실제 코드 계약과 검증 명령 (S5-1 확정)

**RPC 선언·등록·생성**
- `method!`·`topic!`·`domain_error!`는 `devbox_protocol`의 매크로이고 **선언은 도메인 crate의 `lib.rs`에 둔다**(`crates/runtime/src/lib.rs`·`crates/observe/src/lib.rs`·`crates/secrets/src/lib.rs`의 방식). 프로토콜 crate에는 `system.*`·`debug.*`만 있다. 그래서 `api.*`·`webhooks.*`·`tools.*`는 새 crate에 선언하며 `crates/protocol`을 고치지 않는다.
- 형식: `method!(pub ApiSend = "api.send", stream, SendRequest => devbox_protocol::StreamOpened, ApiError);`. 종류는 `query·mutation·subscription·stream`, 확인 정책은 `mutation(always)`·`mutation(reversible)`. 결과 없는 mutation은 `devbox_protocol::Done{ rev }`.
- 등록은 `Router<Services>::register::<M, _, _>(|s, ctx, p| async move { … })`다(`Router::add`가 아니다). 각 crate는 `pub fn export<X: Exporter>(x: &mut X)`로 메서드·주제를 내보낸다.
- 데몬은 `crates/cli/src/daemon/routes.rs`의 `router()`와 `declared()`, `every_declared_method_is_routed` 시험이 등록 누락을 잡는다. 앞선 예 `super::secret_routes::register(router)`처럼 **S5 등록은 `crates/cli/src/daemon/tools_routes.rs` 한 파일에 모으고** `routes.rs`에는 `let router = super::tools_routes::register(router);` 한 줄과 `declared()`의 `devbox_api::export(&mut n); devbox_webhooks::export(&mut n); devbox_tools::export(&mut n);` 줄만 더한다.
- 스트림: `Ctx::open_stream() -> Result<StreamEnds, StreamError>`, `StreamEnds{ id, tx: StreamTx, input: mpsc::Receiver<Bytes> }`, `StreamTx::{write, finish, closed}`. 데이터는 NDJSON 한 줄씩 쓰고(관례: `observe` 로그), 스트림을 열기 전에 모든 입력 검사를 끝내 오류는 RPC 오류로 돌려준다. 화면 `ClientStream.end()`가 `streamClose`를 보내면 데몬의 `tx.closed()`가 취소된다. **그래서 `api.cancel{ streamId }` 메서드는 만들지 않는다**(07의 Interfaces와 다름, PROGRESS §5).
- 핸들러 오류는 `Fail::Domain(E)` 또는 `Fail::Internal(String)`(로그에만, 응답은 `internal`+진단 ID). 하부 오류는 `Internal::internal("what")?`. `invalid_params`의 이유 문자열은 `secrets.` 메서드만 가린다. `api.send` 파라미터 역직렬화 오류 문장에 사용자 값이 들어갈 수 있으므로 `api.*`는 파라미터를 `#[serde(deny_unknown_fields)]`로 단순하게 두고, 화면은 생성 타입으로만 보낸다.
- 생성기: `xtask/src/main.rs`에 `devbox_api::export(&mut exporter);` 등 줄을 더하고 스트림 이벤트처럼 메서드에 직접 걸리지 않는 타입은 기존 `ts_rs::TypeVisitor::visit::<devbox_agents::ReviewExpect>(&mut exporter)` 방식으로 `visit::<devbox_api::SendEvent>` 등을 더한다. `xtask/Cargo.toml`에 의존성 줄을 더한다. 생성은 `pnpm gen`(= `cargo run -q -p xtask -- gen-ts`), 확인은 `cargo run -q -p xtask -- gen-ts --check`.
- 오류 문구: `app/src/rpc/messages.ts`의 `TABLE: Record<RpcErrorCode, Message | ((detail) => Message)>`에 S5 코드 줄만 더한다(현재 315줄). 같은 코드 문자열을 여러 enum이 써도 된다(`runtime`도 `secrets_locked`를 씀). S5 줄은 한 줄 객체로 짧게 써서 50줄 안쪽으로 둔다.

**데몬 조립**
- `crates/cli/src/daemon/mod.rs`: `migrations()` 배열에 `devbox_api::MIGRATIONS`(S5-3에서, 빈 배열을 미리 만들지 않음)·`devbox_webhooks::MIGRATIONS`(S5-6) 줄, `build_services_with_runtime`에 서비스 생성 줄, `Services` 생성 필드 줄을 더한다. `crates/cli/src/daemon/state.rs`에 `pub api: Arc<devbox_api::ApiService>`, `pub webhooks: Arc<devbox_webhooks::WebhooksService>`, `pub index: devbox_core::IndexDb` 줄을 더한다.
- **`IndexDb`는 아직 데몬 어디에서도 열지 않는다**(core 시험만 씀). S3도 같은 것이 필요하다. S5-2는 `let index = devbox_core::IndexDb::open(&paths.index_db_path)?;`를 한 번만 열어 `Services.index`에 두고 `ApiService`에 clone을 넘긴다. **S3가 먼저 머지돼 같은 필드가 있으면 그것을 쓰고 두 번 열지 않는다**(통합 단계에서 확인).
- `IndexDb::ensure_domain(&self, domain: &str, version: u32, create_sql: &str) -> Result<bool, CoreError>`는 동기 함수다(서비스 초기화에서 부름). 도메인 이름은 ASCII 소문자로 시작하고 소문자·숫자만, 밑줄 불가 → **`api`**. 테이블은 `api_` 접두사만 만든다. `true`면 기록 파일을 다시 읽어 색인을 다시 만든다. 읽기·쓰기는 `IndexDb::read`/`write`(async, `rusqlite::Transaction`).
- `devbox_core::Db::{write, write_checked, write_checked_with_commit, read}`와 `Migration{ domain, version, … }`(`crates/core/src/db.rs`)로 `devbox.db` 테이블(`api_reviews`, `webhooks_*`)을 만든다. 테이블 이름은 `<domain>_<name>`이다. 이벤트는 커밋 rev로 `Hub::publish::<Topic>(rev, &payload)`.
- 경로: `Paths{ instance, home, data_dir, config_dir, instance_config_dir, state_dir, log_dir, db_path, index_db_path, … }`(`crates/core/src/paths.rs`). `ProjectsService::get(id) -> R<Option<Project>>`의 `Project.path`로 프로젝트 컬렉션 폴더(`<path>/.devbox/api`)를 얻는다.

**비밀·입력**
- 비밀: `SecretsService::resolve(&self, names: &[String]) -> R<BTreeMap<String, Zeroizing<Vec<u8>>>>`(`crates/secrets/src/service.rs`). 오류 `SecretsError::{Locked("secrets_locked"), KeyMismatch("secrets.key_mismatch"), InvalidInput, Missing{ names }("secrets.missing"), Unavailable}`. 이름 검사 `devbox_secrets::valid_name`. 참조 문법은 `{{secret:NAME}}`이며 runtime의 `secret_input.rs::references`는 private이라 `crates/api/src/template.rs`에 같은 규칙의 작은 파서를 둔다(공용 모듈을 새로 만들지 않음). `ApiError`로 `api.secret_missing{ names }`·`secrets_locked`·`secrets.key_mismatch`(새 코드를 만들지 않고 secrets의 기존 코드 문자열 재사용)를 매핑한다(runtime `mapped`와 같은 모양).
- 입력 상한: `devbox_core::input::{validate_command_text, MAX_COMMAND_BYTES}`(64KiB·NUL)는 명령 문자열(MCP stdio 명령·설치 명령)에 쓴다. 요청 본문은 별도 상한(아래 S5-2)이다.
- 신뢰 승인(`devbox_projects::trust`)은 `.devbox/devbox.toml`의 실행 정의용이다. API 컬렉션 검토는 그것을 넓히지 않고 `api_reviews`(파일 digest)로 따로 둔다(S5-3).
- `notify.raised`(`devbox_core::notify`)는 runtime 전용 사유 enum이라 S5는 쓰지 않는다. 웹훅 수신은 자기 주제 `webhooks.received`를 쓴다.

**화면**
- 섹션: `app/src/shell/sections.tsx`에 이미 `{ path: "/tools", label: "도구", digit: 6 }`이 있다. 페이지 연결은 `router.tsx`가 아니라 **`app/src/shell/Shell.tsx`의 `buildRouter({ pages: { … } })`**에 `"/tools": { component: ToolsPage, validateSearch: toolsSearch },` 한 줄과 import 줄을 더한다(S3의 `/notes`와 같은 지점, 줄 추가만).
- `SectionLayout({ list, children })`(`app/src/shell/SectionLayout.tsx`), `registerBottomTab`·`useBottomTabs`(`shell/BottomPanel.tsx`), `useCommands().register({ id, label, keys?, run })`(`shell/commands.ts`, 명령 팔레트가 읽음), `openExternal(uri)`(`shell/external.ts`, http/https만, Tauri는 `@tauri-apps/plugin-opener`).
- 쿼리: `useRpcQuery(method, params, opts)`, `useRpcMutation(method)`(`clientRequestId` 멱등, params를 호출 순간 snapshot), `useTopicInvalidation(topic, keys)`, `confirmPolicy(method)`(`app/src/rpc/query.ts`). 스트림: `client.openStream(method, params, { onData(bytes, ack), onEnd(), onOpened? }) → ClientStream{ streamId, write, end, result }`(`app/src/rpc/client.ts`). `useRpcSession()`이 `{ client, state, generation }`.
- 코드 보기: `CodeView({ value, language?, readOnly?, onChange?, ariaLabel })`, `loadLanguage(nameOrFileName)`(`app/src/ui/code/`). 요청·응답 본문 편집·보기에 쓴다. `ui`에서 쓰는 것: `Button`·`Dialog`·`ConfirmDialog`·`Menu`(Radix dropdown, 트리거 필요)·`Select`·`SplitPane`·`Tabs`·`TextArea`·`DiffView`·`Banner`·`EmptyState`·`ErrorState`·`StatusDot`·`useToasts`(`app/src/ui/index.ts`에서). `DataTable`·`VirtualList`·`CodeView`는 배럴에 없으므로 `app/src/ui/DataTable`·`app/src/ui/VirtualList`·`app/src/ui/code/CodeView`를 직접 import한다(`ui/index.ts`는 동결이라 고치지 않음).
- **없는 것:** 우클릭 메뉴 primitive가 없고 `KeyValueEditor`는 "환경 변수" 문구가 박혀 있으며 행별 오류·활성 체크가 없다. `app/src/ui`는 동결이라 우클릭 메뉴(`SelectionMenu`)와 요청 행 표(`ParamTable`)는 `app/src/features/tools/` 안에 둔다(공용화는 S6 이후 판단).
- 선택 보내기 지점: 터미널 `features/terminal/TerminalView.tsx`(**정확히 300줄**, `term.hasSelection()`·`term.getSelection()`), 로그 `features/runs/LogView.tsx`(225줄, 가상 목록 `LogBody`). 클립보드는 기존처럼 `navigator.clipboard.readText()`.
- 화면 시험 fixture는 기능별 가짜 transport(`features/projects/test-wire.ts`의 `ProjectWire` 방식)이며 실행기·초안은 순수 함수에 의존성 주입으로 시험한다.

**점검·MCP·Windows**
- 기존 doctor: `crates/cli/src/doctor.rs`(416줄)의 `trait Probe`·`pub fn checks(&dyn Probe) -> Vec<Check>`·`OsProbe`, 선언은 동결된 `crates/protocol/src/system.rs`의 `system.doctor`(`PingParams => DoctorReport{ checks: Vec<Check{ id, status: Ok|Warn|Fail, message, fix: Option<String> }> }`). CLI `devbox doctor`와 앱 `features/settings/DiagnosticsSettings.tsx`가 같은 목록을 쓴다(01 §4.5). S5-8은 **선언·타입을 그대로 두고** 로직을 `crates/tools`로 옮긴다. `tools.doctor`는 만들지 않는다.
- MCP 서버: `crates/cli/src/mcp.rs`(`devbox mcp`, 줄 단위 JSON-RPC stdio, `initialize`·`tools/list`·`tools/call`, 도구 `devbox_projects`·`devbox_agents`·`devbox_agent_send`·`devbox_run_task` 등). `CARGO_BIN_EXE_devbox`는 `crates/cli` 시험에서만 쓸 수 있어 자기 자신 왕복 시험은 `crates/cli/tests/api_mcp.rs`에 둔다.
- Windows 명령: Tauri 명령은 `app/src-tauri/src/lib.rs`의 `generate_handler![…]`에 등록하고, Linux에서 시험 가능한 검증은 `src/core/*.rs`, 실행은 `src/platform/*.rs`에 둔다(`core/external.rs`·`platform/external.rs`의 허용 목록 방식). winget은 이 방식으로 허용 목록 패키지만 실행한다.
- 터미널 세션: `terminal.create{ projectId?, cwd?, title?, command?, attachZellij? }`는 `command`를 tmux `new-session <command>`로 **즉시 실행한다**(`crates/terminal/src/service.rs` `new_session`). 그래서 설치 명령은 화면의 명시 확인 뒤에만 이것으로 연다(S5-8).

**의존성(실측)**
- workspace에 이미 있음: `tokio`·`tokio-util`·`bytes`·`futures`·`serde`·`serde_json`(raw_value)·`thiserror`·`ts-rs` 12.0.1·`uuid`·`sha2`·`zeroize`·`rusqlite`·`tempfile`·`tokio-tungstenite` 0.27·`axum` 0.8·`data-encoding`(Base64·hex는 이것을 씀). lock에는 `hyper` 1.12·`hyper-util` 0.1.21·`http-body-util`·`http` 1.5·`tower` 0.5가 있다(workspace 항목 아님). `notify = "8.2"`·`aho-corasick = "1"`·`nix = { version = "0.30", … }`는 workspace 항목이 아니라 cli·observe·runtime·projects가 crate에 직접 쓰므로 새 crate도 같은 버전을 crate `Cargo.toml`에 직접 적는다. workspace의 기존 줄(`tokio-tungstenite` 등)은 고치지 않고 기능은 crate 쪽 `{ workspace = true, features = [...] }`로 더한다.
- 없음: `reqwest`·`rustls`·`tonic`·`prost`·`prost-reflect`·`tonic-reflection`·`protox`·`eventsource-stream`·`rmcp`·`url`·`cookie_store`·`zip`·`rcgen`. workspace `Cargo.toml`의 `[workspace.dependencies]`에 줄로 더하고 crate는 `.workspace = true`로 쓴다. `members` 배열은 **한 줄**이라 S3와 충돌하면 양쪽 member를 모두 남긴다.
- 화면 `app/package.json`에 없음: `jsonc-parser`·`yaml`·`entities`·`qrcode-generator`. 줄 추가 뒤 `pnpm install`(pnpm 9)로 lock을 만든다.
- rustls는 ring 공급자를 고정한다(`reqwest` `rustls-tls-*-roots-no-provider` 또는 동등 기능 + `rustls::crypto::ring::default_provider().install_default()` 한 곳). S5-2 첫 단계에서 `cargo check -p devbox-api --target x86_64-unknown-linux-musl`로 musl 정적 빌드(01 §10)를 확인한다.

**검증 명령(좁은 검사, 잠금 없이 자원 제한)**
```bash
cd /home/jihoon/projects/devbox-wt/v1-s5-tools
source ~/.cargo/env
export CARGO_TARGET_DIR=$HOME/.cache/targets/devbox-s5
# supervisor --no-lock: 잠금 없이 CPU4·nice+10·jobs2·threads2·workers2·cgroup 6/8GiB·swap1GiB(지원 환경)를 적용하고 환경을 그대로 넘긴다.
python3 scripts/verify-resources.py --no-lock -- cargo test -p devbox-api --test http_send invalid_header -- --test-threads=2
python3 scripts/verify-resources.py --no-lock -- pnpm --filter app exec vitest run src/features/tools/api/runner/runner.test.ts --maxWorkers=2
python3 scripts/verify-resources.py --no-lock -- pnpm --filter app typecheck
python3 scripts/verify-resources.py --no-lock -- cargo run -q -p xtask -- gen-ts --check
```
- supervisor 없이 돌려야 하면 `CARGO_TARGET_DIR=$HOME/.cache/targets/devbox-s5 CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 nice -n 10 cargo test …`(cgroup 제한 없음을 결과에 적는다).
- 전체 검사·E2E는 S5-9에서 한 번: `bash scripts/check.sh`(내부에서 `verify-resources.py --wait` 공통 잠금, 최대 60분 대기), `bash scripts/e2e.sh <spec…>`(같은 대기 잠금). 둘을 다른 명령으로 감싸 잠금을 두 번 잡지 않는다. 기한 초과 exit 75는 "검사 대기 초과"로 보고하고 다시 시도한다.
- 실패 확인(RED)은 목표 시험의 미구현 심벌·assertion 실패여야 한다. `package not found`·시험 0개·잠금 75·환경 오류는 RED가 아니다. 새 crate 과제는 먼저 member·Cargo.toml·lib.rs·빈 시험 파일 뼈대를 연결한다.
- 모든 커밋 단계: 직접 영향 검사 → **별도 하위 에이전트 검토**(00 §3.4) → `pnpm exec biome ci .` → 지정 파일만 stage →
  `git -c user.name=jihoon22-lee -c user.email=188744452+jihoon22-lee@users.noreply.github.com commit -m '<메시지>'`. 중간 push·PR 없음.
- 과제가 끝나면 PROGRESS의 S5 묶음 행·"트랙 2(S5) 위치" 줄과 §5(계획과 달라진 점)에 한 줄 요약 + artifact 경로(Git common-dir `devbox-verification/s5-<과제>/`)를 쓴다. 이 브랜치의 PROGRESS 사본에서 S5 행만 고친다.

### 공유 파일과 동결 경계

| 파일 | S5에서 하는 일 | 규칙 |
|---|---|---|
| `Cargo.toml`(workspace) | `members`에 `crates/api`·`crates/webhooks`·`crates/tools`, `[workspace.dependencies]`에 새 crate·외부 의존성 줄 | 줄 추가만. `members` 한 줄 충돌은 양쪽 보존 |
| `crates/cli/Cargo.toml`·`xtask/Cargo.toml` | 의존성 줄 | 줄 추가만 |
| `crates/cli/src/daemon/{routes,mod,state}.rs` | 등록 1줄·`declared()` 3줄·migrations/서비스/필드 줄. 나머지는 새 `tools_routes.rs` | 줄 추가만. `Services.index`는 S3와 하나만 |
| `xtask/src/main.rs` | export·visit 줄 | 줄 추가만 |
| `app/src/shell/Shell.tsx` | `pages`의 `/tools` 줄, `<ToolsCommands />` 줄, import 줄 | 줄 추가만 |
| `app/src/rpc/messages.ts` | S5 오류 코드 줄 | 줄 추가만. S3와 합쳐 600줄에 가까워지면 아래 예외 후보 |
| `app/package.json` | 의존성 줄 | 줄 추가만 |
| `app/src/rpc/gen/rpc.ts`·`Cargo.lock`·`pnpm-lock.yaml` | 생성물 | 손으로 합치지 않음. main 통합 뒤 `pnpm gen`·`cargo metadata --format-version 1 >/dev/null`·`pnpm install` |
| `crates/core`·`crates/protocol`·`app/src/ui`·`app/src/rpc`(gen 밖)·`.github`·`scripts` | **고치지 않음** | 필요해 보이면 PROGRESS §2에 적고 먼저 머지하는 쪽이 맡음 |
| `features/terminal/TerminalView.tsx`·`features/runs/LogView.tsx` | 선택 보내기 연결(S5-7) | S1·S2 소유 기능 파일이지만 동결 목록 밖. S3·S4가 고치지 않는 파일 |
| `docs/ui-terms.md` | 용어 행 | 행 추가만 |
| `PROGRESS.md` | S5 행·트랙 2 줄·§5 S5 절 | 자기 행만 |

동결 경로를 건드리지 않는 근거: RPC 선언은 도메인 crate 소유, `Paths`가 이미 `data_dir`·`config_dir`·`index_db_path`를 줌, `IndexDb`·`SecretsService::resolve`·`StreamEnds`·`Router`가 그대로 쓰임, doctor 선언 타입(`Check`)이 문자열 `id`·`message`라 새 항목을 담을 수 있음, 우클릭 메뉴·행 표는 기능 안에 둘 수 있음.

**공유 파일 예외 후보(필요해질 때만, PROGRESS §2 기록 후 진행):**
- `app/src/rpc/messages.ts` 크기: S3·S5가 모두 더하면 600줄을 넘을 수 있다. 넘으면 도메인별 파일로 나누는 것은 동결 경로(`app/src/rpc`) 변경이므로 나중에 머지하는 쪽이 PROGRESS §2에 적고 맡는다(분할은 동작 변화 없는 이동). S5는 한 줄 항목으로 줄 수를 줄여 이 상황을 늦춘다.
- 우클릭 메뉴 공용화(`app/src/ui/ContextMenu.tsx` 새 파일): S5는 하지 않는다. S4·S6에서 같은 것이 필요하면 그때 판단한다.

### 파일 분할 계획(600줄·컴포넌트 300줄)

| 원본(v0.9.0 또는 현재) | 줄 | 이식·수정 때 나누는 방법 |
|---|---|---|
| `requests/lib/openapi.ts` | 1,237 | `api/lib/openapi/{document,operations,parameters,bodies,security,preview}.ts`(+ `packages/openapi/src/index.ts` 193줄·`limits.ts` 8줄을 `document.ts`·`limits.ts`로) |
| `requests/lib/graphql.ts` | 633 | `api/lib/graphql/{build,variables,response,mask}.ts` |
| `requests/lib/transfer.ts` | 745 | 이식하지 않음(v0.9.0 내보내기 형식). 가져오기가 쓰는 상수 `MAX_EXPORTED_*`·`isSensitiveName`·`looksLikeSecret`만 `api/lib/limits.ts`·`secretPatterns.ts`로 |
| `requests/lib/persistence.ts` | 549 | localStorage 영속은 이식하지 않음. 정화 함수(`sanitizeRequestForPersistence` 계열)만 `api/lib/storage.ts`로(600줄 넘으면 `storage/{fields,graphql,multipart}.ts`) |
| `transforms/tools/common.tsx` | 658 | `transforms/frame/{ToolInput,ToolOutput,ToolActions}.tsx` + `useToolAction.ts`(전달·컨텍스트 메뉴 코드는 버림) |
| `requests/lib/codegen.ts` | 400 | `api/lib/codegen/{index,shell,javascript,python,go,rust}.ts`(axios·Rust 추가 뒤 600 초과 방지) |
| `crates/cli/src/doctor.rs` | 416 | S5-8에서 `crates/tools/src/doctor/{checks,probe,wsl}.rs`로 이동, cli에는 `OsProbe` 조립만 |
| `app/src/features/terminal/TerminalView.tsx` | 300 | S5-7 선행 이동: 키 동작(복사·붙여넣기·확대) 처리를 `terminalActions.ts`로 옮김(동작 변화 없음, 별도 커밋) |
| `app/src/rpc/messages.ts` | 315 | 위 예외 후보 |
| v0.9.0 `http-client-engine/src/commands/request.rs` | 5,867 | 이식하지 않고 시험만 옮긴다. `crates/api/src/http/{mod,build,body,redirect,stream}.rs`로 처음부터 나눈다 |
| v0.9.0 `webhook-core/src/core/{fixtures,rules}.rs` | 1,844·1,025 | `crates/webhooks/src/{rules,matcher,sequence,store,history,listener}.rs` |

---

## v0.9.0 이식 대상 테스트 수 (S5-1 실측)

**세는 법:** `git -C /home/jihoon/projects/devbox show v0.9.0:<경로>`의 정적 선언을 셌다. TS는 줄 머리의 `it(`·`test(`(`.each`는 1개로 세고 표시), Rust는 `#[test]`·`#[tokio::test]`다. 실행해 본 수가 아니다. 이식 과제는 실제로 옮긴 시험 이름을 PROGRESS artifact(`test-names.json`)에 남긴다. "이식"은 입력·assertion을 그대로 옮김, "적응"은 v1 구조(데몬 송신·파일 컬렉션·[REDACTED] 가림·브라우저 실행)에 맞춰 같은 의도의 시험으로 바꿈, "제외"는 이유가 있는 미이식이다.

### A. `packages/api-studio-features/src/requests/lib` (35개 파일, 226개)

07이 지정한 대상(`importers`·`codegen`·`assertions`·`captures`·`jsonPath`·`graphql`·`openapi`)과 그것이 import하는 의존 모듈을 함께 센다.

| 파일 | 수 | 처리 | 과제 | 비고 |
|---|---|---|---|---|
| `importers/curl.test.ts` | 7 | 이식 7 | S5-3 | 셸 따옴표·`-G`·`-F`·파일 본문 미읽기 |
| `importers/postman.test.ts` | 6 | 이식 6 | S5-3 | fixture `postman-v21.json` 함께 |
| `importers/insomnia.test.ts` | 5 | 이식 5 | S5-3 | fixture `insomnia-v4.json` 함께 |
| `importers/bruno.test.ts` | 5 | 이식 5 | S5-3 | |
| `importers/har.test.ts` | 3 | 이식 3 | S5-3 | |
| `importers/index.test.ts` | 3 | 이식 2·제외 1 | S5-3 | 제외: "preserves secret review … Devbox exports"(v0.9.0 내보내기 형식, CONVENTIONS §9) |
| `importers/preview.test.ts` | 2 | 적응 2 | S5-3 | 미리보기 결과가 "만들 파일 목록"이 됨. 인증 가림·잘림 보고 유지 |
| `codegen.test.ts` | 13 | 이식 12·제외 1 | S5-3 | 제외: "loads a C# trust certificate…"(07 대상 언어에 C# 없음). axios·Rust reqwest 시험 새로 |
| `codegen.syntax.test.mjs` | 1 | 적응 1 | S5-3 | `.ts`로 바꿔 vitest include에 맞춤(python3·bash 문법 검사) |
| `assertions.test.ts` | 7 | 이식 7 | S5-3 | `assertionRegex.worker.ts`(취소·시한) 함께 |
| `captures.test.ts` | 4 | 이식 3·적응 1 | S5-3 | 적응: native 가림 표시 → 데몬 `[REDACTED]`를 쓸 수 있는 캡처 값으로 보지 않음 |
| `jsonPath.test.ts` | 5 | 이식 5 | S5-3 | |
| `graphql.test.ts` | 7 | 이식 6·적응 1 | S5-3 | 적응: "masks query literals…"는 코드 생성·기록 표시용 가림으로 |
| `openapi.test.ts` | 14 | 이식 14 | S5-3 | URL 가져오기는 `api.send`로 받은 텍스트를 넘김 |
| `packages/openapi/src/index.test.ts` | 2 | 이식 2 | S5-3 | 별도 패키지였던 문서 해석기 |
| `runner.test.ts` | 16 | 이식 9·적응 2·제외 5 | S5-3 | 아래 표. AS-07 TS 쪽 원본 |
| `headers.test.ts` | 4 | 이식 4 | S5-3 | |
| `multipart.test.ts` | 6 | 이식 6 | S5-3 | 파일 부분은 WSL 절대 경로 |
| `cookies.test.ts` | 6 | 이식 6 | S5-3 | 요청 Cookie 행 |
| `binary.test.ts` | 3 | 이식 3 | S5-3 | 응답 보기의 이진 판정 |
| `environments.test.ts` | 14 | 이식 7(Rust로)·적응 1·제외 6 | S5-2 | 치환 규칙 7개는 데몬 `template.rs` 시험으로. 적응: "알 수 없는 변수는 그대로"(`{{NAME}}`) → v1은 송신 전 `api.variable_missing{ names }`(CV-F05와 같은 "조용히 보내지 않음" 원칙, 정의 안 된 `${NAME}`은 v0.9.0처럼 그대로). 제외: localStorage 저장소 6개 |
| `importTransaction.test.ts` | 2 | 적응 2 | S5-3 | 여러 파일 가져오기 중 실패 시 새로 만든 파일만 정리 |
| `collections.test.ts` | 11 | 제외 11 | — | localStorage 컬렉션 → 파일 컬렉션(S5-3 새 시험) |
| `persistence.test.ts` | 15 | 적응 8·제외 7 | S5-3 | 적응: 마스킹 8개(JSON/form/url/auth 직접 입력 비밀·부분 참조·form token/password·알려진 토큰 패턴·multipart 파일 경로와 stale body·template URL과 깨진 JSON·GraphQL query literal/variables·비-GraphQL의 stale GraphQL)를 파일 저장 정화 `lib/storage.ts` 시험으로(01 §9.1-14 "파일 저장에도 비밀 정화"). 제외: v2 읽기·마이그레이션·sanitizer 거부 7개 |
| `transfer.test.ts` | 12 | 제외 12 | — | v0.9.0 내보내기 형식 |
| `fileCollection.test.ts` | 5 | 적응 5 | S5-3 | v1 파일 배치로: 결정적 파일 이름·폴더 유지, 예약 이름·대소문자 충돌, unsafe 경로·잘못된 형식·크기 거부, 직렬화 전 비밀 정화와 `requiresSecretReview` 왕복, 유니코드 이름 길이 |
| `history.test.ts` | 5 | 제외 5 | — | 데몬 기록(S5-2)으로 대체 |
| `requestChecksPersistence.test.ts` | 3 | 적응 2·제외 1 | S5-3 | 적응: 잘못된 check만 버리고 알려진 expected token 정화, 구조만 맞는 잘못된 식은 남겨 평가 때 보고. 제외: 이전 문서 읽기 |
| `oauth2Persistence.test.ts` | 4 | 적응 2·제외 2 | S5-3 | 적응: literal client secret 정화·참조 보존, client credentials 변수 검사에서 비활성 authorization URL 무시. 제외: native 문서 왕복·이전 요청 |
| `tlsPersistence.test.ts` | 2 | 적응 1·제외 1 | S5-3 | 적응: 잘못된 인증서 참조 제거·검증 기본 켬. 제외: 문서 왕복 |
| `reviewedHistory.test.ts` | 2 | 제외 2 | — | localStorage 기록 |
| `contextMenu.test.ts` | 4 | 제외 4 | — | `@devbox/context-menu` 패키지 |
| `websocket.test.ts`·`sse.test.ts` | 8·7 | 참고(선택 이식) | S5-4 | 메시지 표시·상한 순수 로직만 |
| `grpc.test.ts`·`mcp.test.ts` | 7·8 | 참고(선택 이식) | S5-5 | 초안·목록 표시 순수 로직만 |

합계: 07 지정 대상 84개(lib 안 82 + `packages/openapi` 2, 이식 77·적응 5·제외 2), 의존·관련 144개(이식 35(Rust 7 포함)·적응 23·제외 56·참고 30). lib 35개 파일 226개 + openapi 2개 = 228개. `runner.test.ts` 처리:

| v0.9.0 시험 | 처리 |
|---|---|
| chains a captured token into the next request · fails a request whose variable was not captured · stops after the first failure when asked and marks the rest skipped · cancels the in-flight request and skips the rest · a missing capture cannot reuse old session or environment values · enforces request and delay bounds before sending · cancels during regex assertions before captures or a subsequent request | 이식 |
| skips requests that need a secret review and continues without stop-on-failure | 이식 + CV-F04 강화(소비 요청 차단) |
| ignores hidden body and authentication drafts until selected | 이식(AU-AS-07) |
| never sends retained multipart file placeholders and clears failed sealed captures · discarding all captures keeps tombstones instead of reviving older environment values | 적응(봉인 값 없이 같은 의도) |
| refuses to undo a discard over a newer captured value · chains native sealed captures … · rejects a native response missing … · revokes native references … · does not revive sealed values … | 제외(v0.9.0 native 봉인 캡처, v1은 캡처를 TS가 소유) |

### B. `packages/api-studio-features/src/transforms` (271개)

| 파일 | 수 | 처리 | 과제 |
|---|---|---|---|
| `tools/transformers.test.ts` | 29 | 이식 | S5-7 |
| `tools/byteCodec.test.ts`·`ids`·`jsonTypescript`·`radix` | 10·10·10·10 | 이식 | S5-7 |
| `tools/jsonYaml.test.ts`·`markdownTable`·`textEncoding`·`jwt`(`.each` 2) | 13·11·11·12 | 이식 | S5-7 |
| `tools/lorem.test.ts`·`qr`·`hmac` | 5·5·5 | 이식 | S5-7 |
| `tools/outputPolicy.test.ts` | 1 | 적응(HMAC·JWT 입력을 최근 기록에 남기지 않는 정책으로) | S5-7 |
| `tools/common.test.tsx`(`.each` 1) | 11 | 적응(입력·출력 틀 분할, 붙여넣기·복사·한도·취소) | S5-7 |
| `tools/diff.test.tsx` | 2 | 이식 1·적응 1(native diff 실패 → 계산 실패 문구) | S5-7 |
| `tools/security.test.tsx` | 5 | 적응(IPC → WebCrypto·로컬 생성, 늦은 응답 무시 유지) | S5-7 |
| `tools/{ByteCodec,Hmac,JsonTypescript,JsonYaml,Jwt,Lorem,MarkdownTable,Qr,Radix,TextEncoding}Tool.test.tsx` | 4·6·4·4·8·6·6·3·5·5 | 적응(native 경로 문구 → 브라우저 경로) | S5-7 |
| `tools/ApiHandoffAction.test.tsx`·`KnowledgeDraftAction.test.tsx`(각 `.each` 1) | 7·7 | 제외(제품 간 전달, 01 §2.7·07) | — |
| `workflows/smartDetection.test.ts` | 7 | 이식(자동 감지) | S5-7 |
| `workflows/SmartWorkflowPanel.test.tsx`·`transformPipeline.test.ts`·`workflowStore.test.ts` | 11·7·10 | 제외(파이프라인 저장 X-2) | — |
| `App.applink.test.tsx`·`api.knowledge.test.ts`·`api.toolbox.test.ts` | 12·5·4 | 제외(앱 링크·제품 간 전달) | — |
| `tools/regex.tsx` | 0 | 시험 없음 → AR-F09 새 시험 | S5-7 |

합계: 이식·적응 208, 제외 63. `apps/api-studio-tools.json` 21행 대조:

| # | id | 그룹 | 이름 | v0.9.0 구현·시험 | v1 |
|---|---|---|---|---|---|
| 1 | `json-format` | JSON | 포매터 | `transformers.tsx` `jsonFormatter` / transformers | `transforms/lib/transformers.ts` |
| 2 | `json-minify` | JSON | 최소화 | `jsonMinifier` / transformers | 같은 파일 |
| 3 | `json-yaml` | JSON | JSON ↔ YAML | `JsonYamlTool`·`jsonYaml.ts`(yaml·jsonc-parser) | `lib/jsonYaml.ts` |
| 4 | `json-typescript` | JSON | JSON → TypeScript | `JsonTypescriptTool`·`jsonTypescript.ts` | `lib/jsonTypescript.ts` |
| 5 | `byte-codec` | 인코딩 | UTF-8 / Base64 / Hex | `ByteCodecTool`·`byteCodec.ts` | `lib/byteCodec.ts` |
| 6 | `radix` | 인코딩 | 진법 변환기 | `RadixTool`·`radix.ts` | `lib/radix.ts` |
| 7 | `html-entity-encode` | 인코딩 | HTML 엔터티 인코딩 | `HtmlEntityEncoder`·`textEncoding.ts`(entities) | `lib/textEncoding.ts` |
| 8 | `html-entity-decode` | 인코딩 | HTML 엔터티 디코딩 | `HtmlEntityDecoder` | 같은 파일 |
| 9 | `url-encode` | 인코딩 | URL 컴포넌트 인코딩 | `UrlEncoder` / transformers | `lib/transformers.ts` |
| 10 | `url-decode` | 인코딩 | URL 컴포넌트 디코딩 | `UrlDecoder` | 같은 파일 |
| 11 | `qr` | 인코딩 | QR 생성기 | `QrTool`·`qr.ts`(qrcode-generator) | `lib/qr.ts` |
| 12 | `timestamp` | 시간 | 타임스탬프 변환기 | `TimestampConverter` / transformers | `lib/transformers.ts` |
| 13 | `case` | 텍스트 | 대소문자 변환기 | `CaseConverter` / transformers | 같은 파일 |
| 14 | `lorem` | 텍스트 | Lorem 생성기 | `LoremTool`·`lorem.ts` | `lib/lorem.ts` |
| 15 | `markdown-table` | 텍스트 | Markdown 표 포매터 | `MarkdownTableTool`·`markdownTable.ts` | `lib/markdownTable.ts` |
| 16 | `hash` | 보안 | 해시 (MD5/SHA) | `HashTool`(security) → native `hash`(MD5 포함), 브라우저는 SHA만 | WebCrypto SHA-256/512 + TS `md5.ts`(에이전트 결정 ①) |
| 17 | `hmac` | 보안 | HMAC 생성 / 검증 | `HmacTool`·`hmac.ts`(WebCrypto) | `lib/hmac.ts` |
| 18 | `uuid` | 보안 | UUID / ULID 생성기 | `UuidTool`·`ids.ts` | `lib/ids.ts` |
| 19 | `regex` | 정규식 | 정규식 테스터 | `RegexTool` → native `regex_test`(byte 위치, AR-F09) | `lib/regex.ts` + worker |
| 20 | `diff` | 차이 | 텍스트 차이 | `DiffTool` → native `diff` | `lib/lineDiff.ts`(TS Myers) |
| 21 | `jwt` | 인증 | JWT 디코더 | `JwtDecoder`·`jwt.ts`(WebCrypto 검증) | `lib/jwt.ts` |

그룹 수: JSON 4·인코딩 7·시간 1·텍스트 3·보안 3·정규식 1·차이 1·인증(JWT) 1 = 21(01 §2.4와 같음).

### C. `crates/http-client-engine/src/commands/request.rs` (67개)

AU-AS 수정 시험: AS-01 `none_body_ignores_hidden_draft_and_rejects_unknown_kind`, AS-02 `query_form_preserves_reserved_characters_duplicates_and_fragment`·`append_query_and_form_keep_existing_behavior`, AS-07 `active_auth_ignores_inactive_secret_references`(Rust 쪽, TS 쪽은 runner "ignores hidden body and authentication drafts"). 모두 S5-2에 걸린다.

| 처리 | 수 | 시험 |
|---|---|---|
| 이식 | 37 | `resolves_both_reference_styles_including_auth_only_in_backend`, `cookie_defaults_and_backend_only_secret_resolution_are_safe`, `invalid_or_ambiguous_cookie_configuration_fails_closed`, `redacts_response_body_error_and_url_values`(가림 대상 = 이 요청에서 푼 devbox 비밀 값), `binary_response_classification_uses_media_type_and_utf8_safely`, `http_clients_accept_valid_pem_material_and_reject_broken_material`, `tls_client_identity_does_not_cross_redirect_origins`, `tls_missing_credential_precedes_oauth_token_resolution`, `tls_missing_credential_fails_before_connecting_even_when_verification_is_off`, `tls_stream_configuration_is_not_silently_ignored`(S5-4), `oauth2_actual_send_fetches_once_and_masks_the_token_response`, `oauth2_token_wait_is_cancelled_by_the_request_owner`, `oauth2_resolves_active_configuration_and_masks_client_and_access_tokens`, `cross_origin_redirect_strips_sensitive_headers`, `body_headers_are_identified_for_body_suppression`, `redirect_method_rules_preserve_307_and_switch_post_302`, `none_body_ignores_hidden_draft_and_rejects_unknown_kind`, `query_form_preserves_reserved_characters_duplicates_and_fragment`, `active_auth_ignores_inactive_secret_references`, `append_query_and_form_keep_existing_behavior`, `missing_multipart_file_error_never_contains_path`, `multipart_secret_resolution_ignores_disabled_parts`, `live_request_streams_text_and_file_multipart_with_derived_boundary`, `live_request_sends_enabled_duplicate_headers_in_order_and_skips_disabled`, `live_request_builds_one_ordered_cookie_header_and_skips_disabled_rows`, `graphql_post_sends_standard_body_and_projects_data_and_errors_without_secret_echo`, `graphql_get_uses_encoded_query_parameters_without_request_body`, `graphql_endpoint_rejects_userinfo_fragment_and_credential_query_before_connecting`, `graphql_preflight_rejects_header_and_query_bounds_without_network`(+CV-F05), `graphql_cancellation_stops_before_network_io`, `graphql_redirects_only_keep_payload_on_the_original_origin`, `redirect_headers_and_final_body_share_one_deadline`, `graphql_cancellation_interrupts_an_in_flight_request`, `live_cross_origin_redirect_strips_auth_and_redacts_every_return_path`, `live_cross_origin_307_and_308_suppress_body_and_derived_secret_headers`, `live_cross_origin_redirect_with_sensitive_destination_is_blocked`, `live_network_error_is_generic_and_contains_no_request_secret` |
| 적응 | 17(→11개 시험) | 기록 정화 6개 → `crates/api/tests/history.rs`의 3개: `mixed_variable_references_do_not_exempt_literal_credentials_from_redaction`·`persisted_oauth_metadata_masks_url_credentials_and_literal_client_secret`·`persisted_history_display_name_survives_and_redacts_environment_secret`(→ `history_never_contains_tokens_or_basic_credentials`), `persisted_graphql_keeps_shape_but_masks_query_and_variable_secrets`·`malformed_graphql_literal_is_masked_without_raw_persistence`(→ `graphql_history_masks_query_literals`), `multipart_persistence_removes_paths_backup_body_and_sensitive_text`(→ `multipart_history_keeps_no_file_paths_or_text_secrets`); `non_graphql_requests_drop_stale_graphql_state_without_unsealing_it`(→ snapshot 본문은 하나뿐 + TS AR-F06), `corrupt_secret_fails_closed_without_ciphertext_fallback`(→ `secrets_locked_or_missing_sends_nothing`), `response_headers_and_cookie_rows_are_masked_while_raw_values_stay_in_vault`(→ 응답 헤더도 같은 가림, vault 없음), `binary_response_projection_is_bounded_and_masks_request_secrets`(→ 이진 응답 스트림·기록 상한), `tls_field_accepts_old_requests_and_defaults_to_verification`(→ 기본 검증 켬), `native_binary_stream_stops_at_the_configured_bound`(→ 기록 본문 상한), `persisted_json_removes_environment_secret_and_sensitive_literals`(→ `secret_reference_is_resolved_on_send_and_kept_as_reference_in_history`), `concurrent_registration_keeps_routing_and_response_ownership_together`·`failed_registration_does_not_replace_the_current_request`·`starting_a_new_request_supersedes_the_previous_cancellation_token`·`delayed_or_early_cancel_is_routed_to_its_exact_request`(→ 스트림 소유라 하나로: `two_streams_cancel_independently`) |
| 제외 | 13 | `persisted_oauth_metadata_survives_native_sanitization_without_exempting_response_keys`·`persisted_json_preserves_only_boolean_secret_review_metadata`·`persisted_collection_wire_shape_survives_backend_sanitization`·`persisted_history_wire_shape_survives_backend_sanitization`(v0.9.0 native 문서·wire 형식, 검토 표시 왕복은 TS fileCollection 적응이 맡음), `legacy_header_defaults_enabled_and_disabled_reference_is_not_unsealed`(v0.9.0 형식), `response_header_vault_rejects_stale_ids_and_capture_disables_raw_on_overflow`·`native_capture_chains_a_sealed_token_while_response_stays_masked`·`native_capture_commit_rejects_cancelled_superseded_and_discarded_owners`·`native_binary_loopback_projects_masked_metadata_and_retains_only_current_raw_bytes`·`capture_restore_rejects_unissued_or_revoked_references`·`response_vault_retains_binary_only_for_current_response_id`(native vault·봉인 캡처), `revealed_curl_keeps_oauth_and_tls_as_explicit_external_placeholders`·`revealed_curl_is_only_built_from_resolved_backend_request`(코드 생성은 TS) |

참고로 같은 crate의 다른 파일: `sse.rs` 10·`websocket.rs` 8(S5-4 참고), `grpc.rs` 12·`grpc_selection.rs` 5·`grpc_credentials.rs` 5·`mcp.rs` 22·`mcp_stdio.rs` 9·`mcp_oauth.rs` 7·`oauth2/*` 16(S5-5 참고). 07이 지정하지 않았으므로 의무 이식은 아니며, 해당 과제가 동작 시험을 고를 때 이름을 대조한다. v0.9.0 gRPC 시험은 `protox`로 시험 안에서 proto를 해석해 `tonic_reflection::server::Builder::register_encoded_file_descriptor_set`에 넣는다(`crates/api-protocols` `compile_local_proto`). v1도 같은 방식이다.

### D. `crates/webhook-core` (95개)

v0.9.0 webhook-core는 HTTP 파서를 직접 구현했다. v1은 hyper 1(07 Tech Stack)이라 파서 시험은 리스너 통합 시험으로 옮긴다.

| 파일 | 수 | 이식 | 적응 | 제외(이유) |
|---|---|---|---|---|
| `core/body.rs` | 2 | `text_stays_text_and_binary_round_trips` | — | `missing_encoding_field_means_utf8`(v0.8 형식) |
| `core/rules.rs` | 21 | 19(`matches_exact_and_wildcard`·`method_matching_is_case_insensitive_and_none_matches_all_methods`·`path_matching_is_exact_or_prefix_only_for_a_trailing_star`·`upsert_assigns_and_preserves_rule_identity`·`validates_status_and_delay_bounds`·`selector_is_insertion_order_independent_and_uses_documented_precedence`·`final_tie_break_is_bytewise_ascending_id`·`conflict_preview_reports_full_and_partial_overlap_without_mutation`·`overlap_matrix_handles_methods_exact_paths_and_nested_prefixes`·`rejects_transport_response_headers_that_would_override_wire_framing`·`validates_method_path_and_body_shape_and_size`·`validates_rule_id_character_and_string_bounds`·`validates_header_count_shape_and_aggregate_limits`·`enforces_rule_count_and_collection_string_limits`·`validates_complete_rule_collection`·`response_sequence_consumes_in_order_and_holds_the_final_step`·`response_sequence_is_bounded_and_validated_like_the_base_response`·`response_sequence_state_reset_starts_at_the_base_response`·`matcher_rejects_non_ascii_paths_even_for_unvalidated_rules`) | 2(`priority_is_bounded_and_old_json_defaults_to_zero` → 범위만, `rejects_invalid_upsert_without_mutating_storage` → DB 트랜잭션) | — |
| `core/history.rs` | 13 | 5(`history_bounded`·`history_bounds_utf8_header_bytes_and_invalid_methods`·`removal_and_clear_do_not_retarget_stale_ids`·`request_rate_is_bounded_per_fixed_window`·`request_rate_recovers_when_the_wall_clock_moves_backwards`) | 3(`binary_history_bodies_skip_text_masking_and_keep_whole_base64_groups`→ `.bin` 파일, `history_bounds_raw_and_masked_header_snapshots`→ 헤더 상한, `history_masks_and_truncates_body`→ 잘림만) | 5 가림 시험(v1 수신 기록은 PC별 데이터 폴더에만 있고 제품 간 전달이 없음) |
| `core/http.rs` | 15 | 1(`response_writer_owns_framing_headers` → 규칙 검증) | 10(chunked 결합·chunked 상한·충돌 TE·`Expect: 100-continue`·알 수 없는 Expect(417)·이진 본문·큰 헤더(431)·느린 본문 시한·본문 없는 상태 코드·HEAD) | 4(`transfer_encoding_conflicts_and_admission_precede_body_io`·`parser_bounds_and_normalizes_a_fixed_length_request`·`parser_rejects_non_ascii_request_targets`·`parser_cancels_before_body_read`: hyper 내부) |
| `core/fixtures.rs` | 26 | 2(`binary_fixtures_keep_their_bytes_and_reject_invalid_base64`·`fixture_to_rule_draft_is_local_and_has_empty_response_metadata`) | 1(`malformed_and_oversized_input_fails_without_a_partial_fixture`) | 23(가림 10·파일 저장소 잠금/CAS/링크 9는 DB로 대체, v0.8 형식·ID 형식·시각 표시·기록 가림 4) |
| `core/replay.rs` | 9 | — | 2(`base64_fixtures_replay_their_original_bytes`·`request_uses_masked_snapshot_and_owns_transport_headers` → TS `toRequest` 변환) | 7(loopback 한정 재전송·native 송신·속도 제한: v1은 `api.send`로 임의 대상에 보냄) |
| `core/handoff.rs` | 5 | — | — | 5(제품 간 전달, 01 §2.7) |
| `core/service_profile.rs` | 4 | — | — | 4(Run Manager 내보내기, 폐기) |
| 합계 | 95 | 28 | 18 | 49 |

---

## 회귀 연결 (01 §9.1·00 §7 → 시험)

| 출처 | 시험 이름(위치) | 입력 | assertion |
|---|---|---|---|
| CV-F05 | `invalid_header_name_is_rejected_before_sending`(`crates/api/tests/http_send.rs`, S5-2) | 사례표: ① 이름 `X API Key` ② 값 `a\nb` ③ 값 `{{bad}}`, 환경 `bad = "x\r\nInjected: 1"` ④ 값 `Bearer {{secret:NL}}`, fixture 비밀 `NL = "a\nb"` ⑤ 화면 행 `[0: 비활성 "X Bad", 1: 활성 "X API Key"]` ⑥ GraphQL 본문 + 사용자 헤더 `X Bad: 1` ⑦ 값 앞뒤 공백(OWS)만 있는 정상 값 | ①–⑥ RPC 오류 `api.invalid_header{ index, field, reason }`(⑤의 index는 1, ①⑤⑥ field `name`, ②③④ `value`), 오류 세부 JSON에 값·비밀 문자열 없음, 테스트 서버 `accepted_connections() == 0`, 기록 0건. ⑦은 정상 송신 |
| CV-F04·AR-F03 | `skipped_or_cancelled_producer_blocks_the_consumer`(`app/src/features/tools/api/runner/runner.test.ts`, S5-3) | 세션 `token = "old"`(출처 `run-0`·`login`). 항목 `[login(캡처 token), use(헤더 Authorization: Bearer {{token}})]`. 생산 요청을 ① 비밀 재검토 필요 ② multipart 파일 부분 경로 없음 ③ 송신 중 abort. 각각 `stopOnFailure` 참·거짓 | `deps.send`가 `use`로 불린 횟수 0, `use` 단계 `status: "skipped"`·`message`에 "출처 요청 실패", 세션 `token`은 여전히 `"old"`(지우지 않음), ③은 `login` 송신 1회 후 취소 |
| CV-F04 | `captured_value_from_failed_request_is_not_used`(같은 파일) | `login`이 500 + assertion 실패, 캡처 경로 없음 | `use` 송신 0회, 출처 표시 |
| CV-F04(정상 갱신) | `token_refresh_can_read_the_previous_run_value`(같은 파일) | 세션 `token = "old"`, 항목 `[refresh(헤더 {{token}}, 캡처 token)]` | `refresh`가 `old`로 송신 1회, 이후 세션 `token`은 새 값·출처 이번 실행 |
| CV-F04(다른 출처 보존) | `other_producers_and_environment_values_are_kept`(같은 파일) | 환경 `base = https://…`, 다른 생산 요청 `a`가 만든 `x`; `login` 실패 | `base`·`x`는 그대로 사용 가능, `token` 소비만 차단 |
| AR-F06 | `body_kind_switch_keeps_each_draft`(`api/model/draft.test.ts`, S5-3) | GraphQL `query = "query Q { me { id } }"`, `variables = '{"a":1}'`, `operationName = "Q"` → 없음 → GraphQL. JSON `{"x":1}` → GraphQL로 바꿔 query 편집 → JSON. 비활성 GraphQL 초안의 헤더 값에 `{{secret:MISSING}}` | 세 필드 그대로, JSON 본문 그대로. `toSnapshot`·`toFile`에는 활성 종류 하나만(`body.kind === "json"`), `secretNames(toSnapshot(d))`에 `MISSING` 없음, 가짜 send 성공 |
| AR-F06(저장) | `save_and_save_as_new_are_distinct`(`api/RequestEditor.test.tsx`, S5-3) | 열린 파일 `a.request.json`(digest d1)에서 "저장"과 "새 요청으로 저장" | 저장은 `api.collection_write{ path: a, expect: { digest: d1 } }` 1회, 새 요청은 새 경로 `expect: absent` 1회·새 파일 1개, 탭·트리 dirty 점 해제 |
| AR-F08(HTTP) | `response_origin_marks_edited_request`(`api/ResponseView.test.tsx`, S5-3) | 요청 보냄 → 응답 → URL 편집 | 머리에 이름·메서드·URL·보낸 시각, 편집 뒤 "편집 중인 요청과 다름" |
| AR-F07 | `grpc_filter_switch_keeps_the_dirty_draft`(`api/grpc/grpcDrafts.test.ts`·`MethodPicker.test.tsx`, S5-5) | 메서드 A 입력 수정 → 필터 입력으로 B 자동 선택 → 필터 지움 → A. 일치 없음 필터 → 지움 | A 초안 그대로, 필터 자동 선택은 여전히 B를 고름 |
| AR-F08(gRPC) | `late_grpc_response_shows_its_origin`(`api/grpc/GrpcPanel.test.tsx`, S5-5) | A 호출 대기 → B 선택 → A 응답 도착 | 결과 머리 "A의 결과", 기록 항목 메서드 A, [취소]는 A 요청 스트림의 `end()` |
| AR-F09 | `regex_ranges_are_utf16_and_cover_the_text`(`transforms/lib/regex.test.ts`, S5-7) | `가a나`·`a` → `[1,2]`, `😀a`·`a` → `[2,3]`, `a.b.c`·`\.` 여러 일치, 너비 0 `^|$`·`(?=a)`, `u` 플래그 `😀` 너비 0 | 범위는 `startUtf16`·`endUtf16`, 일치·불일치 조각을 이으면 원문, 너비 0 일치도 무한 반복 없음, `u`에서 대리 쌍 가운데 위치 없음, 일치 1만 개에서 `truncated` |
| AR-F09(시한) | `regex_worker_stops_catastrophic_patterns`(같은 폴더 worker 시험) | `(a+)+$`, 입력 `"a".repeat(30)+"b"` | 1초 안에 "시간 초과" 결과, 화면 스레드 막힘 없음 |
| AU-AS-01 | `disabled_body_and_auth_are_not_sent`·`none_body_ignores_hidden_draft_and_rejects_unknown_kind`(S5-2) + TS runner "ignores hidden body…"(S5-3) | 본문 없음 snapshot, 화면에 JSON 초안 남음 | 서버 수신 본문 0바이트·`Content-Type` 없음 |
| AU-AS-02 | `query_and_form_are_url_encoded`·`query_form_preserves_reserved_characters_duplicates_and_fragment`·`append_query_and_form_keep_existing_behavior`(S5-2) | query `[("q","a b+c&d=é한글"),("q","2"),("empty","")]`, URL `…/p?x=1#frag`, form 같은 쌍 | 서버가 디코드한 쌍이 순서·중복 그대로, `+`는 `%2B`, fragment 미송신, form `application/x-www-form-urlencoded` |
| AU-AS-07 | `active_auth_ignores_inactive_secret_references`(S5-2) + runner "ignores hidden … authentication drafts"(S5-3) | 활성 인증 없음, 비활성 Bearer 초안에 없는 비밀 | 송신 성공, 비밀 해석 호출에 그 이름 없음 |

---

### S5-1: 실제 코드에 맞춘 상세화

- [x] PROGRESS §5(S2 부분)와 00 §3·§7, 07의 S5 절, 01의 S5 관련 절을 읽고 실제 코드(`crates/protocol`·`crates/cli/src/daemon`·`crates/core`·`crates/secrets`·`crates/cli/src/{doctor,mcp}.rs`·`app/src/{rpc,shell,ui/code,features}`·`app/src-tauri`)로 위 "실제 코드 계약"을 확정했다.
- [x] v0.9.0 이식 대상의 시험 수를 셌다(위 A–D).
- [x] 회귀 입력을 S5-2·S5-3·S5-5·S5-7 시험 이름으로 옮겼다(위 표).
- [x] S5-2–S5-9를 아래에 펼쳤다. 07의 S5 절은 작업 묶음 표와 이 문서 링크로 줄였다.
- [ ] 별도 검토·Biome CI 뒤 커밋(오케스트레이터):
```bash
git add docs/superpowers/plans/2026-10-08-devbox-v1/{07b-s5-tools,07-s4-s6,PROGRESS}.md   # PROGRESS(§1 트랙 2 줄·§2·§4.1 S5 상세화 행·§5)를 반드시 함께 커밋
git -c user.name=jihoon22-lee -c user.email=188744452+jihoon22-lee@users.noreply.github.com commit -m "docs(plan): detail S5 against the S2 code"
```

---

### S5-2: `crates/api` — HTTP 실행 (묶음 AA)

**Files:**
- Modify: `Cargo.toml`(member `crates/api`, workspace 의존성 줄 `devbox-api`·`reqwest`·`url`·`cookie_store`·`hyper`·`hyper-util`·`http-body-util`·`rcgen`. `aho-corasick`·`nix`·`notify`는 `crates/api/Cargo.toml` 직접 줄), `crates/cli/Cargo.toml`, `xtask/{Cargo.toml,src/main.rs}`, `crates/cli/src/daemon/{routes,mod,state}.rs`(줄 추가), `app/src/rpc/messages.ts`(줄 추가), 생성 `app/src/rpc/gen/rpc.ts`(`pnpm gen`)
- Create: `crates/api/Cargo.toml`, `crates/api/src/{lib,error,model,validate,template,redact,cookies,tls,auth,oauth_loopback,service,events}.rs`, `crates/api/src/http/{mod,build,body,redirect,stream}.rs`, `crates/api/src/history/{mod,files,index}.rs`
- Create: `crates/cli/src/daemon/tools_routes.rs`(S5 등록 모음)
- Test: `crates/api/tests/support/mod.rs`(hyper 1 fixture 서버: 받은 요청 기록·연결 수·지연·리디렉션·에코), `crates/api/tests/{http_send,http_redirect,http_auth,http_tls,history}.rs`, `crates/api/src/template_tests.rs`, `crates/cli/tests/api_send.rs`(실제 데몬 RPC·스트림 왕복)

**Interfaces → Produces:**
```rust
// crates/api/src/lib.rs (요점)
method!(pub ApiSend = "api.send", stream, SendRequest => devbox_protocol::StreamOpened, ApiError);
method!(pub ApiHistory = "api.history", query, HistoryQuery => HistoryPage, ApiError);
method!(pub ApiHistoryGet = "api.history_get", query, HistoryId => HistoryEntry, ApiError);
method!(pub ApiCookies = "api.cookies", query, CookieJarRef => CookieList, ApiError);
method!(pub ApiCookiesClear = "api.cookies_clear", mutation(always), CookieJarRef => devbox_protocol::Done, ApiError);
pub fn export<X: Exporter>(x: &mut X) { /* 위 메서드 */ }
pub use service::ApiService;   // S5-2에는 devbox.db 테이블이 없어 MIGRATIONS를 만들지 않는다(S5-3에서 store.rs와 함께 추가)

// crates/api/src/model.rs (요점, 모두 #[derive(Serialize, Deserialize, ts_rs::TS)] #[serde(rename_all = "camelCase", deny_unknown_fields)])
pub struct SendRequest { pub snapshot: RequestSnapshot, pub env: EnvSnapshot, pub run_id: Option<String>,
    pub origin: Option<RequestOrigin>, pub cookie_jar: Option<String> }
pub struct RequestSnapshot { pub method: String, pub url: String, pub query: Vec<Pair>, pub headers: Vec<HeaderRow>,
    pub cookies: Vec<Pair>, pub body: Body, pub auth: Auth, pub tls: TlsOptions, pub timeout_ms: i64, pub follow_redirects: bool }
pub struct HeaderRow { pub row: u32, pub name: String, pub value: String } // row = 화면 행 번호(비활성 행 포함 순번)
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Body { None, Raw { text: String, content_type: Option<String> }, Json { text: String },
    Form { fields: Vec<Pair> }, Multipart { parts: Vec<Part> }, Binary { path: String },
    Graphql { query: String, variables: String, operation_name: Option<String>, method: GraphqlMethod } }
#[serde(tag = "kind")] pub enum Part { Text { name: String, value: String, content_type: Option<String> },
    File { name: String, path: String, file_name: Option<String>, content_type: Option<String> } }
#[serde(tag = "kind")] pub enum Auth { None, Basic { username: String, password: String }, Bearer { token: String },
    ApiKey { name: String, value: String, placement: KeyPlacement },
    Oauth2ClientCredentials { token_url: String, client_id: String, client_secret: String, scope: Option<String> },
    Oauth2AuthCode { authorization_url: String, token_url: String, client_id: String, client_secret: Option<String>, scope: Option<String> } }
pub struct TlsOptions { pub verify: bool, pub ca_path: Option<String>, pub client_cert_path: Option<String>, pub client_key_path: Option<String> }
pub struct EnvSnapshot { pub variables: Vec<EnvVar> }  // 화면이 환경+세션을 합친 값. 값 안의 {{secret:X}}는 그대로
pub struct EnvVar { pub name: String, pub value: String, pub sensitive: bool } // 캡처 값·민감 이름·알려진 토큰 패턴이면 화면이 true
pub struct RequestOrigin { pub root: CollectionRoot, pub path: String }   // S5-3에서 씀
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SendEvent { Started { history_id: String, sent_at_ms: i64 }, Authorize { url: String },
    Head { status: u16, reason: String, http_version: String, headers: Vec<Pair>, elapsed_ms: i64, redirects: Vec<Redirect> },
    Body { data_base64: String },           // 원본 최대 48KiB씩
    End { size_bytes: i64, elapsed_ms: i64, binary: bool, history_truncated: bool },
    Failed { error: devbox_protocol::message::WireError } }

// crates/api/src/error.rs
domain_error! { pub enum ApiError {
    InvalidHeader { index: u32, field: String, reason: String } = "api.invalid_header",
    InvalidUrl { reason: String } = "api.invalid_url", InvalidMethod = "api.invalid_method",
    InvalidBody { reason: String } = "api.invalid_body", VariableMissing { names: Vec<String> } = "api.variable_missing",
    SecretMissing { names: Vec<String> } = "api.secret_missing", SecretsLocked = "secrets_locked",
    SecretKeyMismatch = "secrets.key_mismatch", FileMissing { part: u32 } = "api.file_missing",
    TooLarge { limit: u64 } = "api.too_large", TlsInvalid { reason: String } = "api.tls_invalid",
    OauthFailed { stage: String } = "api.oauth_failed", Network { kind: String } = "api.network",
    StreamUnsupported = "api.stream_unsupported", HistoryNotFound = "api.history_not_found",
} }
```
- `ApiService::new(db: Db, index: IndexDb, paths: Paths, projects: Arc<ProjectsService>, secrets: Arc<SecretsService>) -> Result<Self, CoreError>`: `index.ensure_domain("api", 1, API_INDEX_SQL)?`가 `true`면 기록 파일로 색인을 다시 만든다(백그라운드, 완료 전 `api.history`는 있는 행만).
- `ApiService::send(&self, ctx: Ctx, p: SendRequest) -> R<StreamOpened>` 순서: ① 모양 검사(메서드 RFC 9110 token, URL `http`/`https`·userinfo 거절·fragment 제거, 본문 상한 텍스트 8MiB·multipart 50부분) ② `{{name}}`·`${name}` 변수 치환(활성 필드만: URL·query·헤더·쿠키·활성 본문·활성 인증·TLS 경로). `{{name}}`이 없으면 `api.variable_missing{ names }`(v0.9.0 "알 수 없는 변수는 그대로"를 바꾼 적응, A 표), 정의 안 된 `${name}`은 문자 그대로(v0.9.0 규칙) ③ `{{secret:NAME}}` 수집 → `SecretsService::resolve` → 매핑 오류 ④ 헤더 이름 token·값 field-value 검사(앞뒤 OWS 제거 뒤 CTL 거절, HTAB 허용) → `api.invalid_header{ index: row, … }` ⑤ 파일 부분·이진 본문·TLS 파일 존재 확인(오류 세부에 경로 없음) ⑥ 여기까지 실패면 RPC 오류, 성공해야 `ctx.open_stream()` ⑦ 스트림 작업 spawn 후 즉시 `StreamOpened` 반환.
- 송신 작업: reqwest 클라이언트(요청별 TLS·리디렉션·쿠키 jar 설정) → `Started` → (auth code면 `Authorize{ url }`) → 응답 `Head` → 본문 `Body` 조각 → `End`. 오류는 `Failed{ error }`(`api.network{ kind: dns|connect|tls|timeout|protocol|cancelled }`, URL 없음). `tx.closed()`가 취소되면 요청 future를 drop하고 기록 결과를 `cancelled`로 쓴다.
- 가림(`redact.rs`): 이 요청에서 푼 비밀 값들로 aho-corasick 스트리밍 가림기를 만든다(가장 긴 값−1바이트를 다음 조각까지 보류). 응답 헤더·본문·오류 문장에 적용, `[REDACTED]`(S2 계약 문자열).
- 리디렉션(`http/redirect.rs`): 최대 10회, 다른 origin으로 가면 `Authorization`·`Cookie`·`Proxy-Authorization`·API key 헤더·클라이언트 인증서를 떼고 307/308 본문은 같은 origin만, 302/303 POST는 GET으로(v0.9.0 시험). 모든 hop과 본문이 `timeoutMs` 하나를 공유한다.
- 인증: Basic·Bearer·API key(헤더·쿼리). OAuth2 client credentials는 토큰 URL에 한 번 요청하고 `(tokenUrl, clientId, scope, secret digest)`로 메모리 캐시(만료 30초 전 갱신, 재시작 시 없음). auth code + PKCE는 `oauth_loopback.rs`가 `127.0.0.1:0`의 `/callback`을 인증 중에만 열고(state 무작위·5분 시한·1회용) `Authorize{ url }` 이벤트로 화면이 `openExternal`로 브라우저를 연다. 토큰은 메모리만.
- 쿠키(`cookies.rs`): `cookieJar` 키(컬렉션 키)별 `cookie_store` 저장, `<data>/api-cookies/<sha256(key)>.json`(0600). 도메인·경로·Secure 규칙은 `cookie_store`. 요청의 Cookie 행은 한 `Cookie` 헤더로 순서대로 합치고 jar와 이름이 겹치면 행이 우선.
- TLS(`tls.rs`): CA·클라이언트 인증서·키 PEM 경로(WSL 절대 경로), `verify: false`는 요청 단위이며 기록에 표시한다. 잘못된 PEM은 연결 전 `api.tls_invalid{ reason }`(고정 문구).
- 기록(`history/`): `api-history/<YYYY-MM-DD>/<historyId>.json`(0600, 원자적 쓰기), 응답 본문 1MiB 초과는 `<historyId>.body`(최대 16MiB, 넘으면 `truncated`). **기록 정화:** 기록의 요청은 치환 전 snapshot(`{{name}}`·`{{secret:NAME}}` 그대로)과 치환 뒤 메서드·URL이다. 인증은 종류와 참조만 남기고 직접 입력 값(Basic 비밀번호·Bearer token·API key 값·client_secret)은 `[REDACTED]`, 파생 헤더(`Authorization`·`Cookie`·`Proxy-Authorization`)는 이름만 남긴다. 치환 뒤 URL·응답 헤더·본문에는 같은 가림기로 이 요청에서 푼 비밀 값, `sensitive` 변수 값(캡처 토큰 포함), OAuth access/refresh token·client_secret, Basic `user:pass`와 그 Base64를 `[REDACTED]`로 바꾼다. multipart 파일은 이름만 남긴다. 토큰 자체는 메모리만(위 인증). 색인 `api_history(id TEXT PRIMARY KEY, sent_ms INTEGER, day TEXT, method TEXT, url TEXT, status INTEGER, elapsed_ms INTEGER, size INTEGER, outcome TEXT, run_id TEXT, origin TEXT)` + `sent_ms` 색인. 시작 때와 24시간마다 30일 넘은 날짜 폴더·행 삭제. `api.history{ before?: String, limit: u32(1–256), origin?: RequestOrigin } -> HistoryPage{ items, next }`(JSON 768KiB 이하), `api.history_get{ id } -> HistoryEntry`(본문은 1MiB까지 `bodyBase64` + `bodyTruncated`).
- 로그: `tracing::debug!(method = "api.send", elapsed_ms, outcome)`만.
- 데몬: `tools_routes.rs`의 `pub(super) fn register(router: Router<Services>) -> Router<Services>`가 `ApiSend` 등을 `s.api.send(ctx, p)`로 등록한다.

**단계별 실행:**
- [ ] **① 뼈대·실패 테스트:** member·Cargo.toml·빈 `lib.rs`·시험 파일을 연결하고 `cargo check -p devbox-api --target x86_64-unknown-linux-musl`로 rustls(ring)·reqwest 정적 빌드를 먼저 확인한다(실패면 기능 선택을 고친 뒤 진행). 그다음 아래 핵심 테스트를 쓴다. 출발 assertion:
```rust
// crates/api/tests/http_send.rs (요점)
for (case, snapshot, env, expect) in cv_f05_cases(&server) {          // 회귀 표의 ①–⑥
    let err = send_rpc(&api, snapshot, env).await.expect_err(case);
    assert_eq!(err.code(), "api.invalid_header", "{case}");
    assert_eq!((err.detail()["index"].as_u64(), err.detail()["field"].as_str()), expect, "{case}");
    assert!(!err.detail_text().contains("Injected") && !err.detail_text().contains("fixture-secret"), "{case}");
}
assert_eq!(server.accepted_connections(), 0);
assert_eq!(history_rows(&api).await, 0);
```
- [ ] **② 실패 확인:** `python3 scripts/verify-resources.py --no-lock -- cargo test -p devbox-api --test http_send invalid_header_name_is_rejected_before_sending -- --test-threads=2` → `ApiService::send` 미구현(컴파일 오류 또는 assertion)으로 실패해야 한다. RED 출력은 `devbox-verification/s5-2/`에 둔다.
- [ ] **③ 구현:** 위 Interfaces 순서로 만든다. 핵심:
```rust
// crates/api/src/service.rs (요점)
pub async fn send(&self, ctx: Ctx, p: SendRequest) -> R<StreamOpened> {
    let resolved = self.prepare(&p).await?;           // ①–⑤, 실패는 Fail::Domain(ApiError)
    let ends = ctx.open_stream().map_err(|_| ApiError::StreamUnsupported)?;
    let stream_id = ends.id;
    let (history, redactor) = (self.history.clone(), Redactor::new(resolved.secret_values()));
    self.tasks.spawn(async move {
        let closed = ends.tx.closed();
        let outcome = tokio::select! {
            biased;
            _ = closed.cancelled() => Outcome::Cancelled,
            r = http::execute(resolved, &ends.tx, &redactor) => r,   // Head/Body/End 쓰기
        };
        history.record(outcome).await;                // 파일 → 색인, 실패는 tracing 오류 코드만
        ends.tx.finish().await;
    });
    Ok(StreamOpened { stream_id })                    // 응답 대기·credit 대기 전에 반환
}
```
  `crates/cli/src/daemon/mod.rs`에 `IndexDb::open`·`ApiService::new`·`Services` 줄, `tools_routes.rs` 등록, `xtask` export와 `visit::<devbox_api::SendEvent>`, `messages.ts`에 `api.*` 문구(입력 오류는 "입력란을 확인해 주세요", `api.invalid_header`는 `(d) => ({ title: \`헤더 ${row+1}행 ${이름|값}이 올바르지 않습니다\` })`)를 더하고 `pnpm gen`을 실행한다.
- [ ] **④ 통과:** 같은 RED 명령 → GREEN. 직접 영향: `cargo test -p devbox-api`(시험 파일별), `cargo test -p devbox-cli --test api_send`, `cargo test -p devbox-cli daemon::routes::tests::every_declared_method_is_routed`, `cargo run -q -p xtask -- gen-ts --check`, `pnpm --filter app exec vitest run src/rpc/messages.test.ts --maxWorkers=2`, `pnpm --filter app typecheck`, `cargo check -p devbox-api --target x86_64-unknown-linux-musl`, 600줄 검사(`wc -l`). 모두 supervisor `--no-lock` 아래에서.
- [ ] **⑤ 검토·커밋:** 별도 검토(00 §3.4: 회귀 표의 CV-F05·AU-AS 시험 포함 여부, 로그·오류에 URL/값 없음, 동결 경로 무변경) → Biome → 커밋을 셋으로 나눈다.
  1. `feat(api): send HTTP requests from validated snapshots`(model·validate·template·http·stream·service·등록)
  2. `feat(api): add request authentication, cookies and TLS options`(auth·oauth_loopback·cookies·tls)
  3. `feat(api): record API history in files and the shared index`(history·`IndexDb` 연결)

**핵심 테스트:**
- `invalid_header_name_is_rejected_before_sending`(CV-F05, 회귀 표 입력 ①–⑦)
- `disabled_body_and_auth_are_not_sent`, `none_body_ignores_hidden_draft_and_rejects_unknown_kind`, `active_auth_ignores_inactive_secret_references`(AU-AS-01·07)
- `query_and_form_are_url_encoded`(공백·`+`·한글·중복·빈 값·fragment), `query_form_preserves_reserved_characters_duplicates_and_fragment`, `append_query_and_form_keep_existing_behavior`(AU-AS-02)
- `secret_reference_is_resolved_on_send_and_kept_as_reference_in_history`: fixture 비밀 `TOKEN`(소유 test 키), 헤더 `Bearer {{secret:TOKEN}}` → 서버는 값을 받음, 서버가 그 값을 본문·헤더로 되돌려 줌 → 스트림·기록 파일에 `[REDACTED]`, 기록 요청에 `{{secret:TOKEN}}`, 기록 파일 bytes에 값 없음
- `secrets_locked_or_missing_sends_nothing`: 잠김·없음 → `secrets_locked`·`api.secret_missing{ names }`, 서버 연결 0
- `cancel_aborts_the_in_flight_request`: 서버가 응답 머리 전 대기 → 화면 쪽 `StreamClose`(시험은 `StreamTx::closed` 취소) → 서버가 1초 안에 연결 종료를 봄, 기록 `outcome = cancelled`, `End` 없음
- `two_streams_cancel_independently`: 두 송신 중 하나만 닫으면 다른 쪽은 끝까지 받음
- `variables_resolve_both_styles_in_active_fields_only`(environments 치환 7개 이식 + 적응 1 `unknown_braced_variable_is_rejected_before_sending`, `template_tests.rs`)
- 리디렉션·GraphQL·multipart·쿠키·TLS·OAuth: 위 C 표의 이식 37개 중 S5-2 해당분(`tls_stream_configuration_is_not_silently_ignored` 제외)과 적응 시험
- `live_network_error_is_generic_and_contains_no_request_secret`: 닫힌 포트·잘못된 이름 → `api.network{ kind }`, 오류 JSON·`tracing` 캡처에 URL·비밀 없음
- `history_never_contains_tokens_or_basic_credentials`: 직접 입력 Basic `user:pass`, client credentials(소유 hyper 토큰 서버가 고정 access token 발급, literal client_secret), 캡처 토큰 변수(`sensitive: true`)를 헤더·query에 사용 → 기록 JSON·`.body` 파일 bytes에 password·`dXNlcjpwYXNz`류 Base64·access token·client_secret·토큰 값 검색 0, `{{token}}` 참조는 있음. `graphql_history_masks_query_literals`(query literal·variables 안 비밀, 깨진 literal도 fail-closed), `multipart_history_keeps_no_file_paths_or_text_secrets`
- `history_is_indexed_and_pruned_after_30_days`(가짜 시계, `api.history` 페이지·`next`, 31일 전 폴더·행 삭제), `history_index_is_rebuilt_when_the_domain_version_changes`
- `crates/cli/tests/api_send.rs`: `TestDaemon::fixture()`·`launch()`·`client()`로 실제 `api.send` 스트림 → `Started`·`Head`·`Body`·`End` 순서, 다른 RPC(`system.ping`)가 credit 소진 중에도 응답

---

### S5-3: 컬렉션·환경·가져오기·실행기·코드 생성 (묶음 AA)

**Files:**
- Modify: `crates/api/src/lib.rs`(메서드·주제 줄), `crates/cli/src/daemon/tools_routes.rs`, `app/package.json`(`jsonc-parser`·`yaml`), `app/src/shell/Shell.tsx`(줄 추가), `app/src/rpc/messages.ts`, `docs/ui-terms.md`(행 추가)
- Create(Rust): `crates/api/src/collections/{mod,files,watch,reviews}.rs`, `crates/api/src/store.rs`(`MIGRATIONS`: `api_reviews(root TEXT, path TEXT, digest TEXT, reviewed_ms INTEGER, PRIMARY KEY(root,path))`) · Test: `crates/api/tests/collections.rs`
- Create(TS 이식): `app/src/features/tools/api/lib/`
  - `template.ts`(v0.9.0 generated 타입 중 이식 lib이 쓰는 `RequestTemplate`·`AuthConfig`·`KeyValue`·`RequestHeader`·`RequestCookie`·`MultipartPart`·`GraphqlRequest`·`ApiResponse` 모양만 TS로 다시 선언: "타입 재결합")
  - `importers/{index,curl,postman,insomnia,bruno,har,preview,model,parsing}.ts` + `importers/fixtures/{postman-v21,insomnia-v4}.json`
  - `codegen/{index,shell,javascript,python,go,rust}.ts`, `assertions.ts`, `assertionRegex.ts`, `assertionRegex.worker.ts`, `captures.ts`, `jsonPath.ts`, `graphql/{build,variables,response,mask}.ts`, `openapi/{document,limits,operations,parameters,bodies,security,preview}.ts`, `headers.ts`, `multipart.ts`, `cookies.ts`, `binary.ts`, `secretPatterns.ts`, `references.ts`, `limits.ts`, `storage.ts`(파일 저장 정화, v0.9.0 `sanitizeRequestForPersistence` 적응), `lineDiff.ts`
  - 같은 이름의 `*.test.ts`(위 A 표의 이식·적응분)
- Create(TS 새): `app/src/features/tools/api/model/{draft,snapshot,requestFile,envFile,fromTemplate}.ts`, `runner/{runner,session,plan}.ts` + 시험
- Create(화면): `app/src/features/tools/{ToolsPage,ToolsList,ToolsCommands}.tsx`, `tools/search.ts`, `tools/api/{RequestTabs,RequestEditor,UrlBar,ParamTable,BodyEditor,AuthEditor,ResponseView,ResponseOrigin,CollectionTree,EnvEditor,RunnerPanel,RunPlanView,ImportDialog,CodegenDialog,HistoryList,ResponseCompare}.tsx`, `tools/api/{useSend,useCollections,store}.ts` + 컴포넌트 시험
- Create(E2E): `app/e2e/api.spec.ts`, `app/e2e/api-fixture-server.ts`(127.0.0.1:0 node http 서버, 소유 fixture)

**Interfaces → Produces:**
- 컬렉션 루트: `CollectionRoot = { kind: "personal" } | { kind: "project", projectId }`. personal = `Paths.config_dir.join("api")`(0700 생성), project = `<Project.path>/.devbox/api`.
- 파일 배치: 요청 하나 = `<폴더…>/<이름>.request.json`, 환경 = `env/<이름>.json`, 선택 `collection.json`(순서·실행기 기본값). 데몬은 **텍스트 단위**로만 다룬다(스키마는 TS 소유, `serde_json::Value` 경계 금지).
```rust
method!(pub ApiCollections = "api.collections", query, CollectionsQuery => CollectionList, ApiError);      // 루트별 파일 목록(경로·크기·digest·reviewed), 루트당 2,000개
method!(pub ApiCollectionRead = "api.collection_read", query, FileRef => FileText, ApiError);              // { text, digest }, 1MiB 이하 UTF-8
method!(pub ApiCollectionWrite = "api.collection_write", mutation, WriteFile => Written, ApiError);       // expect: Digest(d) | Absent → 다르면 api.collection_conflict{ digest }
method!(pub ApiCollectionDelete = "api.collection_delete", mutation(always), DeleteFile => devbox_protocol::Done, ApiError);
method!(pub ApiCollectionReview = "api.collection_review", mutation, ReviewFile => devbox_protocol::Done, ApiError); // (root, path, digest) 기록
topic!(pub ApiCollectionsChanged = "api.collections_changed", CollectionsChange);                          // { root, rev }, 300ms 디바운스
```
  경로 검사: 상대 경로, `.`·`..`·빈 조각·NUL 거절, `.json` 끝, 부모 realpath가 루트 안, 마지막 조각 `O_NOFOLLOW`, 같은 폴더 임시 파일 → rename(새 파일은 `expect: Absent`일 때 이미 있으면 `api.collection_conflict`). 오류 `api.collection_path_invalid`·`api.collection_conflict{ digest }`·`api.collection_too_large`·`api.collection_not_found`·`api.project_not_found`.
- 비밀 재검토: 목록 항목 `reviewed`는 프로젝트 루트이면서 파일 텍스트에 `{{secret:`·multipart 파일 부분·MCP stdio 명령이 있을 때만 의미가 있다. `api_reviews`의 digest가 현재와 같으면 `true`. 개인 루트는 항상 `true`. 가져오기·화면 저장은 쓴 digest로 검토를 함께 기록한다(사용자가 본 내용이므로). 밖에서 바뀌면(`git pull`) 다시 필요하다.
- TS 파일 형식(`requestFile.ts`): `{ version: 1, name, request: StoredRequest(활성 종류만), assertions, captures, requiresSecretReview?, savedMessages? }`. 읽을 때 모르는 최상위·하위 필드를 그대로 두고 쓸 때 다시 넣는다(`{ ...original, ...known }`).
- 초안(`model/draft.ts`): `RequestDraft{ method, url, query: Row[], headers: Row[], cookies: Row[], bodyKind, bodies: Partial<Record<BodyKind, BodyDraft>>, authKind, auths: Partial<Record<AuthKind, AuthDraft>>, tls, timeoutMs, followRedirects, assertions, captures }`, `Row{ id, enabled, name, value }`. `setBodyKind`·`setAuthKind`는 다른 종류 초안을 지우지 않는다. `toSnapshot(draft): RequestSnapshot`(활성 행만, `HeaderRow.row` = 화면 행 번호), `toStored(draft, root)`(활성 종류만 → `storage.ts`의 `sanitizeForStorage(stored, root) → { stored, masked: MaskedField[] }`), `secretNames(snapshot)`.
- **파일 저장 정화(01 §9.1-14):** 모든 저장(저장·새 요청으로 저장·가져오기·웹훅에서 만든 요청)은 `sanitizeForStorage`를 지난다. 민감 위치(`Authorization`·`Cookie`·`Proxy-Authorization`·API key 헤더, 인증 비밀번호·token·client_secret, 민감 이름의 query·form·JSON 필드, URL userinfo, 알려진 토큰 패턴, GraphQL query literal·variables)의 직접 입력 값은 `[REDACTED]`로 바꾸고 `{{secret:NAME}}`·`{{name}}` 참조는 그대로 둔다. 프로젝트 루트는 multipart 파일 절대 경로를 지우고 파일 이름만 남긴다(개인 루트는 WSL 경로 유지). `masked`가 있으면 저장 전에 "직접 입력한 비밀 N개는 파일에 저장하지 않습니다"와 [비밀로 옮기기](`secrets.set` 뒤 참조로 바꿈)·[가리고 저장]을 보인다. 읽은 요청에 `[REDACTED]`가 남아 있으면 화면은 보내지 않고 "가린 값을 다시 입력해 주세요", 실행기는 그 이유로 건너뛴다.
- 실행기(`runner/runner.ts`):
```ts
export interface Provenance { runId: string; requestId: string }
export interface SessionValue { value: string; from: Provenance | "environment" }
export interface RunEntry { id: string; name: string; draft: RequestDraft; reviewNeeded: boolean; produces: string[] }
export type StepStatus = "passed" | "failed" | "error" | "skipped";
export async function runCollection(entries: RunEntry[], environment: Pair[], session: Session,
  options: { stopOnFailure: boolean; delayMs: number }, deps: RunDeps, signal: AbortSignal,
  onStep: (step: RunStep) => void): Promise<RunSummary>;
```
  규칙: 실행 전에 `plan.ts`가 변수별 생산 요청(이번 실행 안, 순서대로)을 계산한다. 항목 i가 쓰는 변수 v(활성 필드만, AS-07)에 대해 이번 실행의 앞선 생산 요청 P가 있으면 P가 `passed`이고 v를 이번 실행에서 캡처했을 때만 쓴다. 아니면 송신하지 않고 `skipped`·"출처 요청 실패: <P.name>"(CV-F04). 앞선 생산 요청이 없으면 세션(이전 실행 포함)·환경 값을 쓴다(토큰 갱신). 건너뛰기·취소·실패한 생산 요청의 세션 값은 지우지 않고 "이번 실행에서 사용 불가"로만 둔다. 재검토 필요(`reviewNeeded` 또는 파일 `requiresSecretReview`)는 "비밀 검토가 필요합니다", 파일 부분 경로 없음은 "파일을 다시 선택해야 합니다"로 건너뛴다. `stopOnFailure`는 `failed`·`error`와 함께 출처 차단 `skipped`에서도 멈춘다. 취소는 현재 스트림 `end()` 후 나머지 "실행을 중지했습니다". 한도: 항목 200개, 대기 0–5000ms.
- 송신(`useSend.ts`): `client.openStream("api.send", { snapshot: toSnapshot(draft), env, runId, origin, cookieJar }, …)`. 이벤트 NDJSON을 `SendEvent`로 해석해 응답 상태를 만든다. 응답 상태는 출처 `{ name, method, url, sentAtMs, snapshotHash }`를 가진다(AR-F08). 화면 메모리의 본문은 10MiB까지, 넘으면 "기록에서 보기".
- 화면(01 §8.2 도구): 목록은 `ToolsList`(API 컬렉션 트리 · 웹훅 · 변환 도구 · 개발 환경 점검), 본문은 `SplitPane`으로 요청 | 응답(1100px 미만이면 위아래). 요청 탭(`RequestTabs`)은 열린 요청마다 초안을 메모리(zustand `store.ts`)에 두고 dirty 점을 탭·트리에 보인다. "저장"(Ctrl+S, `expect: Digest`)·"새 요청으로 저장"(`expect: Absent`)을 나눈다. 보내기 Ctrl+Enter. 입력 오류(`api.invalid_header`)는 해당 행 아래에 보인다.
- 실행기 화면(`RunnerPanel`·`RunPlanView`): 실행 전에 순서와 생산→소비 관계(어느 요청의 캡처를 어느 요청이 쓰는지)를 보여 주고 끌어서 순서를 바꾼다. 단계별로 변수 출처(환경·세션·캡처한 생산 요청)와 건너뛴 이유를 보인다.
- 가져오기(`ImportDialog`): 파일 선택 또는 붙여넣기 → 형식 감지(`importers/index.ts`) → 미리보기(만들 파일·경고·가린 인증) → 루트 선택 → 파일들을 `api.collection_write{ expect: Absent }`로 쓴다. 중간 실패 시 이번에 만든 파일만 지운다(importTransaction 적응). OpenAPI URL은 `api.send` GET으로 받은 본문 텍스트를 넘긴다.
- 코드 생성(`CodegenDialog`): curl·fetch·axios·python requests·Go·Rust reqwest. 비밀은 `{{secret:NAME}}` 자리 표시로 남긴다(v0.9.0 규칙).
- 응답 비교(`ResponseCompare`): 같은 출처의 이전 기록(`api.history{ origin }`)과 `lineDiff.ts` → `ui/DiffView`.
- 섹션 연결: `Shell.tsx` `pages`에 `"/tools": { component: ToolsPage, validateSearch: toolsSearch }`, `<ToolsCommands />`. `toolsSearch`는 `{ view?: "api"|"webhooks"|"transforms"|"env", root?, path?, tool? }`.

**단계별 실행(과제 안 커밋 4개):**
- [ ] **3a 이식 lib** ① 실패 테스트: v0.9.0 시험 파일을 위 A 표대로 `app/src/features/tools/api/lib/**`로 복사하고 import 경로를 새 위치로 바꾼다(구현 파일은 아직 없음). ② `python3 scripts/verify-resources.py --no-lock -- pnpm --filter app exec vitest run src/features/tools/api/lib --maxWorkers=2` → 모듈 없음으로 실패. ③ 구현 파일을 복사하고 `template.ts` 타입 재결합·분할 표대로 나눔·`@devbox/*` 의존 제거(`confirmAction` → 호출자에게 넘김). axios·Rust 코드 생성 시험을 새로 쓰고 구현. ④ 같은 명령 GREEN + `pnpm --filter app typecheck`, 600줄 검사. 제외한 시험 목록과 이유를 artifact `s5-3/ported-tests.json`에 남긴다. ⑤ 검토·커밋 `feat(tools): port API request libraries from v0.9.0`.
- [ ] **3b 컬렉션 파일** ① `crates/api/tests/collections.rs`: `write_rejects_stale_digest_and_preserves_bytes`(d1로 읽고 밖에서 고친 뒤 d1로 쓰기 → `api.collection_conflict{ digest: d2 }`, 파일 bytes는 밖에서 고친 것), `save_as_new_never_overwrites`(Absent로 있는 경로 → conflict), `paths_outside_the_root_and_symlinks_are_rejected`(`../x.json`·루트 안 symlink → 밖), `personal_root_uses_the_channel_config_dir`(accept 인스턴스 `Paths` → `devbox-accept/api`, prod 폴더 생성 0), `external_edit_emits_collections_changed`(notify 300ms), `review_is_tied_to_the_file_digest`(검토 뒤 내용 변경 → `reviewed: false`). ② `python3 scripts/verify-resources.py --no-lock -- cargo test -p devbox-api --test collections -- --test-threads=2` RED. ③ 구현·등록·`pnpm gen`. ④ GREEN + `every_declared_method_is_routed`·gen-check. ⑤ 커밋 `feat(api): read and write collection files with change watching`.
- [ ] **3c 초안·저장 정화·실행기** ① `model/draft.test.ts`(`body_kind_switch_keeps_each_draft`), `lib/storage.test.ts`·`model/requestFile.test.ts`(위 핵심 테스트의 적응 18개), `model/requestFile.test.ts`(`collection_files_round_trip_without_losing_unknown_fields`: 모르는 최상위 `x-team`·요청 안 `x-extra`가 쓰기 뒤에도 같은 값), `runner/runner.test.ts`(회귀 표의 CV-F04 4개 + 이식 9·적응 2). ② vitest RED. ③ 구현. ④ GREEN. ⑤ 커밋 `feat(tools): run API collections with capture provenance`.
- [ ] **3d 화면·E2E** ① 컴포넌트 시험: `RequestEditor.test.tsx`(`save_and_save_as_new_are_distinct`, 잘못된 헤더 오류가 해당 행 아래), `ResponseView.test.tsx`(`response_origin_marks_edited_request`), `RunnerPanel.test.tsx`(실행 전 생산→소비 표시, 건너뛴 이유), `ImportDialog.test.tsx`(중간 실패 정리), `ToolsPage.test.tsx`(목록 4묶음·1100px 미만 세로 배치). E2E `app/e2e/api.spec.ts` 2개: (1) cURL 붙여넣기 가져오기 → 보내기 → 응답 상태·본문·출처 표시 (2) 로그인(토큰 캡처) → 내 정보(토큰 사용) 실행기, 두 번째 요청이 fixture 서버에서 받은 토큰이 새 값. ② vitest RED(컴포넌트 없음). ③ 구현. 컴포넌트는 300줄 이하. ④ vitest GREEN·typecheck. E2E는 S5-9에서 한 번에 돌린다(여기서는 `pnpm --filter app exec playwright test --list`로 수만 확인). ⑤ 커밋 `feat(tools): add the API request workspace`.

**핵심 테스트:**
- 이식한 TS 시험 전부(A 표의 이식·적응분)
- `captured_value_from_failed_request_is_not_used`, `skipped_or_cancelled_producer_blocks_the_consumer`(① 비밀 재검토 ② 파일 누락 ③ 취소 × `stopOnFailure` 참·거짓), `token_refresh_can_read_the_previous_run_value`, `other_producers_and_environment_values_are_kept`
- `body_kind_switch_keeps_each_draft`(AR-F06 + 비활성 초안의 없는 비밀)
- `save_and_save_as_new_are_distinct`, `response_origin_marks_edited_request`
- `collection_files_round_trip_without_losing_unknown_fields`(TS) + `write_rejects_stale_digest_and_preserves_bytes`(Rust)
- `lib/storage.test.ts`(persistence 적응 8): `direct_secrets_in_json_form_url_auth_are_masked_and_references_kept`, `partial_reference_values_are_still_masked`, `form_token_and_password_fields_are_masked`, `known_token_patterns_are_masked_without_sensitive_names`, `project_root_drops_multipart_file_paths_and_stale_body`, `template_urls_and_broken_json_fail_closed`, `graphql_query_literals_and_variable_secrets_are_masked`, `non_graphql_request_does_not_store_stale_graphql_state`; oauth2·checks·tls 적응 5: `oauth_client_secret_literal_is_masked`, `inactive_authorization_url_is_ignored_for_client_credentials`, `invalid_checks_are_dropped_and_expected_tokens_masked`, `invalid_expressions_are_kept_for_reporting`, `invalid_tls_references_are_cleared_and_verify_defaults_on`
- `model/requestFile.test.ts`(fileCollection 적응 5): `file_names_are_deterministic_and_folders_kept`, `reserved_names_and_case_collisions_get_unique_paths`, `unsafe_paths_malformed_and_oversized_files_are_rejected`, `secrets_are_redacted_before_serialization_and_review_flag_survives`, `unicode_names_fit_filename_limits`
- `RequestEditor.test.tsx` `masked_secrets_are_announced_before_saving`(직접 입력 Bearer 값 → 저장 대화상자, 쓰인 텍스트에 값 없음), `redacted_value_blocks_sending`(가짜 send 0회). 송신 차단은 저장 때 기록한 가린 필드 위치(`masked`)로만 판정해 사용자가 본문에 직접 쓴 `[REDACTED]` 문자열은 막지 않는다(`user_typed_redacted_text_is_not_blocked`). E2E·수용 fixture(cURL·실행기·Postman)는 직접 입력 비밀 대신 `{{secret:…}}` 참조와 민감하지 않은 이름만 써서 저장 정화 뒤 송신 차단에 걸리지 않게 한다
- `personal_root_uses_the_channel_config_dir`, `review_is_tied_to_the_file_digest`
- E2E 2개(위)

---

### S5-4: WebSocket·SSE (묶음 AB)

**Files:**
- Modify: `Cargo.toml`(`eventsource-stream` 줄 추가만, 기존 `tokio-tungstenite` 줄은 그대로), `crates/api/Cargo.toml`(`tokio-tungstenite = { workspace = true, features = ["rustls-tls-webpki-roots"] }`), `crates/api/src/lib.rs`, `tools_routes.rs`, `messages.ts`
- Create: `crates/api/src/realtime/{mod,ws,sse,buffer}.rs` · Test: `crates/api/tests/realtime.rs`
- Create: `app/src/features/tools/api/realtime/{WsPanel,SsePanel,MessageList,MessageComposer}.tsx`, `realtime/messages.ts`(+ v0.9.0 `websocket.test.ts`·`sse.test.ts` 중 표시·상한 순수 시험 선택 이식)

**Interfaces → Produces:**
```rust
method!(pub ApiWsOpen = "api.ws_open", stream, WsOpen => devbox_protocol::StreamOpened, ApiError);   // { url(ws/wss), headers, subprotocols, tls, env, origin? }
method!(pub ApiSseOpen = "api.sse_open", stream, SseOpen => devbox_protocol::StreamOpened, ApiError); // { url, headers, tls, env, lastEventId?, origin? }
#[serde(tag = "type")] pub enum WsInput { Text { data: String }, Binary { data_base64: String }, Close { code: Option<u16>, reason: Option<String> } }
#[serde(tag = "type")] pub enum RealtimeEvent { Open { protocol: Option<String> },
    Message { direction: Direction, kind: MessageKind, data: Option<String>, data_base64: Option<String>, at_ms: i64 },
    SseEvent { id: Option<String>, event: Option<String>, data: String, retry_ms: Option<i64>, at_ms: i64 },
    Reconnecting { attempt: u32, last_event_id: Option<String> }, Dropped { count: u64 },
    Closed { code: Option<u16>, reason: Option<String> }, Failed { error: WireError } }
```
- 입력 검사(헤더 RFC 9110·변수·비밀·TLS)는 S5-2 `prepare`를 그대로 쓴다. 화면 → 데몬 입력은 스트림 input(`StreamEnds.input`)의 NDJSON `WsInput`.
- 흐름 제어(01 §5.1): 데몬은 소켓을 계속 읽어 `buffer.rs`의 상한(1,000개 또는 4MiB) 큐에 넣는다. credit이 없어 큐가 차면 새 메시지를 버리고 수를 센다. 쓰기가 재개되면 `Dropped{ count }`를 먼저 보낸다. 로그 tail처럼 소켓 읽기를 멈추지 않는다.
- SSE 재연결: 끊기면 `retry:` 값(없으면 3초)으로 최대 5회 다시 연결하며 `Last-Event-ID`를 보낸다. 사용자가 스트림을 닫으면 재연결하지 않는다.
- 화면: 메시지 목록(보낸·받은, 시각, `VirtualList`), 보내기 입력란(텍스트·JSON·이진 Base64), 저장한 메시지(요청 파일의 `savedMessages`).

**단계별 실행:**
- [ ] **① 실패 테스트:** `crates/api/tests/realtime.rs` — 소유 tungstenite 서버(127.0.0.1:0)와 hyper SSE 서버.
```rust
let (opened, mut io) = open_ws(&api, &ws_server).await?;     // 시험 opener가 StreamEnds를 노출
io.send(WsInput::Text { data: "ping".into() }).await;
assert_eq!(io.next_event().await, message(Direction::Received, "pong:ping"));
```
- [ ] **② 실패 확인:** `python3 scripts/verify-resources.py --no-lock -- cargo test -p devbox-api --test realtime -- --test-threads=2` → 미구현으로 실패.
- [ ] **③ 구현:** ws·sse·buffer와 등록·`pnpm gen`, 화면 패널.
- [ ] **④ 통과:** 같은 명령 + 화면 시험(`vitest run src/features/tools/api/realtime`)·typecheck·gen-check.
- [ ] **⑤ 검토·커밋:** `feat(api): stream WebSocket and SSE sessions`.

**핵심 테스트:**
- `ws_text_and_binary_messages_round_trip`(서버 에코, 이진은 Base64 왕복, 닫기 코드·이유)
- `ws_slow_client_drops_and_reports_count`: credit 0인 opener + 서버가 2,000개 전송 → 데몬 읽기는 계속(서버 쓰기 완료), credit 재개 뒤 첫 이벤트 `Dropped{ count > 0 }`, 그동안 다른 RPC 응답 유지
- `sse_reconnects_with_last_event_id`: 서버가 `id: 7` 뒤 연결 끊음 → 재연결 요청 헤더 `Last-Event-ID: 7`, 이벤트 순서 유지, 스트림 닫으면 재연결 0
- `tls_stream_configuration_is_not_silently_ignored`(C 표 이식: wss에 잘못된 CA → 연결 전 `api.tls_invalid`)
- `ws_invalid_header_is_rejected_before_connecting`(CV-F05와 같은 검사, 서버 연결 0)

---

### S5-5: gRPC·MCP 클라이언트·OAuth 2.1 (묶음 AC)

**Files:**
- Modify: `Cargo.toml`(`tonic`·`prost`·`prost-reflect`(serde)·`tonic-reflection`(client, dev는 server)·`protox`·`rmcp`), `crates/api/src/lib.rs`, `tools_routes.rs`, `messages.ts`
- Create: `crates/api/src/grpc/{mod,describe,call,json,metadata}.rs`, `crates/api/src/mcp/{mod,session,stdio,http,oauth}.rs` · Test: `crates/api/tests/{grpc,mcp_http}.rs`, `crates/cli/tests/api_mcp.rs`
- Create: `app/src/features/tools/api/grpc/{GrpcPanel,MethodPicker}.tsx`, `grpc/grpcDrafts.ts`, `app/src/features/tools/api/mcp/{McpPanel,McpCatalog}.tsx`, `mcp/session.ts` + 시험

**Interfaces → Produces:**
```rust
method!(pub ApiGrpcDescribe = "api.grpc_describe", query, GrpcTarget => GrpcCatalog, ApiError);
// GrpcTarget { target, tls, metadata: Vec<Pair>, env, protoFiles: Option<Vec<String>> } → reflection(v1, 없으면 v1alpha) 또는 protox
// GrpcCatalog { source: Reflection|ProtoFiles, services: Vec<{ name, methods: Vec<{ name, fullName, clientStreaming, serverStreaming, inputDefault: String /*JSON 텍스트*/ }> }> }
method!(pub ApiGrpcCall = "api.grpc_call", stream, GrpcCall => devbox_protocol::StreamOpened, ApiError);
// GrpcCall { target, method /*full*/, json: String, metadata, tls, env, timeoutMs, protoFiles?, origin? } — unary·server streaming만
// 이벤트: Headers{ metadata } · Message{ json } · Trailers{ code, message, metadata } · Failed{ error }
method!(pub ApiMcpConnect = "api.mcp_connect", stream, McpConnect => devbox_protocol::StreamOpened, ApiError);
// McpConnect { transport: Stdio{ command, args, cwd?, env: Vec<Pair> } | Http{ url, headers }, origin? }
// 입력 NDJSON: List{ what: tools|resources|prompts, cursor? } · Call{ id, name, argumentsJson } · Read{ id, uri } · GetPrompt{ id, name, argumentsJson }
// 이벤트: Ready{ serverName, serverVersion, capabilities } · Result{ id, json } · Authorize{ url } · Failed{ error } · Closed
```
- gRPC 메타데이터 이름·값은 HTTP 헤더와 같은 검사(소문자, `-bin`은 Base64)를 연결 전에 한다. JSON ↔ 동적 메시지는 `prost-reflect`의 serde. 입력 기본값은 메시지 서술자에서 만든 JSON 텍스트.
- proto 파일은 WSL 절대 경로이며 `protox::compile`(include 경로 = 각 파일의 부모)로 해석한다. protoc·build.rs·CI 변경이 없다.
- MCP는 `rmcp` 클라이언트: stdio는 `TokioChildProcess`(명령 문자열은 `validate_command_text`, `cwd`는 등록 프로젝트 안, 스트림을 닫으면 자식 kill·wait), HTTP는 streamable HTTP. OAuth 2.1은 rmcp auth(보호 자원 메타데이터 → 인가 서버 메타데이터 → 동적 클라이언트 등록 → PKCE)이며 콜백은 S5-2 `oauth_loopback.rs`(127.0.0.1, 인증 중에만)를 쓴다. 토큰은 메모리만.
- 프로젝트 컬렉션의 stdio 요청은 검토(`reviewed`)되지 않으면 화면이 연결 버튼 대신 "명령 검토 필요"와 정확한 명령을 보이고, 확인하면 `api.collection_review` 뒤 연결한다(S5-3 규칙).
- 화면 gRPC 초안(`grpcDrafts.ts`): `Map<"${target}\u0000${method}", { json, metadata, dirty }>`. 필터가 선택을 바꿔도 지우지 않는다. 필터의 자동 선택 동작은 유지한다. 호출은 `{ method, requestId, stream }`에 묶이며, 다른 메서드를 고른 뒤 도착한 응답은 "<A>의 결과"로 출처를 붙여 보이고 기록·[취소]는 A를 가리킨다.

**단계별 실행:**
- [ ] **① 실패 테스트:** `crates/api/tests/grpc.rs`의 fixture는 시험 안의 proto 텍스트(`package demo; service Echo { rpc Unary(Payload) returns (Payload); rpc Server(Payload) returns (stream Payload); }`)를 `protox`로 해석해 `register_encoded_file_descriptor_set`에 넣은 reflection 서버와, raw codec으로 `/demo.Echo/*`를 처리하는 tower 서비스를 `127.0.0.1:0`에 띄운다(v0.9.0 방식). `crates/cli/tests/api_mcp.rs`는 `env!("CARGO_BIN_EXE_devbox")`를 `mcp` 인자, 임시 HOME·`DEVBOX_INSTANCE=test-<난수>`로 실행한다. 화면 시험은 회귀 표의 AR-F07·F08.
- [ ] **② 실패 확인:** 모두 `python3 scripts/verify-resources.py --no-lock --` 아래에서 `cargo test -p devbox-api --test grpc grpc_reflection_describe_and_unary_round_trip -- --test-threads=2`, `cargo test -p devbox-cli --test api_mcp -- --test-threads=2`, `pnpm --filter app exec vitest run src/features/tools/api/grpc --maxWorkers=2` → 미구현 실패.
- [ ] **③ 구현:** gRPC·MCP 모듈, 등록·`pnpm gen`, 화면.
- [ ] **④ 통과:** 같은 명령 + `cargo test -p devbox-api --test mcp_http`, typecheck, gen-check, musl check(`tonic`·`rmcp`가 같은 rustls/ring를 쓰는지 `cargo tree -p devbox-api -i rustls`로 하나인지 확인).
- [ ] **⑤ 검토·커밋:** 둘로 나눈다. `feat(api): describe and call gRPC services` · `feat(api): connect to MCP servers over stdio and HTTP`.

**핵심 테스트:**
- `grpc_reflection_describe_and_unary_round_trip`(서비스·메서드 목록, 기본값 JSON, `{"name":"a","count":"2"}` 왕복)
- `grpc_server_streaming_messages_arrive_in_order`, `grpc_proto_files_describe_without_reflection`(reflection 없는 서버 + protox), `grpc_invalid_metadata_is_rejected_before_connecting`
- `grpc_filter_switch_keeps_the_dirty_draft`(AR-F07), `late_grpc_response_shows_its_origin`(AR-F08)
- `mcp_stdio_lists_devbox_tools`(`crates/cli/tests/api_mcp.rs`): `devbox mcp`를 stdio 대상으로 `initialize` → `tools/list`에 `devbox_projects`가 있음, 스트림 닫으면 자식 종료(PID 없음)
- `mcp_http_oauth_registers_and_uses_pkce`(`crates/api/tests/mcp_http.rs`): 소유 hyper 인가 서버(메타데이터·등록·authorize·token) + streamable HTTP MCP 서버, 시험이 `Authorize{ url }`의 콜백 URL로 code를 보내 완료 → `tools/list` 성공, 토큰 파일 생성 0
- `project_stdio_profile_requires_review`(화면): 미검토 프로젝트 파일 → 연결 호출 0, 명령 표시

---

### S5-6: `crates/webhooks` — 리스너·수신 기록·응답 규칙 (묶음 AD)

**Files:**
- Modify: `Cargo.toml`(member, `devbox-webhooks`), `crates/cli/{Cargo.toml,src/daemon/{mod,state,routes}.rs}`(줄 추가), `tools_routes.rs`, `xtask`, `messages.ts`
- Create: `crates/webhooks/Cargo.toml`, `crates/webhooks/src/{lib,error,model,rules,matcher,sequence,store,history,listener,address,service}.rs` · Test: `crates/webhooks/src/{rules,history}_tests.rs`, `crates/webhooks/tests/listener.rs`
- Create: `app/src/features/tools/webhooks/{WebhooksPanel,ListenerForm,ReceivedList,ReceivedDetail,RuleEditor}.tsx`, `webhooks/toRequest.ts` + 시험

**Interfaces → Produces:**
```rust
method!(pub WebhooksListeners = "webhooks.listeners", query, Empty => ListenerList, WebhooksError);
method!(pub WebhooksStart = "webhooks.start", mutation, StartListener => Listener, WebhooksError);   // { port: u16, lan: bool } → { id, port, lan, addresses, running }
method!(pub WebhooksStop = "webhooks.stop", mutation, ListenerId => devbox_protocol::Done, WebhooksError);
method!(pub WebhooksRules = "webhooks.rules", query, ListenerId => RuleList, WebhooksError);
method!(pub WebhooksRuleUpsert = "webhooks.rule_upsert", mutation, UpsertRule => Rule, WebhooksError);
method!(pub WebhooksRuleDelete = "webhooks.rule_delete", mutation(always), RuleRef => devbox_protocol::Done, WebhooksError);
method!(pub WebhooksHistory = "webhooks.history", query, HistoryQuery => ReceivedPage, WebhooksError);
method!(pub WebhooksHistoryGet = "webhooks.history_get", query, ReceivedId => ReceivedDetail, WebhooksError); // 헤더·본문(1MiB까지 base64)
method!(pub WebhooksFixtureSave = "webhooks.fixture_save", mutation, SaveFixture => Fixture, WebhooksError);
topic!(pub WebhooksReceived = "webhooks.received", Received);   // { listenerId, id, method, path, sizeBytes, atMs }
```
- `address.rs`: `bind_address(lan: bool) -> IpAddr`(거짓 `127.0.0.1`, 참 `0.0.0.0`)와 LAN 주소 목록(`/proc/net`·인터페이스에서 읽기만). 화면은 LAN 켬일 때 "Windows·다른 PC에서 받으려면 WSL 포트 전달이 필요합니다"와 명령 예시를 보여 주고 실행하지 않는다(방화벽·portproxy를 바꾸지 않음).
- 리스너: hyper 1 `server::conn::http1`(+`auto` 없음), 요청마다 규칙 매칭(`matcher.rs`, 메서드 대소문자 무시·정확 경로 또는 끝 `*` 접두사·우선순위·삽입 순서 무관·ID 바이트 순 동률) → 응답(상태·헤더·본문·지연, 순서 응답 `sequence.rs`: 1번째 500, 2번째 200, 마지막 단계 유지). 응답 규칙은 `Content-Length`·`Transfer-Encoding`·`Connection`을 쓸 수 없다. chunked·`Expect: 100-continue`(hyper가 본문을 읽을 때 100을 보냄)·이진 본문을 처리한다. 본문 16MiB 초과는 413과 `truncated` 기록. 헤더 64KiB 초과 431.
- 기록: 메서드·경로·쿼리·헤더(행 상한)·본문 크기는 DB(`webhooks_received`), 본문은 `<data>/webhooks/<listenerId>/<n>.bin`(0600). 리스너별 초당 요청 상한(v0.9.0 고정 창). 30일 보관.
- DB(`MIGRATIONS`): `webhooks_listeners(id, port, lan, created_ms)`, `webhooks_rules(id, listener_id, priority, method, path, response_json TEXT)`, `webhooks_received(…)`, `webhooks_fixtures(…)`. 리스너는 데몬이 다시 시작하면 **자동으로 다시 열지 않고** "중지됨"으로 보인다(LAN 노출을 사용자 행동에만 묶음).
- 재전송: 화면 `toRequest.ts`가 `ReceivedDetail`을 `RequestDraft`로 바꿔 API 편집기를 연다(hop-by-hop·`Content-Length`·`Host` 제거, 이진은 Binary 본문 임시 파일 대신 base64 → raw로). 보내기는 `api.send` 경로 그대로다. `webhooks.replay` 메서드는 만들지 않는다.

**단계별 실행:**
- [ ] **① 실패 테스트:** rules·history 이식 시험(D 표)과 `crates/webhooks/tests/listener.rs`. 출발 assertion:
```rust
let l = svc.start(StartListener { port: 0, lan: false }).await?;
assert!(l.addresses.iter().all(|a| a.starts_with("127.0.0.1:")));
assert_eq!(address::bind_address(true), IpAddr::V4(Ipv4Addr::UNSPECIFIED)); // 실제 bind는 하지 않음
assert_eq!(statuses(&client, &url, 3).await, [500, 200, 200]);              // 순서 응답
```
- [ ] **② 실패 확인:** `python3 scripts/verify-resources.py --no-lock -- cargo test -p devbox-webhooks -- --test-threads=2` → 미구현.
- [ ] **③ 구현:** 이식 후 정리(파일 저장소·가림·전달 코드는 가져오지 않음), 등록·`pnpm gen`, 화면.
- [ ] **④ 통과:** 같은 명령 + `cargo test -p devbox-cli daemon::routes::tests::every_declared_method_is_routed`, 화면 시험(`toRequest.test.ts`·`WebhooksPanel.test.tsx`), gen-check, typecheck.
- [ ] **⑤ 검토·커밋:** `feat(webhooks): listen for webhooks with response rules and history`.

**핵심 테스트:**
- 이식 webhook-core 시험 28개 + 적응 18개(D 표)
- `lan_false_binds_loopback_only`(실제 바인드 주소 확인), `lan_address_selection_is_pure`(0.0.0.0 선택은 함수 시험만)
- `sequenced_responses_follow_order`(500 → 200 → 200, 상태 초기화 시 처음부터)
- `chunked_and_100_continue_are_recorded`(chunked 확장·트레일러 결합, `Expect: 100-continue`에 100 뒤 본문, 기록 크기·`.bin` bytes 일치)
- `oversized_body_and_headers_are_rejected`(413·431, 부분 기록 없음)
- `listeners_are_not_reopened_after_restart`
- `received_request_converts_to_an_api_request`(TS `toRequest`: 이진 본문 bytes 보존, 전송 헤더 제거)
- `received_authorization_is_not_saved_in_plain_text`(TS): `Authorization: Bearer <fixture token>`이 있는 받은 요청 → `toRequest` → 프로젝트 루트에 저장 → `api.collection_write`의 text에 토큰 없음·`[REDACTED]`, 저장 전 정화 안내 표시(S5-3 `sanitizeForStorage` 경유)

---

### S5-7: 변환 도구 21종 (묶음 AE)

**Files:**
- Modify: `app/package.json`(`entities`·`qrcode-generator`, `yaml`은 S5-3에서 추가됨), `app/src/features/tools/{ToolsPage,ToolsList,ToolsCommands}.tsx`, `app/src/features/runs/LogView.tsx`(우클릭 연결 몇 줄), `app/src/features/terminal/TerminalView.tsx`
- Create(선행 이동): `app/src/features/terminal/terminalActions.ts`
- Create: `app/src/features/tools/transforms/{registry.ts,TransformsPage.tsx,ToolPicker.tsx,detect.ts,recent.ts,handoff.ts,SelectionMenu.tsx}`, `transforms/frame/{ToolInput,ToolOutput,ToolActions}.tsx`, `transforms/frame/useToolAction.ts`, `transforms/tools/*.tsx`(21종 화면), `transforms/lib/{transformers,byteCodec,radix,textEncoding,jsonYaml,jsonTypescript,lorem,markdownTable,qr,hmac,hash,md5,ids,jwt,regex,lineDiff}.ts`, `transforms/lib/regex.worker.ts` + 시험(B 표)

**Interfaces → Produces:**
- `registry.ts`: `TOOLS: { id, group, name, policy: "exportable"|"non-persistable"|"sensitive", component }[]` 21행. `id`·`group`·`name`·`policy`는 `apps/api-studio-tools.json`과 같다(시험이 v0.9.0 목록 사본과 비교).
- `detect.ts`(smartDetection 이식): 입력 → 제안 도구 id 목록(JSON·JWT·Base64·URL 인코딩·Unix 시각·UUID·hex). 결정적 순서, 1MiB 초과·자격 URL·경로는 제안하지 않음.
- `recent.ts`: 최근 사용 도구 id만 `localStorage("devbox.tools.recent")`(최대 8). 입력·출력은 저장하지 않는다(`non-persistable`·`sensitive` 정책).
- `handoff.ts`: `sendToTransforms(text, source: "clipboard"|"log"|"terminal")` → zustand에 입력을 두고 `router.navigate({ to: "/tools", search: { view: "transforms", tool: detect(text)[0] } })`. 512KiB 초과는 잘라 알림.
- 명령 팔레트: `ToolsCommands`가 `useCommands().register({ id: "tools.clipboard", label: "클립보드로 변환…", run })` — `navigator.clipboard.readText()` 실패 시 토스트.
- 선택 보내기: `SelectionMenu({ getSelection, onSend })`(기능 안의 우클릭 메뉴, Radix 없이 `role="menu"` 하나·Esc 닫기·키보드 접근). `LogView`는 목록 컨테이너 `onContextMenu`에서 `window.getSelection()?.toString()`, `TerminalView`는 `term.hasSelection()`일 때 `term.getSelection()`. 코드 편집기는 S6-1b.
- 해시: SHA-256·SHA-512는 `crypto.subtle.digest`. MD5는 유지한다(에이전트 결정 ①, PROGRESS §5): WebCrypto에 MD5가 없어 RFC 1321 시험 벡터를 통과하는 작은 TS `md5.ts`(의존성 없음)를 둔다. 07 "해시·HMAC는 WebCrypto"는 SHA·HMAC에 적용한다.
- 정규식(AR-F09): `regexMatches(pattern, flags, text, limit = 10_000) → { matches: { startUtf16, endUtf16 }[], truncated }`, `segments(text, matches) → { text, matched }[]`. 항상 `g`, 너비 0 일치는 `u`면 code point, 아니면 code unit만큼 앞으로. 실행은 `regex.worker.ts`에서 하고 1초가 넘으면 worker를 끝내고 "시간 초과"(assertionRegex.worker와 같은 방식). 화면 스레드에서 사용자 패턴을 돌리지 않는다.
- 텍스트 차이: `lineDiff.ts`(S5-3에서 만든 Myers 줄 diff)를 쓴다.

**단계별 실행(과제 안 커밋 3개):**
- [ ] **7a 터미널 선행 이동** ① `TerminalView.test.tsx`(549줄)를 이동 전 실행해 통과 수를 기록한다. ② 키 동작(copy·paste·zoom) 처리를 `terminalActions.ts`로 옮긴다(동작·시험 무변경). ③ 같은 시험 이동 후 통과·TerminalView 300줄 미만 확인. ④ 검토·커밋 `refactor(terminal): move key actions out of the terminal view`. 기능 RED는 만들지 않는다(동작 보존 이동).
- [ ] **7b 도구 lib·화면** ① B 표 시험 복사 + `regex.test.ts`(회귀 표 AR-F09 두 시험)·`registry.test.ts`(`registry_matches_v090_tool_list`)·`md5.test.ts`(RFC 1321 벡터 7개). ② `python3 scripts/verify-resources.py --no-lock -- pnpm --filter app exec vitest run src/features/tools/transforms --maxWorkers=2` RED. ③ 구현(분할 표대로 `common.tsx`를 frame으로), 컴포넌트 300줄 이하. ④ GREEN·typecheck. ⑤ 커밋 `feat(tools): port the 21 transform tools`.
- [ ] **7c 감지·보내기** ① `detect.test.ts`(smartDetection 7 + 입력별 제안 순서), `handoff.test.ts`, `SelectionMenu.test.tsx`(키보드로 열고 보내기), "선택이 있으면 보내기 메뉴" 시험은 새 파일 `features/runs/LogView.selection.test.tsx`·`features/terminal/TerminalView.selection.test.tsx`에 둔다(`TerminalView.test.tsx`가 이미 549줄), `ToolsCommands.test.tsx`(클립보드 실패 토스트). ② RED. ③ 구현. ④ GREEN + 기존 `LogView`·`TerminalView` 시험 전부. ⑤ 커밋 `feat(tools): suggest transforms and accept selections from logs and terminals`.

**핵심 테스트:**
- 이식 시험 전부(B 표 208)
- `registry_matches_v090_tool_list`(21행 id·그룹·이름·정책)
- `detect` 단위: 입력별 제안 순서(JSON 객체 → json-format 먼저, JWT → jwt, `aGVsbG8=` → byte-codec, `%E2%9C%93` → url-decode, `1700000000` → timestamp, UUID → uuid), 같은 입력 반복 시 같은 순서
- `regex_ranges_are_utf16_and_cover_the_text`, `regex_worker_stops_catastrophic_patterns`(AR-F09)
- `clipboard_command_opens_the_suggested_tool`, `log_and_terminal_selection_can_be_sent`

---

### S5-8: 개발 환경 점검·도구 설치·지원 번들 (묶음 AE)

**Files:**
- Modify: `Cargo.toml`(member `crates/tools`, `zip`), `crates/cli/{Cargo.toml,src/doctor.rs,src/main.rs,src/setup.rs}`(`crate::doctor::checks`는 `devbox_tools::doctor::checks`로 넘기는 얇은 함수 또는 `pub use`로 남겨 `routes.rs`의 기존 `system.doctor` 핸들러 줄을 고치지 않음), `crates/cli/src/daemon/{tools_routes,mod}.rs`(줄 추가), `xtask`, `messages.ts`, `app/src/features/settings/DiagnosticsSettings.tsx`(기존 파일을 넓힘), `app/src-tauri/src/lib.rs`(`generate_handler!` 줄)
- Create: `crates/tools/Cargo.toml`, `crates/tools/src/{lib,error,install,support}.rs`, `crates/tools/src/doctor/{mod,checks,probe,wsl}.rs` · Test: `crates/tools/src/doctor/checks_tests.rs`, `crates/tools/tests/support_bundle.rs`
- Create: `crates/cli/src/setup/uninstall.rs`(`devbox uninstall`) · Test: `crates/cli/tests/setup_uninstall.rs`
- Create: `app/src-tauri/src/core/install.rs`(winget 허용 목록·인자, Linux 시험), `app/src-tauri/src/platform/install.rs`(실행), `app/src-tauri/src/platform/wsl_cleanup.rs`(WSL 쪽 정리 실행). Modify: `app/src-tauri/src/platform/deploy.rs`·`core/deploy.rs`([다시 설정] 명령 `redeploy_wsl`: 기존 `platform::deploy::install`·`setup`을 그대로 부름)
- Create: `app/src/features/tools/env/{EnvCheck.tsx,installAction.ts}` + 시험

**Interfaces → Produces:**
- `system.doctor`(동결 선언 그대로) → `devbox_tools::doctor::checks(&dyn Probe) -> Vec<Check>`. `crates/cli/src/doctor.rs`는 `OsProbe`(명령 실행·파일 읽기)만 남기고 `crate::setup::env_path` 결과를 `Probe::env_path_valid()`로 넘긴다(tools가 cli에 의존하지 않음). `OsProbe{ env_file, hook_binary }` 필드와 `routes.rs`의 생성 코드는 바꾸지 않는다(새 probe 항목은 `OsProbe` 메서드로만 더함). 기존 id(`systemd`·`linger`·`tool.tmux`·`inotify`·`env`·`tool.git|claude|codex|gh|node|pnpm`·`hook.binary`)는 유지한다.
- 새 점검(01 §2.4): `wsl.idle`(Windows `.wslconfig`의 `instanceIdleTimeout`·`vmIdleTimeout`을 interop으로 읽기만), `tool.rust`(`cargo`·`rustc` 버전), `tool.docker`(CLI·데몬 응답), `auth.gh`(`gh auth status` 종료 코드만), `auth.claude`·`auth.codex`(버전과 인증 파일 존재 여부만, 내용은 읽지 않음), `hook.claude`·`hook.codex`(S1 hook 설치 확인 함수가 있으면 그것을, 없으면 설정 파일에 devbox hook 문자열이 있는지만). 모든 probe는 2초 시한·읽기 전용.
```rust
method!(pub ToolsInstallCommand = "tools.install_command", query, CheckRef => InstallPlan, ToolsError);
// InstallPlan = Wsl{ command: String, title: String, shellCommand: String } | Windows{ package: WindowsPackage } | Manual{ text: String } | None
method!(pub ToolsSupportBundle = "tools.support_bundle", mutation, Empty => SupportBundle, ToolsError); // { path, sizeBytes }
```
- 설치 표(`install.rs`): `tool.tmux|git|gh` → `sudo apt install <pkg>`, `tool.node` → 안내(Manual), `tool.pnpm` → `npm i -g pnpm@9`, `tool.rust` → rustup 안내(Manual), `tool.claude` → `npm i -g @anthropic-ai/claude-code`, `tool.codex` → `npm i -g @openai/codex`, `tool.docker` → `Windows{ DockerDesktop }`. `InstallPlan::Wsl{ title, command, shellCommand }`의 `shellCommand`는 Rust가 만든 `bash -lc '<command>; exec bash -l'`(작은따옴표 escape)이라 끝난 뒤에도 셸과 출력이 남는다.
- **설치는 명시 확인 뒤에만 실행한다.** `terminal.create{ command }`는 즉시 실행하므로(위 계약) [설치]는 먼저 `ConfirmDialog`에 정확한 명령·"새 터미널에서 바로 실행됩니다(sudo 비밀번호를 물을 수 있음)"를 보이고, [실행]을 누를 때만 `terminal.create{ title, command: shellCommand }`를 부른다. 취소하면 아무것도 호출하지 않는다. Windows면 같은 확인 뒤 Tauri 명령 `install_windows_tool{ package }`(허용 목록 enum → `winget.exe install --id <id> -e --source winget`, 새 콘솔 창). 렌더러가 임의 id·명령을 넘길 수 없다.
- 지원 번들(`support.rs`): `<data>/support/devbox-support-<시각>.zip`(0600). 내용: 데몬 로그 7일·crash 로그·doctor 결과 JSON·버전·인스턴스·설정 요약(섹션 이름과 항목 수만, 값 없음)·`manifest.json`. 모든 텍스트에서 홈 절대 경로 → `~`, vault 기본 경로(`~/notes`, S3 설정은 S6에서 연결) → `<notes>`. **비밀 가림:** 비밀 저장소가 열려 있으면 `SecretsService::list()` 중 `has_value == true`인 이름만 `resolve`해(값 없는 이름 때문에 `secrets.missing`으로 번들 전체가 실패하지 않게) 얻은 값으로 S2 방식(aho-corasick, `[REDACTED]`)을 모든 파일에 적용하고 값은 번들이 끝나면 버린다(`Zeroizing`). 잠김·사용 불가이면 값을 알 수 없으므로 **로그·crash 파일을 넣지 않고** `manifest.json`에 `logsOmitted: "secrets_locked"`, 화면에 "비밀 저장소가 잠겨 있어 로그를 넣지 않았습니다"를 보인다. `crates/tools`는 `devbox-secrets`에 의존한다. 화면은 경로와 [폴더 열기](`open_external{ kind: "explorer" }`의 WSL 경로)를 보인다.
- 앱 진단 화면은 하나다(`DiagnosticsSettings.tsx`): 데몬 상태(`system.info`) · 로그 위치 · doctor 목록(항목별 [설치] 버튼은 `tools.install_command`가 있을 때) · [다시 설정](Tauri 명령 `redeploy_wsl`: 첫 실행 배치와 같은 `platform::deploy::install`·`setup` 재사용, always 확인. 데몬이 실행 중이면 01 §4.6 재시작 규칙대로 새 바이너리 배치 뒤 unit을 재시작한다) · [셸 환경 다시 읽기](`system.refresh_env`, 지금 `AgentsSettings`에 있는 것을 여기서도 부름) · [지원 번들] · [WSL 쪽 정리](always 확인 → Tauri 명령 `cleanup_wsl` → `wsl.exe -d <distro> --exec <채널 bin> uninstall`). 컴포넌트 300줄을 넘으면 `diagnostics/{DoctorList,SupportBundle,WslCleanup}.tsx`로 나눈다.
- `devbox uninstall`: 이 채널의 user unit(`devbox@<i>`·`devbox-tmux@<i>`, accept는 `devbox-accept@…`)을 멈추고 unit 파일·채널 bin 폴더만 지운다. 데이터·설정·다른 채널은 남긴다(01 §10).
- 환경 구성 내보내기·적용(X-4)은 하지 않는다.

**단계별 실행(과제 안 커밋 2개):**
- [ ] **8a 점검 이동·확장** ① `crates/tools/src/doctor/checks_tests.rs`로 기존 `checks()` 시험 4개(`versions_are_validated`·`unavailable_manager_fails_and_responsive_manager_allows_degraded_units`·`saved_environment_diagnostic_is_independent_of_current_tool_checks`·`agent_checks_use_daemon_path_even_without_saved_environment`)를 옮기고(`which_requires_absolute_directory_and_executable_file`은 `OsProbe` 시험이라 cli에 남김) 새 항목별 `FakeProbe` 시험을 쓴다. ② `python3 scripts/verify-resources.py --no-lock -- cargo test -p devbox-tools doctor -- --test-threads=2` RED. ③ 이동(먼저 동작 보존 이동, 그다음 새 항목). `system.doctor` 핸들러는 그대로 `crate::doctor::checks`를 부른다. ④ 같은 접두사로 GREEN + 기존 CLI doctor 시험(`cargo test -p devbox-cli doctor`)·`every_declared_method_is_routed`. ⑤ 커밋 `refactor(tools): move environment checks into the tools crate` → `feat(tools): check development tools and suggest installs`(이동과 기능을 따로).
- [ ] **8b 설치·번들·정리** ① `support_bundle_redacts_secret_values_when_unlocked`·`support_bundle_omits_logs_when_secrets_are_locked`·`support_bundle_scrubs_home_and_note_paths`, `wsl_install_runs_in_a_shell_that_stays_open`, `uninstall_removes_only_this_channel_units_and_binary`(소유 XDG 임시 폴더·unit 디렉터리 fixture, unit 제어는 가짜 controller 주입으로 실제 `systemctl`을 부르지 않음), Tauri `core/install.rs` 시험(`only_allowlisted_winget_packages_deserialize`·`winget_arguments_are_fixed`)·`core/deploy.rs` `redeploy_uses_the_first_launch_deploy_plan`, 화면 `EnvCheck.test.tsx`(`install_requires_confirmation_before_creating_a_terminal`). ② `python3 scripts/verify-resources.py --no-lock -- cargo test -p devbox-tools --test support_bundle -- --test-threads=2`·`… cargo test -p devbox-cli --test setup_uninstall`·`… pnpm --filter app exec vitest run src/features/tools/env --maxWorkers=2` RED. ③ 구현. ④ 같은 명령 GREEN + CI와 같은 `python3 scripts/verify-resources.py --no-lock -- cargo test --manifest-path app/src-tauri/Cargo.toml --locked --lib core:: -- --test-threads=2`(Windows 컴파일은 S5-9 CI). ⑤ 커밋 `feat(tools): add install actions, support bundles and WSL cleanup`.

**핵심 테스트:**
- doctor 항목별 `FakeProbe` 단위(Ok·Warn·Fail·fix 문구, 2초 시한 초과는 Warn "확인하지 못함")
- `support_bundle_redacts_secret_values_when_unlocked`: 소유 test 키로 fixture 비밀을 저장하고 그 값을 **데몬 로그 줄**과 **설정 파일 값**(예: 프로젝트 설정 항목) 양쪽에 넣은 뒤 번들 → zip 안 모든 파일에서 값 검색 0, 로그에는 `[REDACTED]`, 설정 요약에는 값·비밀 이름 없음, doctor 결과·버전은 있음
- `support_bundle_omits_logs_when_secrets_are_locked`: 잠근 상태 → 로그·crash 파일 없음, `manifest.json` `logsOmitted = "secrets_locked"`
- `support_bundle_scrubs_home_and_note_paths`: `/home/<fixture>/notes/…`·홈 절대 경로 검색 0
- `uninstall_removes_only_this_channel_units_and_binary`(다른 채널 bin·데이터 폴더 bytes 그대로)
- `only_allowlisted_winget_packages_deserialize`, `wsl_install_runs_in_a_shell_that_stays_open`(`'` 포함 명령의 quoting·`exec bash -l`), `install_requires_confirmation_before_creating_a_terminal`(클릭 → 대화상자에 정확한 명령, `terminal.create` 0회 / 취소 0회 / [실행] 1회·인자 `shellCommand`)

---

### S5-9: 도구 섹션 마무리·S5 완료

현재 결과: 구현·검토·CI와 영향 설치 수용1·2 완료(b18bc740), 최종 근거 독립 감사 APPROVE·PR #645 → a55907a2 squash 머지 완료. 최종 문서는 S4에 인계하며 dirty 작업 사본/공유 cache는 보존한다. 7단계는 전체6개 PASS가 아니며 과거429의3·4 PASS, 5·6 NOT_RUN 및 원본 실패/복구 근거를 PROGRESS §5에 구분했다. 아래 체크리스트는 계획 절차이고 실제 진행 원장은 PROGRESS다.

**Files:**
- Modify: `app/src/features/tools/{ToolsPage,ToolsList}.tsx`, `app/e2e/tools.spec.ts`(새 1개), 이 문서 상태 줄, `PROGRESS.md`(S5 행·PR 표·트랙 2 위치·§5·§8)

- [ ] **1 · 섹션 마무리:** ① 실패 테스트 `ToolsPage.test.tsx`: `request_kind_shows_only_its_panel`(HTTP·GraphQL·WS·SSE·gRPC·MCP 요청마다 해당 패널만, 다른 종류 패널 없음), `narrow_width_stacks_response_below`(1099px → 세로, 1100px → 좌우), `tools_list_has_four_groups`(API 컬렉션 트리 · 웹훅 · 변환 도구 · 개발 환경 점검). ② `python3 scripts/verify-resources.py --no-lock -- pnpm --filter app exec vitest run src/features/tools/ToolsPage.test.tsx --maxWorkers=2` → 새 assertion 실패 확인. ③ 구현(요청 | 응답 2분할, 1100px 미만이면 응답이 아래). E2E `tools.spec.ts` 1개: 웹훅 리스너 시작(포트 0·LAN 끔) → 시험이 127.0.0.1로 POST → 받은 목록 → [요청으로 열기] → 변환 도구로 본문 JSON 포매터. ④ 같은 명령 GREEN·typecheck(E2E는 5단계 `check.sh`에서). ⑤ 검토·커밋 `feat(tools): finish the tools section layout`.
- [ ] **2 · 개수 확인:** E2E 총수를 센다(`cat app/e2e/*.spec.ts | grep -cE "^\s*test\("`): 현재 11 + S5 3 + 그 사이 머지된 S3 수 ≤ 40(SC7, 01 §11 L3). 넘으면 겹치는 여정을 합친다. `bash scripts/loc.sh 160000` 결과(SC8 관찰, 07 S4와 같은 관찰값)와 600/300줄 초과 0, `.github`+`scripts` 1,500줄·워크플로 2개 무변경을 기록한다. 새 의존성(`reqwest`·`tonic`·`prost`·`protox`·`rmcp`·`zip`)의 CI 영향으로 이 브랜치 첫 CI와 직전 main CI의 `Rust (Cargo workspace)`·`Rust (Windows)` 소요 시간(`gh run view --json jobs`)을 PR 본문·PROGRESS에 적는다(SC7·CONVENTIONS §9).
- [ ] **3 · main 통합(먼저 머지된 트랙이 있으면):** `git -C /home/jihoon/projects/devbox-wt/v1-s5-tools fetch origin main` 뒤 S3(또는 다른 PR)가 들어왔으면 PR 전에 통합한다. 로컬 미푸시 브랜치라 `git rebase origin/main`도 되며 main 머지는 항상 squash다. 등록 파일(`Cargo.toml` members·의존성, `routes.rs`·`mod.rs`·`state.rs`, `xtask`, `Shell.tsx` pages, `messages.ts`, `app/package.json`) 충돌은 양쪽 줄을 모두 남긴다. `Services.index`는 하나로 합친다. 생성물·lock은 손으로 합치지 않는다:
```bash
cd /home/jihoon/projects/devbox-wt/v1-s5-tools && source ~/.cargo/env && export CARGO_TARGET_DIR=$HOME/.cache/targets/devbox-s5
git config merge.lockfile.driver true   # 00 §3.3, 이미 true면 그대로
cargo metadata --format-version 1 >/dev/null
pnpm install
pnpm gen
```
  통합 뒤 직접 영향 검사(`every_declared_method_is_routed`, gen-check, messages 시험, typecheck, S5 crate 시험)를 다시 하고 통합 결과를 PROGRESS §5에 적는다. `rerere`를 활용한다.
- [ ] **4 · 브랜치 전체 검토:** 과제별 검토와 별도로 `origin/main..HEAD` 전체를 하위 에이전트가 검토한다(00 §3.4 기준 + 위 Review Focus 표 + 회귀 연결 표의 시험 존재). 결과를 PROGRESS에 남긴다. 지적은 좁은 회귀 → 수정 → 재검토.
- [ ] **5 · 최종 상세 검증:** `bash scripts/check.sh`를 한 번 실행한다(내부 대기 잠금, fmt·clippy·test·biome·vitest·tsc·gen-ts --check·금지어·E2E 포함). 다른 명령으로 감싸지 않는다. 실패는 해당 범위만 고치고 그 범위만 다시 돌린다. exit 75는 "검사 대기 초과"로 보고 후 재시도. Biome CI 뒤 완료 커밋 `chore(tools): record S5 verification`(PROGRESS 갱신).
- [ ] **6 · push·산출물:** 최종 소스 SHA를 push(`git push -u origin v1/s5-tools`)하고 `gh workflow run windows.yml --ref v1/s5-tools`로 그 SHA의 산출물을 만든다. PR은 아직 열지 않는다.
- [ ] **7 · 에이전트 설치본 검증(6개):** `python3 scripts/verify-resources.py --wait --lock windows-acceptance -- <수용 driver>` 안에서 다운로드 확인→설치→검증→제거·정리를 한 번에 한다. 설치 파일은 그 안에서 `bash scripts/dogfood.sh v1/s5-tools --channel accept`로 받고 저장소·workflow·run·source SHA·channel·SHA256을 확인한다. WSL에서 PowerShell 절대 경로로 설치·실행·제거한다. `accept` 채널만 쓰고 prod 설치본·`devbox@prod`는 바꾸지 않으며 설치 전후 읽기 전용 해시·unit 상태 비교로 불변을 기록한다. 소유 표식 없는 `accept` 자원이 이미 있으면 제거하지 않고 묻는다. UAC·재부팅·사용자 클릭이 필요하면 멈춰 보고한다. v0.9.0(`com.devbox.*`, DevboxSuite)·기존 데이터는 보존하고 이번 스모크가 만든 설치 경로·데이터·WSL unit·소켓만 정리한다. 근거는 앱·데몬 로그·user systemd unit 상태·진단 정보·필요시 스크린샷. UI 자동화가 안 되는 항목만 미실행. PASS·FAIL·미실행을 PROGRESS와 PR 본문에 구분하고 자동 시험 성공을 Windows 실기 PASS로 바꾸지 않는다.
  1. API 요청·환경·비밀 사용 — 소유 fixture 비밀(스모크가 만들고 지움, 사용자 비밀 아님)을 WSL `127.0.0.1`의 소유 서버로 보내 수신값·기록의 참조 유지 확인. 네트워크가 되면 같은 fixture 비밀로 공개 echo 엔드포인트에도 보내 응답 가림(`[REDACTED]`)을 확인하고, 네트워크가 안 되면 그 부분만 미실행·이유 기록(에이전트 결정 ②).
  2. Postman 컬렉션 가져오기 — 소유 fixture 파일, accept 채널 개인 컬렉션(`devbox-accept/api`)에만 쓰이고 prod 설정 bytes·mtime 불변.
  3. 웹훅 수신·재전송 — Windows 쪽 소유 `curl.exe`로 `http://127.0.0.1:<port>`(WSL localhost 전달)에 송신 → 수신 기록 → [요청으로 열기] → 소유 로컬 대상으로 재전송. 전달이 안 되면 FAIL·근거를 기록하고 **`lan: true`로 바꾸거나 portproxy·방화벽을 바꾸지 않는다.** 외부 서비스에서 받기는 "사용자 결정 대기"(00 §3.2).
  4. JWT·Base64·JSON 도구 각 1회.
  5. 명령 팔레트 "클립보드로 변환…" — driver가 시작 전 현재 텍스트 클립보드를 프로세스 메모리에만 보관(로그·artifact에 쓰지 않음)하고 소유 fixture 텍스트를 넣어 확인한 뒤 원래 내용을 복원한다. 텍스트가 아니거나 복원에 실패하면 "사용자 클립보드를 덮어씀"을 PROGRESS·PR에 기록한다.
  6. 개발 환경 점검에서 빠진 도구 [설치] — [설치] 버튼 표시와 `tools.install_command` 결과(명령·winget id)가 설치 표와 같은지만 확인하고 **버튼·[실행]을 누르지 않는다**(터미널 생성이 곧 실행이므로). 실제 설치는 사용자 승인 뒤에만(사용자 결정 ④, 00 §3.2). 승인 전에는 "사용자 결정 대기"로 기록한다.
- [ ] **8 · PR 한 번:** PROGRESS를 "구현 완료·PR 수용 대기"로 갱신한 결과 기록은 검증한 SHA를 바꾸지 않도록 00 §3 규칙(결과 기록은 다음 작업 브랜치의 문서 커밋으로 인계)을 따른다. `gh pr create --base main --head v1/s5-tools`로 PR을 한 번 연다. 본문에 범위·과제별 시험·제외한 이식 시험과 이유·설치본 6개 결과·E2E 수·loc·사용자 결정 항목을 적는다.
- [ ] **9 · CI·리뷰:** 필수 `Frontend (pnpm)`·`Rust (Cargo workspace)`·`Rust (Windows)` 성공을 확인한다. 자동 코드·보안 리뷰는 최대 3회(서로 다른 SHA 3개)까지 반영하고, 회차의 수정은 모아서 한 번에 push한다. 4회째부터는 실제 데이터 손실·비밀 유출·승인하지 않은 실행만 고치고 나머지는 PROGRESS §8에 적는다(CONVENTIONS §8). SHA가 바뀌면 필요한 설치본 항목을 새 산출물로 다시 확인한다.
- [ ] **10 · 머지·정리:** `gh pr merge --squash`. 실제 머지 확인 뒤 이 문서 상태 줄, PROGRESS의 S5 행·PR 표·트랙 2 위치를 갱신한다. S4 완료도 확인한 뒤 S6로 간다. 정리는 clean·merged 확인 → worktree 제거 → `git worktree prune` → 로컬·원격 브랜치 삭제 순서이고 **트랙 2 전용 `~/.cache/targets/devbox-s5`만 지운다**(실행 중인 cargo·rustc가 없을 때). 공유 cache·사용자 자료·다른 worktree는 보존한다.
