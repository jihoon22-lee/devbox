# 앱 간 연동 — 인바운드 계약과 생태계 확장 설계

- 상태: v0.4.1 범위(§1의 Path/Workspace 라우팅·§3·§5.1) 구현 및 안정판 배포 완료;
  v0.5.0 catalog·Profile/Query·snapshot 정리와 protocol v2 handoff **범위 확정, 개발 착수**
- 2026-08-27: Webhook Lab → API Playground `api-request/v1` producer/receiver draft 구현;
  preview·claim/ack/restore·privacy/no-clipboard 경계는 #315에서 검증
- 2026-08-28: `#320` Devbox Launcher bounded catalog/snapshot consumer와 `Task`/`Install`
  routing 구현. 기존 Life Log→Knowledge 구조화 handoff 계약은 유지
- 2026-08-28: `#366/#367` Run Manager·WSL Desktop producer와 Log Lens bounded
  claim/preview lifecycle 보강. Run log를 실제로 읽는 Log Lens receiver adapter는 별도
  후속 작업이며, 기존 ancestor TOCTOU와 local-adapter FIFO/UNC reader 위험은 잔여 범위다.
- 작성일: 2026-08-17
- 범위: 저장소 전체 — `crates/applink`, `crates/launch`, `crates/integration`, 신규
  `crates/catalog`, `apps/catalog.json`, 기존 13개 앱 + 구현된 Devbox Launcher·계획된 Log Lens
- 관련: [UX 개선 설계](./2026-08-15-ux-improvements-design.md) §4.2, [wsl-desktop 터미널 설계](./2026-08-17-wsl-desktop-terminal-design.md) §4.4
- 근거: `docs/product-opportunities.md` §10.1(versioned read-only snapshot), §12.4(앱 간 연결)

> **2026-08-22 확장.** 이 문서의 v0.5.0 제안 범위는
> [v0.5.0 네이티브 우선 계획](./2026-08-22-v0.5.0-native-first-plan.md)에서 확정됐다.
> `Path`·`Profile`·`Workspace`·`Query`처럼 작은 argv만으로 표현할 수 없는 API request,
> Knowledge draft, log source 전달을 위해 protocol v2의 one-time `Handoff`를 추가한다.
> devbox가 양쪽 앱을 제어할 때 사용자가 파일·클립보드로 데이터를 운반하지 않게 하는 것이
> 목적이다.

## 0. 배경 — 링크가 작성돼 있으나 작동하지 않는다

repo-manager와 workbench는 다른 앱을 실행하며 인자를 넘긴다.

```text
// v0.4.1 release-gate target mapping
repo-manager -> code-pad:    OpenTarget::Workspace { path }
repo-manager -> wsl-desktop: OpenTarget::Path { path }
repo-manager -> workbench:   OpenTarget::Path { path }

workbench -> wsl-desktop:    OpenTarget::Path { path } // concrete WSL path, then Windows path
workbench -> code-pad:       OpenTarget::Workspace { path } // Windows workspace path
```

`OpenTarget::Profile`과 그에 따른 터미널 레이아웃 선택은 v0.5.0 범위다(§4.4). v0.4.1은
프로필 id를 wsl-desktop에 보내지 않고, 실제로 열 수 있는 구체적인 `Path`만 보낸다.
v0.4.1의 `Path` 타깃에는 distro 정보가 없으므로 WSL Desktop은 앱에서 선택된 distro를 사용하고,
선택값이 없으면 기본 distro를 사용한다. 특정 프로필의 distro와 레이아웃을 복원하는 동작은 v0.5.0으로 미룬다.

위 매핑 이전의 v0.4.0 구현에는 다음 문제가 있었다. **argv를 읽는 앱이 하나도 없었다.** 저장소 전체에서 `std::env::args` 소비처는
run-manager의 `--background` 하나뿐이다(`apps/run-manager/src-tauri/src/lib.rs:108`).
code-pad·wsl-desktop·workbench의 `main.rs`/`lib.rs`에는 인자 처리가 **전혀 없다.**

> v0.4.0 당시 repo-manager에서 "CodePad" 버튼을 누르면 **빈 Code Pad가 열렸다.**
> Workbench의 "Start Workspace"는 wsl-desktop과 code-pad를 띄웠지만
> **둘 다 아무 프로젝트도 모르는 상태로** 떴다.

### 0.1 v0.4.0 기준 현재 연동 실태 (역사적 기준선)

다음 표와 설명은 v0.4.1 수정 전의 v0.4.0 상태를 기록한 역사적 관찰이며, 현재 구현 상태를
뜻하지 않는다. 현재 릴리스 범위는 §5.1, 이후 기능은 §5.2의 v0.5.0 범위를 따른다.

```
repo-manager   ──launch(path)─────────► code-pad       ✗ 인자 무시
repo-manager   ──launch(path)─────────► wsl-desktop    ✗ 인자 무시
repo-manager   ──launch(path)─────────► workbench      ✗ 인자 무시
workbench      ──launch(--profile)────► wsl-desktop    ✗ 인자 무시
workbench      ──launch(--workspace)──► code-pad       ✗ 인자 무시
workbench      ──snapshot v1──────────► run-manager    ✓
life-log       ──snapshot v1──────────► run-manager    ✓
workbench      ──data.db 직접 읽기────► life-log       ⚠ 자체 정책 위반
knowledge-base ──write snapshot v1────► (소비자 없음)   ⚠ 고아 producer
```

- **완전 고립 5개 앱**: api-playground, developer-toolbox, everything-plus, port-manager, webhook-lab
- workbench가 life-log의 SQLite를 직접 연다(`workspace.rs:424-457`) —
  `docs/architecture.md:62-66`이 금지한 바로 그 행위이고, `docs/architecture.md:50`의
  "외부 DB 직접 조회 없음"이라는 서술과도 어긋난다
- knowledge-base는 아무도 읽지 않는 스냅샷을 쓴다
- life-log는 `crates/integration`을 쓰지 않고 `core/readers.rs`에 같은 계약을 중복 구현했다

### 0.2 빠진 것은 "인바운드 계약"이다

메커니즘 재고를 정리하면:

| 있는 것 | 크레이트 | 방향 |
|---|---|---|
| 앱을 실행한다 | `crates/launch` | 아웃바운드 |
| 내 상태를 남이 읽게 한다 | `crates/integration` | 아웃바운드 |
| **이미 떠 있는 앱에게 "이 상태로 가라"고 말한다** | **없다** | **인바운드** |

이 문서는 그 인바운드 계약을 정의한다.

### 0.3 설계 기준 — 생태계 확장

**v0.5.0의 14·15번째 예정 앱과 그 이후 새 앱을 추가할 때 기존 13개 앱을 한 줄도 고치지
않아도 연결되어야 한다.**

이 기준이 아래 세 결정을 낳는다:

1. argv 계약은 **타입이 있고 전방 호환**이어야 한다 (§1)
2. "어떤 앱이 무엇을 받는가"는 **카탈로그가 선언**하고 UI가 그것에서 생성돼야 한다 (§2)
3. 스냅샷 소비자는 producer id를 하드코딩하지 말고 **발견**해야 한다 (§4)

---

## 1. `crates/applink` — 인바운드 계약

### 1.1 왜 새 크레이트인가

`crates/launch`에 넣지 않는다. 수신 앱 13개가 "설치 경로를 해석하는 코드"에 의존하게 되기
때문이다. 계층을 나눈다:

```
crates/applink   계약만 (타입 + parse + build)   ← 수신 앱 13개가 의존
crates/launch    설치 해석 + 실행                ← applink 에 의존, 발신 앱만 의존
```

### 1.2 타입

```rust
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OpenTarget {
    Path { path: String, line: Option<u32>, column: Option<u32> },
    Profile { id: String },
    Workspace { path: String },
    Query { text: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OpenRequest {
    pub target: OpenTarget,
    pub from: Option<String>,   // 보낸 앱의 카탈로그 id — 로깅·되돌아가기용
}

pub fn parse_argv(args: &[String]) -> Result<Option<OpenRequest>, ParseError>;
pub fn build_argv(req: &OpenRequest) -> Vec<String>;
```

플래그:

| 플래그 | 타깃 |
|---|---|
| `--path <p>` `[--line N]` `[--column N]` | `Path` |
| `--profile <id>` | `Profile` |
| `--workspace <p>` | `Workspace` |
| `--query <text>` | `Query` |
| `--from <app-id>` | 공통 |
| (맨 앞 위치 인자) | `Path` — 하위 호환 |

### 1.3 전방 호환 규칙 — 생태계 확장의 핵심

새 타깃 종류를 추가했을 때 **이미 설치된 구버전 앱**이 그것을 받는 상황이 반드시 생긴다.
(devbox-manager는 앱을 개별적으로 업데이트하므로 버전이 섞인다.)

| 상황 | 처리 |
|---|---|
| 모르는 플래그 | **무시한다.** 오류가 아니다 |
| 알려진 타깃 플래그가 하나도 없음 | `Ok(None)` → 앱은 평소대로 뜬다 |
| 알려진 플래그인데 값이 없거나 형식 오류 | `Err(ParseError)` → 로그만 남기고 평소대로 뜬다 |
| 맨 앞 위치 인자 | `Path`로 해석 |

즉 **새 발신자가 구버전 수신자에게 `--query`를 보내면 오늘과 동일한 동작(빈 앱)으로
degrade한다.** 크래시하거나 오류 대화상자를 띄우지 않는다. 새 기능이 구버전을 깨뜨리지
않는다는 보장이 있어야 카탈로그에 앱을 자유롭게 추가할 수 있다.

위치 인자를 `Path`로 받는 이유: 이미 배포된 repo-manager 빌드가 그 형태로 보낸다
(`commands.rs:151`). 사용자가 repo-manager를 업데이트하지 않아도 링크가 살아난다.

### 1.4 각 앱의 수신 라우팅

| 앱 | 받는 타깃 | 동작 |
|---|---|---|
| code-pad | `Path`(+line/column), `Workspace` (v0.4.1) | 파일 열기 / 워크스페이스 열기 |
| wsl-desktop | `Path` (v0.4.1), `Profile` (v0.5.0) | 그 경로에서 터미널 / 프로필 레이아웃 (터미널 설계 §4.4) |
| workbench | `Path` (v0.4.1), `Profile` (v0.5.0) | 경로로 기존 프로필 선택·없으면 생성 초안 / opaque profile id로 정확히 선택 |
| knowledge-base | `Path`, `Query`, `Handoff(knowledge-draft/v1)` | 노트 열기 / 검색 / 저장 전 draft preview |
| everything-plus | `Query` | 검색어로 시작 |
| repo-manager | `Path` | 해당 저장소 선택 |
| api-playground | `Handoff(api-request/v1)` | 전달된 templated request를 저장 전 preview |
| developer-toolbox | 없음 | receiver integration 이후 text와 추천 transformer를 입력 초안으로 적용 |
| run-manager | `Task` (v0.5.0) | 저장된 job/service id를 재검증하고 확인 후 실행 |
| devbox-manager | `Install` (v0.5.0, hidden) | embedded catalog app id를 재검증하고 설치 화면 표시 |
| webhook-lab | `Handoff(webhook-fixture/v1)` | response fixture 초안 preview |
| log-lens | `Handoff(log-source/v1)` | local/WSL source 연결과 Run identity handoff preview (Run receiver follow-up) |

이 표는 §2의 `accepts` 선언과 1:1 대응한다. 선언하지 않은 target은 수신 앱이 명시적인
no-op/error로 처리하며 다른 기본 동작으로 fall through하지 않는다.

### 1.5 v0.5.0 protocol v2 — one-time handoff

argv에 JSON body나 장문 text를 직접 넣지 않는다. command line 노출, 길이 상한, quoting,
secret persistence 문제를 피하기 위해 `OpenTarget`에는 opaque id만 전달한다.

```rust
pub const PROTOCOL_VERSION: u32 = 2;

pub enum OpenTarget {
    // v1 variants 유지
    Path { path: String, line: Option<u32>, column: Option<u32> },
    Profile { id: String },
    Workspace { path: String },
    Query { text: String },
    Task { id: String },
    Install { app_id: String },
    Handoff { kind: String, id: String },
}
```

`Task`와 `Install`은 장문 payload를 운반하지 않는 bounded routing target이다. 수신 앱이 현재
저장된 task 또는 embedded catalog를 다시 확인하며 one-time handoff envelope을 바꾸지 않으므로
protocol version은 2를 유지한다.

handoff envelope:

```text
protocolVersion, id, kind, sourceApp, targetApp?, createdAt, expiresAt, payload
```

- 128-bit random id, 기본 TTL 10분, serialized payload 10MiB 상한.
- create-new + atomic rename으로 완성되지 않은 payload를 노출하지 않는다.
- `targetApp`이 있으면 그 앱만 consume할 수 있다.
- kind/schema/source/target/expiry/size를 검증한 뒤 claim하고 성공하면 한 번만 ack해 삭제한다.
- 처리 실패는 claim을 restore해 TTL까지 재시도할 수 있게 남기되 손상·만료 payload는 정리한다.
- 큰 binary는 payload에 복제하지 않고 canonical path, size, digest만 전달한다.
- secret 원문은 금지한다. `api-request/v1`에는 `${SECRET_NAME}` 참조만 허용한다.

read-then-delete로는 동시 consumer가 같은 payload를 처리할 수 있으므로 상태를 고정한다.

```text
pending --atomic claim(consumer+lease)--> claimed --ack--> consumed/deleted
   ^                                      |
   └----------- restore/nack --------------┘
```

- producer는 pending payload를 create-new + atomic rename으로 발행한다. claim은 exclusive
  rename/claim record와 consumer token/lease를 사용해 한 consumer만 소유하게 한다.
- 처리 성공 뒤에만 token을 확인한 `ack`가 삭제하고, validation/import 실패는 `restore`가
  pending으로 되돌린다. consumer crash는 lease expiry 후 재시도한다.
- claim/ack/restore는 id, target, kind, schema와 token을 재검증하며 concurrent claim,
  duplicate ack, wrong target, crash recovery를 계약 테스트한다.
- 기본 lease는 60초이며 payload `expiresAt`을 넘지 않는다. 같은 token의 상한 내 lease 갱신만
  허용하고 만료 token의 ack/restore는 거부한다.

표준 kind는 `api-request/v1`, `webhook-fixture/v1`, `knowledge-draft/v1`, `log-source/v1`,
`toolbox-text/v1`이다. 새 kind는 source/target/payload schema와 redaction 규칙을 설계 문서에
먼저 추가한다.

**2026-08-28 `log-source/v1` producer/claim-preview contract (#366/#367).** Run Manager는 기존
`{ kind, sourceId, runId, stream }` reference를 그대로 payload로 사용한다. `sourceId`는
`run-manager:<run-id>:<stdout|stderr>`와 exact 일치하며 run 저장소의 상대 log directory,
명령, cwd, 환경변수, credential, 원문은 포함하지 않는다. WSL Desktop은 arbitrary command를
전달하지 않고 다음 두 payload만 허용한다:

```json
{ "sourceType": "wslFile", "distro": "Ubuntu", "wslPath": "/var/log/app.log" }
{ "sourceType": "wslJournal", "distro": "Ubuntu", "unit": "sshd.service" }
```

`wslPath`는 host `path`와 구별되는 bounded absolute WSL path이며 `..`, root, 제어 문자와
argv injection 문자를 거부한다. 두 producer는 catalog가 선언한 설치된 Log Lens를 확인한 뒤
공용 one-time store에 10분 TTL envelope을 만들고 AppLink에는 kind/id만 전달한다. Log Lens는
cold/hot request를 자동으로 source에 추가하지 않고 claim→summary preview를 먼저 표시한다.
사용자가 명시적으로 `읽기 전용 source 추가`를 누를 때만 ack 후 지원되는 fixed adapter로 넘기고,
취소·검증 실패·lease expiry는 restore한다. missing/expired/lease-expired claim 오류는 stale modal을
정리하고, storage/restore 실패는 claim이 있는 경우 exact claim(그 외에는 exact request ID)을
유지한 채 최대 세 번의 bounded recovery 시도를 제공한다.
native 오류는 고정 코드만 frontend로 건너가며 raw path/payload/storage detail은 노출하지 않는다.
payload와 argv에는 secret/raw credential/로그 원문을 넣지 않으며, WSL path는 이 일회성 TTL
envelope과 process-local adapter 설정 밖에 저장하지 않는다. Run payload는 identity-only이므로
Run log를 읽는 app-owned receiver adapter는 별도 후속 작업이다. clipboard·shell·network
ingest·permanent archive 경로를 제공하지 않는다.

producer가 envelope을 만든 뒤 Log Lens launch가 실패하면 방금 만든 descriptor와 immutable
envelope을 다시 대조해 exact pending 파일만 제거한다. 따라서 launch 실패가 사용 불가능한
pending handoff를 남기지 않으며, cleanup/launch 오류는 payload나 경로가 없는 고정 코드로
반환한다.

이 범위에는 shared store의 same-user ancestor replacement TOCTOU를 없애는
`openat`/directory-handle 전환이나 기존 local-adapter FIFO/UNC reader 위험을 해결하는 작업은
포함하지 않는다. 둘 다 별도 후속 보안/reader 작업으로 남긴다.

Producer publish와 launch는 각 producer 프로세스 안에서 single-flight로 묶는다. 이미 처리 중인
요청은 고정 `handoff-busy` 오류로 종료하고 새 envelope을 만들지 않는다. Receiver는 claim 시
protocol version, opaque id/token, timestamp와 lease 범위, target/source-family parity를 다시
검증하며, frontend도 native preview/source 응답의 허용 key·identity·경로·unit을 재검증한다.
기존 preview/action 중 새 요청이 들어오면 최신 opaque id 하나만 bounded queue에 보존하고,
복구 중인 claim이 있으면 queue를 drain하지 않는다. 오래된 React 응답은 generation/unmount
guard로 폐기한다. `wslJournal`의 선택적 `unit`은 native JSON의
`null` 또는 누락을 동일하게 “unit 없음”으로 해석한다. modal은 명시적 add/cancel, Escape/Tab
focus trap과 opener 복원을 제공한다.

Adapter cancellation은 Windows Job Object kill-on-close 또는 Unix process-group 종료와 bounded
reap을 사용하며, helper 종료 실패 시 direct child fallback을 유지한다. shell/network/clipboard
fallback이나 raw log/path/credential 전달은 없다.

`knowledge-draft/v1`의 Life Log→Knowledge 구현은 이 generic lifecycle 위에 aggregate-only
앱 계약을 둔다. Life Log는 검증된 native digest에서 period/range/timezone, bounded summary,
결정론적 Markdown body, 고정 tags와 네 source provenance만 publish하고, session/window title/
Git project path/note path/credential은 payload와 argv에서 제외한다. Knowledge는 source·target·
kind·schema와 body 재현성/size/privacy bounds를 다시 검증해 process-local claim slot에서
preview를 제공한다. `Save draft` 확정 뒤에만 exclusive Journal note create→SQLite index→ack/
delete 순서를 따르며, cancel·pre-commit 실패는 restore한다. envelope TTL은 10분, preview lease는
최대 60초(30초 cadence)이고 만료·손상·launch race는 고정 오류와 새 digest 재생성으로 격리한다.
이 구현은 persistent pending/sent/consumed/expired 상태 저장이 아니라 P3 상태 보강의 기반이다.

Run Manager의 #311 local validation은 `log-source/v1` reference를
`{ kind, sourceId, runId, stream }`으로 한정한다. `sourceId`는
`run-manager:<opaque-run-id>:<stdout|stderr>`와 exact 일치해야 하며 absolute path,
command, environment, credential, remote address를 payload에 넣지 않는다. 이 reference는
unknown field도 거부하므로 추가 path field를 숨길 수 없다. 현재 검색 결과의 source
identity를 검증하기 위한 local boundary이며, grouped #366/#367은 이 DTO를 변경하지 않고
위의 one-time claim/ack handoff 경계로 확장한다.

---

## 2. 카탈로그 capability — 메뉴를 선언에서 생성한다

### 2.1 문제

repo-manager는 대상 앱을 **하드코딩**한다:

```rust
// apps/repo-manager/src-tauri/src/commands.rs:148
if !matches!(app_id.as_str(), "code-pad" | "wsl-desktop" | "workbench") {
    return Err("알 수 없는 앱".into());
}
```

그리고 UI의 버튼 세 개도 하드코딩이다(`apps/repo-manager/src/App.tsx:105-107`).
14·15번째 예정 앱과 그 이후 앱이 "경로를 받을 수 있다"고 해도, repo-manager를 고치지
않으면 나타나지 않는다.
같은 문제가 "다른 앱으로 열기"를 넣고 싶은 모든 앱에서 반복된다.

### 2.2 선언

`apps/catalog.json`의 `schemaVersion`을 2로 올리고 단조 증가하는 `catalogRevision`과 각
항목의 capability를 추가한다:

```json
{
  "schemaVersion": 2,
  "catalogRevision": 1,
  "apps": [
    {
      "id": "code-pad",
      "productName": "Code Pad",
      "identifier": "com.devbox.codepad",
      "accepts": ["path", "workspace"],
      "produces": ["snapshot:code-pad/workspace/v1"]
    }
  ]
}
```

이것이 만드는 것:

- repo-manager의 하드코딩 allowlist가 사라진다 — `accepts`에 `path`가 있는지 보면 된다
- **"다른 앱으로 열기" 메뉴가 카탈로그에서 생성된다.** 어떤 앱이든 "나는 경로를 가지고
  있다"만 선언하면, `accepts`에 `path`가 있고 **실제로 설치된** 앱들이 메뉴에 자동으로
  나타난다
- **새 앱은 `catalog.json` 항목 하나로 capability가 맞는 기존 앱 메뉴에 등장한다**
- 구조화 payload는 `handoff:<kind>/v<n>`, snapshot은
  `snapshot:<producer>/<kind>/v<n>` 형식으로 선언한다
- 정적 launcher action은 항목의 `actions`에 `actionId`, `actionVersion`, label, target과
  versioned payload kind만 선언한다. profile/job/query의 동적 상태와 secret은 catalog에
  저장하지 않고 snapshot에서 발행한다.
- schema v1 또는 capability가 없는 항목은 빈 배열로 읽어 하위 호환한다

### 2.3 런타임 카탈로그 배포

현재 `catalog.json`은 devbox-manager만 **빌드 타임에** `include_str!`로 갖는다
(`manager.rs:19`, `doctor.rs:11`). 게다가 `apps/devbox-manager/src/api.ts:6-18`이 13개 항목
전체를 TS로 손수 중복해 두고 있다. 다른 앱은 접근 경로가 아예 없다.

설계:

1. devbox-manager가 설치/업데이트마다 `<common_root>/catalog.json`
   (`%LOCALAPPDATA%\devbox\catalog.json`)에 **런타임 사본을 원자적으로 쓴다.**
   `crates/integration::write_atomic`의 tmp+rename 패턴을 재사용한다
2. 새 순수 `crates/catalog`가 schema v1/v2 type, `catalogRevision`, runtime/build-time
   freshness 선택, capability filter를 제공한다:
   ```rust
   pub struct AppRef { pub id: String, pub display_name: String, pub accepts: Vec<String> }
   pub fn capable_targets(kind: &str) -> Vec<AppRef>;
   ```
   유효한 runtime 사본의 revision이 build-time 이상이면 runtime을 사용하고, 더 오래됐거나
   손상된 사본은 build-time으로 폴백한다. Manager는 현재 revision보다 낮은 사본을 쓰지 않는다.
3. `crates/launch::installed_targets(kind)`가 capable target에
   `resolve_installed`를 적용해 실제 설치된 것만 남긴다. `crates/applink`는 argv 계약만
   담당한다. `launch`가 이미 `applink`에 의존하므로 이 분리가 순환 의존을 막는다
4. catalog와 custom root의 선행 계약으로
   `%LOCALAPPDATA%\devbox\install-roots\v1\registry.json` versioned locator를 둔다.
   `schemaVersion`, 단조 증가하는 `registryRevision`, 기록 당시 `catalogRevision`, `rootId`,
   canonical root path와 app-owned manifest 경로를 기록하고 tmp+rename으로 갱신한다. catalog
   revision 변경만으로 root를 무효화하지 않는다. `crates/launch`가 이를 해석하며 v0.4.x 고정 위치는 migration
   기간에만 read-only fallback으로 사용한다. custom root UI는 이 locator를 소비한 뒤 붙인다.
5. **폴백**: 런타임 사본이 없으면(단독 설치, Manager 미실행) 각 앱이 빌드 타임
   `include_str!` 사본을 바닥값으로 쓴다. `catalogRevision`이 freshness 조건을 통과한 runtime
   사본만 build-time보다 우선한다
6. `api.ts:6-18`의 손수 중복은 제거하고 카탈로그에서 파생시킨다

폴백이 있어야 하는 이유: 앱은 devbox-manager 없이 단독 설치될 수 있다. 그 경우 자기 자신은
알지만 다른 앱의 설치 여부는 모른다 — `resolve_installed`가 `None`을 반환하므로 메뉴가
비어 있을 뿐, 오류는 아니다.

### 2.4 UI 생성

컨텍스트 메뉴(UX 개선 설계 §1)의 "다른 앱으로 열기" 섹션만 이것으로 생성한다.
**나머지 항목은 100% 각 앱 소유다.**

```
┌─────────────────────────┐
│ 경로 복사               │  ← 앱 고유
│ 탐색기에서 열기         │  ← 앱 고유
├─────────────────────────┤
│ 다른 앱으로 열기     ▸  │  ← 카탈로그에서 생성 (공통)
│   Code Pad              │
│   WSL Desktop           │
│   Workbench             │
├─────────────────────────┤
│ 삭제                    │  ← 앱 고유 (danger)
└─────────────────────────┘
```

---

## 3. 이미 떠 있는 인스턴스 — single-instance 포워딩

앱이 이미 실행 중일 때 다시 실행하면 두 번째 창이 뜬다. 저장소에 선례가 있다 —
run-manager(`lib.rs:52-63`)와 life-log(`lib.rs:39-41`)가 `tauri-plugin-single-instance`를
쓴다. 다만 둘 다 인자를 크로스앱 라우팅에 쓰지 않는다.

```rust
.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
    match devbox_applink::parse_argv(&args) {
        Ok(Some(req)) => {
            app.state::<PendingOpen>().set(req.clone());
            let _ = app.emit("devbox://open", req);
        }
        Ok(None) => {}                       // 타깃 없음 — 창만 띄운다
        Err(e) => eprintln!("applink: {e}"),  // 형식 오류 — 무시하고 창만 띄운다
    }
    // 창 복원 + 포커스 (최소화돼 있을 수 있다)
}))
```

**콜드 스타트도 같은 코드 경로를 쓴다.** `setup`에서 `std::env::args()`를 파싱해
`PendingOpen` 관리 상태에 넣고, 프론트가 마운트 시 `take_pending_open` 커맨드로 가져간다.
이벤트를 emit하면 프론트가 아직 리스너를 걸지 않아 놓칠 수 있으므로 **pull 방식**으로
경합을 없앤다. (wsl-desktop의 `attach_session`이 같은 문제를 같은 방식으로 푼다 —
터미널 설계 §2.6.)

| 상황 | 동작 |
|---|---|
| 앱이 꺼져 있음 | 콜드 스타트 → `PendingOpen` → 프론트가 마운트 시 pull |
| 앱이 떠 있음 | single-instance 핸들러 → 이벤트 + `PendingOpen` → 창 포커스 |
| 앱이 떠 있고 타깃이 유효하지 않음 (경로 없음 등) | 창은 포커스하고, 프론트가 사용자에게 사유 표시. 조용히 무시하지 않는다 |
| 앱이 떠 있고 타깃 없음 | 창만 포커스 |

---

## 4. 스냅샷 버스 — 정리와 자동 발견

### 4.1 정리

| 작업 | 위치 | 효과 |
|---|---|---|
| life-log가 `projects` 스냅샷을 발행, workbench가 consumer로 | `workspace.rs:424-457`의 직접 SQLite 읽기 삭제 | `docs/architecture.md:62-66` 정책 위반 해소 |
| life-log가 knowledge-base 스냅샷의 consumer가 됨 | `product-opportunities.md §11.1`에 이미 "Knowledge의 note 작성·수정 수"로 계획돼 있음 | 고아 producer 해소 |
| life-log의 중복 구현 삭제 | `apps/life-log/src-tauri/src/core/readers.rs` 제거 → `crates/integration` 사용 | 계약 단일화 |
| wsl-desktop을 producer로 | Docker 컨테이너 + 열린 터미널 발행 | **UX 개선 설계 §4.3의 "workbench가 WSL Desktop의 Docker/포트를 자동 반영"은 이것 없이는 구현 불가.** wsl-desktop은 현재 `write_snapshot`을 전혀 호출하지 않는다 |

wsl-desktop producer 구현 시 `core/parsers.rs`의 interactive Docker detail parser와 snapshot
parser를 섞지 않는다. 기존 `ContainerInfo.ports`는 #276 detail UI가 보여 줄 원시 문자열이고,
`runtime/v1` producer는 별도의 fixed four-field query(`ID`, `Names`, `State`, `Ports`)와
`core::runtime_snapshot` parser로 validated `portMappings`만 발행한다. 이 분리는 원문 detail을
runtime snapshot에 실수로 저장하지 않게 하며, Workbench consumer가 주소가 다른 IPv4/IPv6
binding을 deterministic tuple로 dedupe할 수 있게 한다.

일회성 `knowledge-draft/v1` 전달은 snapshot bus와 별개로 공용 handoff store를 사용한다.
producer가 공용 root의 pending envelope를 atomic publish하고 `launch_open`에는 `Handoff`의
kind/id만 넘긴다. Knowledge가 claim한 payload를 preview한 뒤 사용자가 저장할 때만 파일과
index를 변경하고 ack하며, validation/file/index 오류는 restore해 다른 producer와 저장소의
원문 경계를 침범하지 않는다.

**2026-08-26 #410 구현 상태.** WSL Desktop은 `wsl.exe --list --running --quiet`로 이미
실행 중인 distro만 순차 열거하고, 각 distro에서 `wsl.exe -d <validated-distro> -- docker ps
-a --no-trunc --format {{.ID}}\\t{{.Names}}\\t{{.State}}\\t{{.Ports}}`를 고정 실행한다.
stopped distro는 시작하지 않으며 shell/user command/mutation은 없다. bounded parser는 distro
64개·container 256개/distro·512개 전체·terminal 256개/distro·mapping 32개/container·
1,024개 전체·stdout 4MiB·line 16KiB·5초 timeout을 적용하고, malformed/partial/timeout은
빈 snapshot으로 덮어쓰지 않고 last-good을 보존한다. `dockerAvailability`는 성공/빈 출력,
exit 127, 기타 non-zero를 각각 `available`/`missing`/`error`로 구분한다. envelope은
`Envelope::with_views` + `write_atomic`으로 producer당 한 파일만 교체하며 catalog capability는
`snapshot:wsl-desktop/runtime/v1`로 선언한다. Workbench #281 consumer와 Docker/WSL action은
이 producer 구현에 포함되지 않는다.

### 4.2 자동 발견

지금은 consumer가 producer id를 하드코딩한다. 새 producer가 생겨도 아무도 모른다.

```rust
// crates/integration
pub struct SnapshotRef {
    pub producer: String,
    pub version: u32,
    pub generated_at: String,
    pub path: PathBuf,
}

/// <common_root>/integration/*/v*/summary.json 을 스캔한다.
pub fn discover() -> Vec<SnapshotRef>;
```

life-log의 "Data sources" 패널(`apps/life-log/src/App.tsx:233-235`)이 하드코딩 목록 대신
발견된 모든 producer를 표시한다 — **새 앱이 producer가 되면 자동으로 등장한다.**
기존 `SourceStatus`(available / schemaVersion / producerVersion / generatedAt / freshnessMs /
error)는 그대로 쓰고, 목록만 발견 결과로 채운다.

Devbox Launcher가 소비할 수 있는 동적 source path는 다음과 같다. 이는 consumer-side 지원
계약이며 `#320`이 모든 producer를 구현하거나 등록한다는 뜻이 아니다. path가 아직 없으면
Launcher는 해당 source를 `missing`으로 격리하고 나머지 검색을 계속한다.

| snapshot path | 예상 producer | 내용 |
|---|---|---|
| `workbench/profiles/v1` | Workbench (후속 producer) | recent profile/workspace id와 표시 metadata |
| `repo-manager/repositories/v1` | Repo Manager (후속 producer) | repository/worktree id와 path label |
| `run-manager/jobs-services/v1` (`status/v1` 호환) | Run Manager | job/service id, 상태와 실행 action metadata |
| `everything-plus/saved-queries/v1` | Everything+ (후속 producer) | query/filter만, 결과 목록은 제외 |
| `wsl-desktop/profiles/v1` | WSL Desktop (후속 profile producer) | profile/layout/distro/cwd metadata |

각 envelope은 `schemaVersion`, `producer`, `producerVersion`, `generatedAt`과 `data.views`를
포함하고 `%LOCALAPPDATA%\devbox\integration\<app-id>\v1\summary.json`에 atomic replace로
쓴다. 각 view는 자체 `schemaVersion`, `freshnessMs`, `entries`를 가지며 entry에는 versioned
action payload와 안정적인 id만 둔다. secret, environment value, raw log, full query result는
금지한다. stale·손상 source 하나가 다른 source 검색을 막지 않는다. 기존 Life Log→Knowledge
`knowledge-draft/v1`은 구조화 catalog action으로 유지하지만 Launcher가 clipboard text로
변환하지 않는다. Developer Toolbox `toolbox-text/v1` static action은 실제 claim/ack receiver가
준비된 뒤 선언한다.

기존 snapshot path에는 producer/version별 파일이 하나뿐이다. 여러 kind를 발행하는 WSL
Desktop과 Run Manager는 `data.views` 아래 `runtime`/`profiles`, `status`/`jobs-services` view를
한 envelope에 모아 한 번만 atomic replace한다. kind별 writer가 같은 `summary.json`을 서로
덮어쓰지 않으며 catalog capability가 지원 view와 version을 선택한다.

`read_snapshot`은 이미 파일 없음을 `Ok(None)`으로, producer/schema 불일치를 `Err`로
처리한다(`crates/integration/src/lib.rs:76-97`). 발견 기반으로 바뀌어도 이 계약은 그대로다.

---

## 5. 릴리스 분리

### 5.1 v0.4.1 (핫픽스) — 끊긴 링크만 되살린다

포함:
- `crates/applink` 신설 (§1.2, §1.3)
- **이미 인자를 받고 있는 3개 앱만** 수신 구현 — code-pad, wsl-desktop, workbench
- 그 3개 앱에 `tauri-plugin-single-instance` + `PendingOpen` (§3)
- `crates/launch`가 `build_argv`를 쓰도록 변경

v0.4.1 발신 타깃은 다음과 같이 고정한다.

- repo-manager → Code Pad: `OpenTarget::Workspace { path }`
- repo-manager → WSL Desktop/Workbench: `OpenTarget::Path { path }`
- Workbench → WSL Desktop: `OpenTarget::Path { path }` (구체적인 WSL 경로 우선, Windows 경로 폴백)
- Workbench → Code Pad: `OpenTarget::Workspace { path }` (비어 있지 않은 Windows 경로만)

제외:
- 카탈로그 capability 스키마 (§2)
- 런타임 카탈로그 배포 (§2.3)
- 스냅샷 정리·자동 발견 (§4)
- 나머지 10개 앱의 수신

**컷 라인 근거**: `--path`·`--workspace`는 v0.4.1에서 실제로 보낼 수 있는 타깃이다.
Workbench → WSL Desktop은 프로필 id가 아니라 구체적인 `Path`를 보낸다. 수신을 붙이는 것은 새 기능이 아니라
**출시된 기능의 완성**이다. 반면 카탈로그 capability와
스냅샷 정리는 새 표면이므로 핫픽스에 넣지 않는다.

`--profile`을 통한 프로필 선택과 터미널 레이아웃 동작은 v0.5.0 §4.4로 명시적으로 미룬다.

repo-manager의 하드코딩 allowlist(`commands.rs:148`)는 §2의 카탈로그로 대체되므로
핫픽스에서는 손대지 않는다.

### 5.2 v0.5.0

`crates/catalog` + `catalogRevision`/versioned install-root locator → §4 snapshot producer와
자동 발견 → 나머지 앱 수신 → §1.5 handoff core(claim/ack/restore) → P2 Webhook Lab → API
Playground → P2 Life Log → Knowledge → P3 Devbox Launcher/Log Lens bootstrap → P3 Toolbox
→ API → P3 Run/WSL → Log Lens producer 순서다. 2026-08-27 현재 P2 Webhook Lab → API
Playground 단계는 #315 draft로 구현되었고, catalog capability·shared store·cold/hot
single-instance·preview/apply/cancel·fixed error/no-clipboard 경계를 포함한다. 카탈로그와 locator가 있어야 컨텍스트
메뉴의 "다른 앱으로 열기"와 custom-root executable discovery가 생성되고, handoff kind를
수신할 설치 앱도 안전하게 찾을 수 있다.

다음 수동 검증은 v0.5.0 범위이며 v0.4.1 릴리스 게이트가 아니다.

- 카탈로그에 가짜 16번째 앱 항목을 추가했을 때 다른 앱의 "열기" 메뉴에 자동 등장하고,
  설치돼 있지 않으면 보이지 않는지 확인한다.
- workbench가 life-log의 SQLite를 직접 열지 않고 스냅샷 계약을 사용하는지 확인한다
  (`grep -rn "lifelog" apps/workbench`).

---

## 6. 테스트 계획

| 대상 | 방법 |
|---|---|
| `parse_argv` | 알려진 플래그 전부. **모르는 플래그 무시**(전방 호환 회귀 방지 — 명시적으로 단언). 타깃 없음 → `Ok(None)`. 값 누락 → `Err`. 맨 앞 위치 인자 → `Path` |
| `build_argv` ↔ `parse_argv` | 왕복 테스트. 모든 `OpenTarget` 변형 |
| 카탈로그 파싱 | `accepts`가 없는 구버전 항목 → 빈 목록으로 폴백(에러 아님) |
| `installed_targets` | 설치되지 않은 앱은 제외되는지. `catalogRevision` freshness를 통과한 runtime만 우선하고, 없거나 stale/corrupt면 build-time 폴백. versioned install-root locator와 custom root manifest 경계 |
| `discover()` | 스냅샷 0개/1개/N개. 손상된 JSON은 건너뛰고 나머지를 반환하는지 |
| single-instance | 콜드 스타트와 두 번째 인스턴스가 **같은 `PendingOpen` 경로**를 쓰는지 |
| handoff | create/claim/ack/restore, 10분 expiry, wrong target, 10MiB 상한, 손상 JSON, 중복·동시 consume, consumer crash lease recovery |
| secret 경계 | handoff/snapshot에 secret 원문이 없고 API payload에는 이름 참조만 있는지 |

**실기 검증** (Windows):
- repo-manager에서 "CodePad" → Code Pad가 **그 저장소를 열고** 뜬다
- Code Pad를 띄워둔 상태에서 다시 "CodePad" → 새 인스턴스가 아니라 **기존 창이 포커스되며
  같은 workspace/저장소가 열린다**
- Workbench "Start Workspace" → wsl-desktop이 프로필의 **구체적인 경로**에서 뜬다

---

## 7. 구현 순서

| # | 작업 | 릴리스 |
|---|---|---|
| 1 | `crates/applink` 타입 + `parse_argv`/`build_argv` + 테스트 | v0.4.1 |
| 2 | code-pad·wsl-desktop·workbench 수신 + single-instance | v0.4.1 |
| 3 | `crates/launch`가 `build_argv` 사용 | v0.4.1 |
| 4 | `crates/catalog` + catalog v2 `accepts`/`produces`/`actions` + `catalogRevision` + v1 fallback | v0.5.0 |
| 5 | 런타임 카탈로그 freshness 배포 + versioned install-root locator(`registryRevision` 별도) + `crates/launch::installed_targets` + `api.ts` 중복 제거 | v0.5.0 |
| 6 | repo-manager allowlist 제거 → 카탈로그 기반 | v0.5.0 |
| 7 | life-log `projects` producer → workbench 직접 DB 읽기 삭제 | v0.5.0 |
| 8 | life-log가 knowledge-base consumer + `core/readers.rs` 삭제 | v0.5.0 |
| 9 | wsl-desktop producer (Docker/터미널) + 포트 구조화 | v0.5.0 |
| 10 | `discover()` + life-log Data sources 패널 | v0.5.0 |
| 11 | 나머지 앱 수신 라우팅 (§1.4) | v0.5.0 |
| 12 | protocol v2 `Handoff` + atomic claim/ack/restore store + 계약 테스트 | v0.5.0 |
| 13 | Webhook Lab → API Playground `api-request/v1` | v0.5.0 |
| 14 | Life Log → Knowledge `knowledge-draft/v1` | v0.5.0 |
| 15 | Devbox Launcher catalog/snapshot action consumer bootstrap | v0.5.0 |
| 16 | Log Lens `log-source/v1` claim/preview boundary; app-owned Run receiver adapter remains a follow-up | v0.5.0 |
| 17 | Developer Toolbox → API Playground `api-request/v1` (P3 integration) | v0.5.0 |
| 18 | Run Manager·WSL Desktop → Log Lens `log-source/v1` producer integration (Run receiver reading is a separate follow-up) | v0.5.0 |

1과 2는 한 PR로 묶는다 — 계약과 첫 소비자를 분리하면 검증이 안 된다.
