# UX 개선 설계 — 컨텍스트 메뉴·도구 확장·앱별 항목

- 상태: v0.5.0 P1·P2 범위 확정, 개발 착수
- 작성일: 2026-08-15 (개정: 2026-08-17, 2026-08-22)
- 근거: `docs/product-opportunities.md` §11.1~11.3(기능 순서), §12·§13·§14(기존 앱 확장)
- 선행: PR 1~39 + Stage 4/5 (모두 완료), 13개 앱 + 공용 crates/packages

> **개정 안내 (2026-08-17).** 초판은 wsl-desktop 클립보드(§2)와 앱 간 연동(§4.2)을 이 문서 안의
> 표 몇 줄로 다뤘다. 코드를 읽어 확인한 결과 둘 다 **표 몇 줄로 처리할 수 없는 결함**이
> 아래에 깔려 있었고, 두 주제를 별도 문서로 분리했다. 이 문서는 컨텍스트 메뉴와 앱별
> 기능 확장만 다룬다.
>
> - [wsl-desktop 터미널 설계](./2026-08-17-wsl-desktop-terminal-design.md) — 초판 §2를 대체
> - [앱 간 연동 설계](./2026-08-17-app-interop-design.md) — 초판 §4.2의 전제
>
> 초판의 사실 오류 두 건도 정정했다 (§4.1 wikilink, §5 WebSocket). 아래 각 항목에 표시.

> **2026-08-22 범위 개정.** 외부 도구에 유사 기능이 있거나 새 dependency가 필요하다는
> 사실만으로 native 기능을 제외하지 않는다. P1·P2 core는 오프라인 제공, 대형 전문 도구는
> 선택적 보완재로 두는 정책으로 바뀌었다. §5의 “제외 확정”을 재검토 표로 교체했으며,
> 우선순위·구현 상한·신규 Devbox Launcher/Log Lens 범위는
> [v0.5.0 네이티브 우선 계획](./2026-08-22-v0.5.0-native-first-plan.md)을 따른다.

## 0. 배경

13개 앱이 기능적으로 완성된 뒤 남은 불편은 대부분 **상호작용(UX) 격차**다.
전 앱에 `onContextMenu` 핸들러가 **0개**(저장소 전체 grep으로 확인)라 우클릭은 웹뷰 기본
반응(또는 무반응)에 그친다.

원칙:
- 각 앱의 책임을 다른 앱에서 복제하지 않는다.
- 파괴적 동작(삭제·종료·제거)은 `danger` 스타일 + 확인을 기본으로 한다.
- 공용 코드는 실제 소비자가 확정된 것만 `packages/`·`crates/`로 추출한다.

---

## 1. 우클릭 컨텍스트 메뉴

### 1.0 공용은 껍데기뿐, 항목은 100% 앱 소유

**13개 앱에 일괄 적용하되, 메뉴 내용은 앱마다 다르다.** 같은 앱 안에서도 대상마다 다르다.
무엇을 공용화하고 무엇을 하지 않는지를 먼저 못박는다.

| 소유자 | 책임 |
|---|---|
| `packages/context-menu` | 렌더링, 뷰포트 경계 포지셔닝, 바깥 클릭/Esc/스크롤 닫기, 키보드 이동, `role="menu"` 접근성, `danger` 스타일. **항목을 하나도 정의하지 않는다** |
| 각 앱 | `MenuItem[]`을 만들어 넘긴다. 대상 종류(포트 행 / 파일 노드 / 에디터 탭 / 캘린더 날짜)마다 다른 목록 |
| 카탈로그·실행 (`crates/catalog` + `crates/launch`)과 `crates/applink` | capability·설치 상태로 "다른 앱으로 열기" 섹션 **하나만** 만들고 versioned argv로 라우팅 |

즉 공용 컴포넌트의 API는 `<ContextMenu items={...} />` 하나이고, `items`는 각 앱이 자기
도메인 지식으로 채운다. §1.2의 표가 그 목록의 명세다.

차이가 큰 예:

- **port-manager** 포트 행 — 포트 복사 · PID 복사 · `localhost:port` 열기 · **Kill**(danger)
- **life-log** 캘린더 날짜 — 날짜 복사 · export **두 개뿐** (읽기 전용 집계 앱이므로 최소화)
- **code-pad** — 탭에서는 "닫기/다른 탭 닫기/이름 변경", 에디터 본문에서는 **전혀 다른
  메뉴**(잘라내기/복사/붙여넣기/정의로 이동)
- **wsl-desktop** 터미널 팬 — 복사 · 붙여넣기 · 검색 · 분할 · 팬 닫기

### 1.1 공통 구현 방향

네이티브 메뉴(`tauri-plugin-menu`)는 13개 앱 전부에 붙이기 무겁고 플러그인 의존이
늘어나므로 **HTML 기반 공용 컴포넌트**를 만든다.

```ts
type MenuItem =
  | { type: "item"; label: string; action: () => void; disabled?: boolean; danger?: boolean }
  | { type: "submenu"; label: string; items: MenuItem[] }
  | { type: "separator" };
```

- **`packages/context-menu`를 처음부터 공용으로 만든다.** 저장소 규칙("두 번째 실제
  소비자가 생길 때 추출")과 충돌하지 않는다 — **같은 배치에 13개 소비자가 확정돼 있으므로
  규칙의 전제가 이미 충족된다.** 규칙의 취지는 "쓰이지도 않을 추상을 미리 만들지 말라"이지
  "소비자가 13개여도 두 번 기다려라"가 아니다.
- 위치: 뷰포트 기준 포지셔닝(화면 밖으로 나가면 반대편으로 뒤집기), 바깥 클릭/Esc/스크롤로 닫기.

### 1.1.1 선결 과제 — 클립보드 채널

§1.2 항목의 대부분이 "복사"다. 이는 13개 앱 전부의 `capabilities/default.json`에 영향을
주므로 **메뉴 구현보다 먼저 결정한다.** 현재 전 앱의 capability는
`["core:default", "opener:default"]`뿐이고(run-manager만 `notification:default`),
클립보드 플러그인은 어디에도 없다.

- **쓰기**(`navigator.clipboard.writeText`)는 이미 5곳에서 동작한다 — api-playground 2,
  developer-toolbox 2(`tools/common.tsx:83-90`의 `CopyBtn`), everything-plus 1.
  **그대로 쓴다. 플러그인 불필요.**
- **읽기**(붙여넣기)가 필요한 앱만 `tauri-plugin-clipboard-manager`를 도입한다.
  WebView2가 `readText()`에 사용자 활성화/권한을 요구하기 때문이다. 현재 그 대상은
  wsl-desktop과 텍스트 입력이 있는 앱들이다.

### 1.1.2 접근성과 IME

- `role="menu"` / `role="menuitem"`, focus trap, 닫힐 때 **원래 요소로 포커스 복귀**
- 키보드: ↑↓ 이동, Enter 실행, Esc 닫기, **Shift+F10 / Menu 키로 열기**
- 선택되지 않은 행에서 우클릭하면 **선택을 그 행으로 동기화**한 뒤 메뉴를 연다
- **IME 보호**: 텍스트 입력·에디터 영역에서 기본 웹뷰 메뉴를 대체하면 한국어 입력기 관련
  기본 동작을 잃는다. 입력 영역은 기본 메뉴를 유지하거나, 최소한 잘라내기/복사/붙여넣기/
  실행취소를 직접 제공한다
- **CodeMirror 영역**(code-pad, knowledge-base)은 `onContextMenu`가 CM6 자체 처리와 겹치므로
  `EditorView.domEventHandlers` 경유로 붙인다

### 1.2 앱별 메뉴 항목 — 앱 → 대상 → 항목

**대상마다 메뉴가 다르다.** 한 앱에 여러 행이 있는 것은 그래서다.

| 앱 | 대상 | 항목 |
|---|---|---|
| port-manager | 포트/프로세스 행 | 포트 복사 · PID 복사 · `localhost:port` URL 복사 · localhost 열기(LISTEN) · 프로세스 경로 복사 · 탐색기에서 열기 · **Kill**(danger) |
| developer-toolbox | 입력 텍스트 영역 | 붙여넣기 · 모두 선택 · 비우기 |
| developer-toolbox | 출력 텍스트 영역 | 복사 · 모두 선택 · 결과 파일로 저장 |
| everything-plus | 검색 결과 행 | 열기 · 폴더에서 보기(Explorer /select) · 경로 복사 · 파일명 복사 · **다른 앱으로 열기 ▸** |
| knowledge-base | 파일 트리 노드 | 새 파일 · 새 폴더 · 이름 변경 · 삭제(danger) · 경로 복사 · 탐색기에서 열기 · **다른 앱으로 열기 ▸** |
| knowledge-base | 에디터 본문 | 잘라내기 · 복사 · 붙여넣기 · 링크 삽입 (CM6 경유) |
| code-pad | 에디터 탭 | 닫기 · 다른 탭 닫기 · 오른쪽 탭 모두 닫기 · 경로 복사 · 탐색기에서 열기 · 이름 변경/삭제 |
| code-pad | 에디터 본문 | 잘라내기 · 복사 · 붙여넣기 · 정의로 이동 · 참조 찾기 (CM6 경유) |
| code-pad | 파일 트리 (§4.3) | 새 파일 · 이름 변경 · 삭제(danger) · 경로 복사 |
| run-manager | 작업 행 | 지금 실행 · 활성화/비활성화 · 편집 · 로그 열기 · 삭제(danger) |
| run-manager | 서비스 행 | 시작 · 정지 · 재시작 · 편집 · 삭제(danger) |
| run-manager | 이력 행 | 로그 보기 · 재실행 · 로그 저장 |
| devbox-manager | 앱 목록 행 | 설치/업데이트 · 실행 · 이전 버전 롤백 · 설치 폴더 열기 · 제거(danger) |
| workbench | 프로젝트 프로필 | Start Workspace · Stop What I Started · 프로필 편집 · 삭제(danger) · 경로 복사 · **다른 앱으로 열기 ▸** |
| webhook-lab | 수신 요청 history | **마스킹 복사**(기본) · **원본 복사**(확인 후) · 헤더 복사 · API Playground로 변환 · 삭제(danger) |
| webhook-lab | 규칙 행 | 편집 · 복제 · PowerShell curl.exe 복사 · POSIX sh curl 복사 · 삭제(danger) |
| repo-manager | 저장소 행 | **다른 앱으로 열기 ▸** · worktree 생성 · 경로 복사 · 탐색기에서 열기 |
| api-playground | History/Collection 항목 | 복제 · 이름 변경 · 삭제(danger) · curl 복사 |
| **wsl-desktop** | **터미널 팬** | **복사(선택 시만 활성) · 붙여넣기 · 검색 · 세로 분할 · 가로 분할 · cwd 복사 · 팬 닫기(danger)** |
| **wsl-desktop** | **탭** | **닫기 · 다른 탭 닫기 · 이름 변경 · 레이아웃 전환** |
| life-log | 캘린더 날짜 | 날짜 복사 · 해당 날짜 Markdown/JSON export |

> - life-log는 읽기 전용 집계 앱이므로 메뉴를 최소화한다.
> - **wsl-desktop 행은 초판에 누락돼 있었다.** 그 탓에 초판 §2-3("우클릭 붙여넣기")과
>   §1(우클릭 메뉴)의 충돌이 표면화되지 않았다. **결정: 우클릭 = 메뉴, 가운데 클릭 =
>   붙여넣기.** 상세는 [터미널 설계](./2026-08-17-wsl-desktop-terminal-design.md) §3.3.
> - **"다른 앱으로 열기 ▸"는 각 앱이 하드코딩하지 않는다.** `apps/catalog.json`의
>   `accepts` 선언에서 생성되므로, 새 앱을 추가하면 기존 앱을 고치지 않아도 메뉴에
>   나타난다. [앱 간 연동 설계](./2026-08-17-app-interop-design.md) §2.

> **2026-08-26 구현 상태 (#261).** Life Log의 toolbar 선택 날짜와 주·월 daily chart
> 날짜에 메뉴를 연결했다. exact chart date를 먼저 선택한 뒤 `YYYY-MM-DD`를
> 복사하며 pointer/keyboard close 후 focus를 복원한다. Markdown/JSON export는 #305의
> date-range/source metadata/privacy/native save 경계를 반쪽만 구현하지 않도록 그 전까지
> disabled다. 이로써 기존 13개 앱의 P1-06 앱별 적용이 완료됐다.

### 1.3 완료 조건

- 13개 앱 전부에서 우클릭이 앱 고유 메뉴를 연다. 같은 앱의 서로 다른 대상은 서로 다른
  메뉴를 연다.
- 파괴적 항목은 전부 `danger` 스타일이며 확인을 거친다.
- 키보드만으로 메뉴를 열고(Shift+F10) 항목을 실행하고 닫을 수 있으며, 닫으면 포커스가
  원래 요소로 돌아온다.
- 텍스트 입력 영역에서 한국어 IME 입력이 메뉴 도입 전과 동일하게 동작한다.
- "다른 앱으로 열기"에 **설치된** 앱만 나타난다. 카탈로그에 앱을 추가하면 기존 앱의
  코드 변경 없이 목록에 반영된다.

---

## 2. wsl-desktop 터미널

**별도 문서로 분리했다** → [wsl-desktop 터미널 설계](./2026-08-17-wsl-desktop-terminal-design.md)

초판은 이 절에서 복사/붙여넣기를 표 6줄로 다뤘다. 코드를 읽어보니 그 아래에 **터미널
출력을 손상시키는 결함**이 있었다 — `terminal.rs:113-118`이 PTY 읽기마다
`String::from_utf8_lossy`를 호출해, 읽기 경계에 걸친 한글·박스드로잉 문자가 U+FFFD로
치환되고 이후 컬럼 계산이 어긋난다. "중간 중간 화면이 깨진다"는 실사용 보고의 직접
원인이다. 여기에 `windowsPty` 미설정, 팬이 1×2로 찌그러져 셸 화면을 파괴하는 문제,
영구 resize desync가 겹친다.

분리한 문서가 다루는 것:
- §2 결함 수정 (v0.4.1 핫픽스)
- §3 클립보드·단축키 — **드래그 시 자동 복사를 기본 켬으로 하고**, 그 결과 초판 §2-2의
  `hasSelection()` 분기가 불필요해진다 (선택 시점에 이미 복사됐으므로 Ctrl+C는 항상 SIGINT)
- §4 레이아웃 복원 + 멀티플렉서 opt-in + 사용성 기본기 + 워크스페이스/명령 팔레트

---

## 3. developer-toolbox 도구 확장

현재 14종(JSON/Encoding/Time/Text/Security/Regex/JWT). **오프라인·저비용·고빈도**
기준으로 추가를 제안한다.

| 도구 | 그룹 | 구현 | 가치 |
|---|---|---|---|
| JSON ↔ YAML 변환 | JSON | TS(`js-yaml`) | 높음 — 설정 파일 작업 |
| Base64/Base64URL/Hex + 진법 변환 | Encoding | TS | 높음 — 디버깅 기본기 |
| JSON → TypeScript 타입 생성 | JSON | TS | 높음 — API 응답 → 인터페이스 |
| UUID v4/v7 다량 생성 + ULID | Security | Rust(기존 `uuid` 확장) | 중간 |
| HTML Entity Encode/Decode | Encoding | TS | 중간 |
| URL Component Encode/Decode | Encoding | TS | 중간 |
| HMAC / JWT 서명 검증(HS256) | Auth | Rust(`hmac`) | 중간 — 기존 JWT 확장 |
| Lorem Ipsum/placeholder 생성 | Text | TS | 중간 |
| Markdown 테이블 생성기 | Text | TS | 중간 |
| QR 생성 | Encoding | Rust(`qrcode`) | 중간 — URL/Wi-Fi/text를 오프라인 export |

추가 도구는 기존 `ToolDef[]` 레지스트리에 한 줄 추가만 하면 좌측 메뉴에 자동 등록되는
구조를 그대로 이용한다 (`apps/developer-toolbox/src/tools/index.tsx`).

**범위 제한 (초판 누락):**
- **JSON → TS 타입 생성**은 단일 sample을 기준으로 object/array/null과 동일 object 배열의
  optional field를 추론한다. 서로 다른 scalar type의 복잡한 union, schema merge, codegen
  plugin은 제외한다. root type 이름과 identifier 정규화 규칙은 deterministic해야 한다.
- **`js-yaml` 의존 추가**는 §5에서 QR 생성을 "의존 추가 대비 가치 낮음"으로 제외한 것과
  형평이 맞아야 한다. 근거: YAML은 이 저장소의 설정 파일 작업에서 고빈도이고,
  `js-yaml`은 브라우저 번들 친화적이며 추가 런타임 의존이 없다.
- **HMAC를 Rust로 두는 것은 일관성이 있다** — `hash`/`generateUuid`가 이미 Rust 커맨드다
  (`tools/security.tsx:2`, `src-tauri/src/commands/tools.rs`).
- **QR은 제외에서 복원한다.** permissive pure-Rust `qrcode`를 설치물에 정적으로 포함할 수
  있고, 별도 웹 서비스·실행 파일·network 없이 SVG/PNG를 만들 수 있어 devbox 목적과 맞는다.

### 3.1 완료 조건

- 추가 도구가 좌측 메뉴에 자동 등록되고, 기존 도구의 동작에 영향이 없다.
- 변환 도구는 잘못된 입력에 대해 예외를 던지지 않고 오류 메시지를 표시한다.
- 각 변환기에 왕복 테스트가 있다 (`tools/transformers.test.ts` 확장).

---

## 4. 기타 앱별 추천

### 4.1 기존 계획에서 확정된 항목 (세부화)

| 앱 | 항목 | 범위 | 난이도 |
|---|---|---|---|
| knowledge-base | 백링크 + 역링크 패널 | #272에서 parser·dynamic key index·autocomplete·unresolved·source 위치 이동, #273에서 fixed diff 승인·link-aware rename rollback transaction 구현 | **상** |
| knowledge-base | 퀵캡처(전역 단축키)+Inbox | global shortcut → Inbox note | 중 |
| knowledge-base | 첨부파일(이미지) 관리 | root 내 attachment 폴더, 이미지 드롭·삽입·프리뷰 | 중 |
| api-playground | 파일 업로드 | `multipart/form-data`, `reqwest::multipart`; #270에서 runtime-only picker 경로·bounded stream·safe history로 구현 | 저 |
| api-playground | 응답 헤더/쿠키 뷰어 | #271 Body/Headers/Cookies 탭, Set-Cookie 기본 masking, bounded native raw-copy 확인 경계 | 저 |
| api-playground | OpenAPI 3 import | 스펙 파싱 → endpoint 요청 초안. **기존 collection 덮어쓰기 방지를 위해 적용 전 preview 필요** — `packages/diff-view`가 이미 있으므로 소비자로 연결 | 중 |
| life-log | Markdown/JSON export | 집계 결과 직렬화 | 저 |
| port-manager | 프로세스 명령줄 표시+복사 | Win32 process command line 조회. **타 사용자/권한 프로세스는 접근 거부되므로 "권한 없음" 표시 경로를 함께 설계** | 중 |

> **정정 (초판 사실 오류).** 초판은 백링크 항목에 "`[[wikilink]]` 파싱(`core/markdown` 기반
> 존재)"이라 적었다. **사실이 아니다.**
> - `core/markdown` 경로는 저장소에 없다 (`crates/markdown`)
> - `crates/markdown/src/lib.rs`(377줄)의 공개 API는 `render()` **하나뿐**
> - `wikilink`는 `apps/`·`crates/`·`packages/` 전체에 **0건**
>
> 파서를 처음부터 만들어야 하고, 링크 파싱 + unresolved 판정 + 역인덱스 + rename 시 링크
> 깨짐 preview까지가 한 덩어리다. 난이도를 **중 → 상**으로 올린다.

### 4.2 신규 추가 항목

| 앱 | 항목 | 근거 |
|---|---|---|
| everything-plus | 검색 결과 → 다른 앱으로 열기 | `product-opportunities.md` P1의 크로스앱 연동. **`crates/launch:77`과 argv 계약이 전제** — 아래 참조 |
| code-pad | 탭 우클릭 + "다른 탭 닫기/탐색기 열기" | 파일 다중 편집기 기본 UX (§1.2와 연계) |
| run-manager | 로그 뷰어 검색/필터 | 회전 로그 tail에 검색 부재 시 장기 로그 사용 불가 |
| devbox-manager | 설치 폴더 열기·제거 | Manager 관리 완결성 (§1.2와 연계) |

> **전제 조건 (v0.4.0 → v0.4.1 갱신).** "다른 앱으로 열기"는 작은 항목이 아니라
> **저장소 차원의 프리미티브에 막혀 있었다.** v0.4.0에서는 repo-manager와 workbench가
> 다른 앱에 인자를 넘겨도 **argv를 읽는 앱이 없어** 빈 앱이 열렸다. v0.4.1에서
> [앱 간 연동 설계](./2026-08-17-app-interop-design.md)의 `crates/applink`와
> Code Pad/WSL Desktop/Workbench의 single-instance pending-open 수신 및 대상 매핑 복구를
> 완료했으므로, 이제 후속 v0.5.0 링크 항목을 구현할 기반이 준비됐다.
>
> 반대로 그 프리미티브 하나가 8개 링크를 동시에 연다 — repo-manager→3개(이미 작성됨),
> workbench→2개(이미 작성됨), everything-plus→code-pad, knowledge-base→code-pad,
> run-manager job cwd→code-pad.

### 4.3 실사용 피드백 (v0.4.0-rc1)

Windows 실기 검증에서 수집한 UX 개선 항목. 기능 버그가 아니라 편의·가독성 항목이다.
**추측 기반 항목보다 우선한다** (§7 참조).

| 앱 | 항목 | 설명 | 비고 |
|---|---|---|---|
| devbox-manager | 일괄 설치/업데이트 + 다중 선택 | #274에서 catalog checkbox, manifest 1회 조회, 순차 실행, 앱별 결과와 실패 exact-mode retry 구현 | 성공 앱 유지·실패 앱만 재시도. portable current/registry rollback, setup 다중 마법사 확인 |
| devbox-manager | 설치 위치 표시 + 지정 | #275에서 locator/manifest가 검증한 executable/root/source manifest 읽기 전용 표시 구현. 경로 지정은 후속 | portable actual path만 표시하고 installer 위치는 미추정. lifecycle DTO는 path-free, 변경은 별도 P2 |
| wsl-desktop | Docker 패널 컴팩트 포맷 | #276에서 name/state/port 우선 disclosure list 구현. 펼치면 ID/image/status/ports 원문과 기존 action 표시 | 260px fixture, 긴 이름 ellipsis·port 2개+나머지 수 축약. 기본 table 추측 대신 Docker format 5필드 사용 |
| code-pad | 언어 서버 패널 높이 확보 | 언어 서버 목록 패널이 좁아 가독성이 떨어짐 | `App.css:622-627`의 `min-height:120px`. **같은 모달 안에 스크롤 영역이 둘**(`:685-690` installer)이라 그 충돌을 먼저 해소해야 한다 |
| code-pad | 빠른 열기 → 트리 + 탭/패널 | #277에서 기존 bounded workspace snapshot의 fuzzy 결과를 재귀 directory group으로 표시하고 긴 파일명·부모 경로를 줄바꿈 가능하게 분리 | 표시 tree 순서와 keyboard selection을 일치시키고 `Ctrl/⌘+P`, `↑/↓`, `Home/End`, `Enter`, `Esc`, modal focus 복원을 fixture로 고정. 별도 filesystem walk·Git grep·LSP 없음 |
| ~~code-pad~~ | ~~상태 표시줄 하단 고정~~ | ~~파일 길이에 따라 움직이지 않고 항상 최하단 고정~~ | #190에서 해결됨 (`.content-area` 높이 제약) |
| code-pad | 프리뷰/편집 영역 구분 강화 | 프리뷰 영역이 편집 영역과 시각적으로 구분되게 | **저비용.** `.preview-pane`(`App.css:227-235`)과 `.view-pane`(`:270-274`)이 같은 배경색(`#171e29`)에 1px 선 하나로만 나뉜다. CSS만 수정 |
| workbench | ports/services 입력 UX + 자동 반영 | 입력 방법을 명확히 하고, WSL Desktop의 Docker/포트를 자동 반영 | **services는 입력 UI가 아예 없다** (`App.tsx:146-184`에 필드 없음, 항상 `[]`). ports는 매 키 입력마다 `join(", ")`으로 재포맷돼 입력이 튄다. **Docker 자동 반영은 wsl-desktop이 producer가 되어야 가능** → 연동 설계 §4.1 |
| webhook-lab | rule 필드 라벨/설명 | 각 rule 필드가 무엇인지 옆에 설명 표시 | **#282 구현 계약.** method는 대소문자 무시하고 빈 값(`None`)은 모든 method에 매치한다. path는 전체 문자열 exact match가 기본이며 마지막 문자가 `*`인 경우에만 `*` 앞부분 prefix match를 한다(중간 `*`는 literal). status·headers·body는 매칭 요청에 반환할 response이고 delay는 응답 전 대기 시간이다. status `100~599`, delay `0~60000ms` 정수 bounds와 method/path/header/body/rule-count 및 UTF-8 char/byte bounds를 frontend와 Rust IPC/storage에서 함께 검증한다. backend `upsert`는 실패 시 map을 변경하지 않고 raw input/secret 없는 고정 오류만 반환한다. 겹치는 rule에 대해 `HashMap` 순회 순서의 우선순위·결정성을 약속하지 않으며 id·순서·matcher semantics를 변경하지 않는다. label/help/error를 `aria-describedby`로 입력에 연결하고 invalid draft 보존, stale/busy/double action을 fixture로 고정한다. |
| webhook-lab | 규칙 저장 후 예시 curl 표시 | 규칙 설정 직후 테스트용 curl 예시 자동 생성 | **#283 구현 계약.** PowerShell `curl.exe`와 POSIX `sh curl`을 별도 context-menu 항목으로 제공하고 shell별 quoting을 독립 구현한다. fresh running address와 현재 rule을 재검증한 뒤에만 복사하며, trailing `*`는 backend prefix matcher와 같은 concrete sample로 바꾼다. response status/headers/body/delay는 request arguments가 아닌 주석 metadata로 표시한다. |

#### #283 Webhook Lab example curl 구현 세부

example curl은 설치·network·replay를 추가하지 않는 순수 formatter + clipboard action이다.
Windows desktop의 PowerShell 사용성과 WSL/POSIX 사용성을 함께 지원하되 `cmd.exe` 형식은
이번 항목에 포함하지 않는다.

- context menu는 `PowerShell curl.exe 복사`와 `POSIX sh curl 복사`를 각각 독립 action으로
  만든다. PowerShell single quote는 `'`를 `''`로, POSIX single quote는 close/escape/reopen
  방식으로 처리한다. 문자열에는 command substitution, variable expansion, URL glob이
  일어나지 않도록 하고 command에는 `--globoff`와 `--path-as-is`를 포함해 curl의
  dot-segment path 정규화도 차단한다.
- 복사 직전에 `serverStatus`와 `listRules`를 다시 조회한다. stopped/주소 없음이면 복사하지
  않고, 메뉴를 연 뒤 바뀐 bind address는 fresh 값을 다시 검증해 사용한다. 삭제된 rule은
  복사하지 않고 고정 alert를 표시한다. 허용 destination은 loopback IPv4/IPv6뿐이며
  `0.0.0.0`·`[::]` wildcard bind는 각각 `127.0.0.1`·`[::1]`로 바꾼다. 외부 주소와
  bracket 없는 IPv6는 거부한다.
- backend matcher가 rule path 전체의 마지막 문자 `*`만 wildcard로 취급하므로, `/events/*`
  는 `/events/example` 요청으로 구체화한다. 중간 `*`는 literal이며 exact path/query를
  trim·decode·re-encode해 matcher 결과를 바꾸지 않는다. absolute URL, host escape,
  fragment, raw/decoded whitespace/control, malformed percent escape, path token/placeholder를
  fail-closed한다. 민감 query를 `[REDACTED]`로 바꾸면 exact route가 변하므로 query 전체를
  거부한다.
- response rule의 status·headers·body·delay는 request method/path 조건이 아니라 서버가
  반환할 response metadata다. `--include`로 실제 response headers/body를 출력하고,
  response fields를 `--header`/`--data` request argument로 복사하지 않는다. raw secret
  reveal과 request replay는 비범위다.
- header/body의 placeholder는 값 전체가 `${NAME}` 또는 `{{NAME}}`일 때만 보존한다.
  `Bearer ${TOKEN}`·`prefix ${TOKEN}` 같은 mixed raw+placeholder는 전체 redact하고,
  JSON object key와 path placeholder는 거부한다. path 4,096자, header 100개·name 256자·value 16,384자·
  합계 64,000자, body 256,000자, JSON depth 32·node 10,000개·string 64,000자, 최종
  출력 512,000자, status 100~599, delay 0~60,000ms bounds를 적용한다. builder와 clipboard
  예외는 원문을 DOM에 반향하지 않는 고정 alert로 처리한다.
- `exampleCurl.test.ts`는 두 shell golden, quote metacharacter, concrete wildcard,
  placeholder/masking, sensitive query fail-closed, URI/address policy, 각 bounds를
  고정한다. `App.test.tsx`와 `contextMenus.test.ts`는 두 menu action, running/fresh
  address, stale selection, busy/double action, raw error alert, `Shift+F10`/Escape focus
  복원을 고정한다.

#### #282 Webhook Lab rule 설명 구현 세부

`apps/webhook-lab`의 rule editor는 값이 이미 채워져 있어도 method/path/status/delay/body의
label과 설명을 계속 노출한다. 설명과 backend `core/rules.rs::matches`의 계약은 다음처럼
일치해야 한다.

- method가 비어 있으면 프론트가 `null`을 보내고, backend의 `None`은 모든 method에 매치한다.
  값이 있으면 대소문자를 무시한 문자열 비교만 한다.
- path는 문자열 전체가 같은 exact match이거나, rule path의 마지막 문자가 `*`일 때
  `*`를 제외한 앞부분으로 시작하는 prefix match다. `/events/*`는 `/events/`·`/events/123`에
  매치하지만 `/eventslater`에는 매치하지 않으며 `/events/*/tail`의 중간 `*`는 wildcard가
  아니다. query를 포함한 URL 문자열도 backend가 전달한 문자열 그대로 비교한다.
- status·headers·body는 요청을 매치시키는 입력이 아니라 반환할 HTTP response의 구성이고,
  delay는 해당 response 전의 대기 시간이다. 매치 엔진/fixture/replay 동작 자체를 이 UX PR에
  추가하지 않는다.
- 현재 저장소는 rule을 `HashMap`에 보관하므로 여러 rule이 겹칠 때 순회 순서가 우선순위나
  결정성의 공개 계약이 아니다. UI/문서/fixture는 특정 rule이 항상 먼저 선택된다고 암시하지
  않으며, 겹침을 사용자가 피할 수 있도록 매칭 의미만 설명한다.
- editor와 Rust `set_rule`/`upsert`는 동일한 storage 경계를 사용한다. rule은 최대 200개,
  id는 최대 128자/128바이트, method는 `null` 또는 ASCII HTTP token 최대 16자/16바이트,
  path는 `/` 시작·control 금지·최대 4,096자/16,384바이트, status는 `100~599`, delay는
  `0~60000ms` 정수다. response headers는 최대 100개이고 이름 256자/256바이트, 값
  16,384자/65,536바이트, 이름+값 합계 64,000자/256,000바이트다. body는 256,000자/
  1,024,000바이트다. collection의 id/method/path/header 이름·값/body 문자열 합계는
  2,000,000자/8,000,000바이트다. char는 JS `Array.from`/Rust Unicode scalar count,
  byte는 UTF-8 `TextEncoder`/Rust `str::len()`으로 계산하고 새 UUID id의 36자 footprint도
  예약한다. Rust UTF-8로 표현할 수 없는 unpaired JavaScript surrogate는 frontend에서 먼저
  거부한다.
- backend는 frontend-only 방어에 의존하지 않으며 invalid input을 저장하거나 부분 변경하지
  않는다. IPC 오류는 `규칙 입력이 유효하지 않습니다`처럼 raw input·path·secret이 없는
  고정 문구다. frontend는 invalid raw draft를 유지하고 save/duplicate IPC를 호출하지 않으며,
  편집 대상이 refresh에서 사라진 stale id는 저장하지 않는다. `aria-invalid`, linked
  `aria-describedby`, `aria-busy`, disabled/double-action guard를 포함한다.

검증 fixture는 `src-tauri/src/core/rules.rs`에서 None method·대소문자 무시, exact path,
trailing-star prefix, 중간-star literal, query 문자열 차이, numeric/string/header/collection
경계와 invalid upsert의 no-mutation을 고정한다. 프론트 `ruleValidation.test.ts`는 같은
status/delay/method/path/header/body/count와 char/byte 경계를, `App.test.tsx`는 항상 보이는
설명/ARIA 연결·범위 초과 저장 차단·raw error 안전 대체·invalid draft 보존·stale save·busy
double action을 고정한다. 이 항목은 설명/검증 범위만 다루며 예시 curl(#283), fixture/replay
엔진은 별도 PR로 유지한다.

> 참고: v0.4.0-rc1에서 발견된 **기능 버그**는 별도 PR로 전부 수정 완료됨 —
> git 집계(#188), open_in/앱 실행(#186), 중복 실행(#187), grid/스크롤 레이아웃(#189·#190).
>
> 다만 #189은 `PaneCanvas.tsx:50`에 `repeat(0, 1fr)` 버그를 함께 들여왔다 —
> [터미널 설계](./2026-08-17-wsl-desktop-terminal-design.md) §2.7에서 수정.

---

## 5. 이전 제외 항목 재검토

“다시 검토하지 않음”이라는 초판 표현을 폐기한다. 외부 도구의 존재는 제외 근거가 아니며,
오프라인 제공 가치, 앱 책임, 안전, 설치 크기, 유지비를 함께 본다.

| 앱/영역 | 이전 항목 | v0.5.0 결정 | 경계 |
|---|---|---|---|
| developer-toolbox | QR 생성 | **P2 포함** | text/URL/Wi-Fi 생성과 SVG/PNG export. camera/dynamic service 제외 |
| api-playground | GraphQL | **P2 포함** | HTTP query/variables/operationName. introspection/codegen 제외 |
| api-playground | SSE | **P2 포함** | bounded stream parser와 stop/pause/reconnect opt-in |
| api-playground | WebSocket | **P2 포함** | ws/wss text/binary/ping/pong. Socket.IO/STOMP 제외 |
| everything-plus | PDF/DOCX/XLSX 내용 | **P2 포함** | text extraction만. OCR/암호/legacy Office/서식 제외 |
| port-manager | WSL 포트 | **P2/P3 포함** | distro·PID·start identity와 source provenance 필수 |
| life-log | 자동 일기 | 규칙 기반 local digest 포함 | cloud/local LLM 호출과 외부 전송 제외 |
| 범용 command palette | 신규 앱 제외 | **Devbox Launcher로 포함** | devbox action/context만. OS 범용 검색 제외 |
| log viewer | 조건부 후보 | **Log Lens 신규 앱 포함** | local/Run/WSL/container source. network ingest 제외 |
| data inspector | 독립 앱 후보 | Manager 내부 P3 포함 | devbox DB read-only. 임의 DB/write 제외 |
| knowledge-base | Git UI | 제외 유지 | Repo Manager 책임 |
| developer-toolbox | Cron 설명 | 제외 유지 | Run Manager 책임 |
| developer-toolbox | 도구별 설정 | 개별 설정은 제외 유지 | recent/favorite/pipeline만 제공 |
| port-manager | 점유 앱 icon | 제외 유지 | 작업 효율 대비 추출·cache 비용이 큼 |
| everything-plus | semantic search | 제외 유지 | embedding model/vector DB가 설치 규모 경계 초과 |
| clipboard | 범용 history | 제외 유지 | 호출 시 current clipboard만 일회성 routing |
| system | hosts/system env editor | 제외 유지 | Workbench project environment만 제공 |
| terminal | 범용 terminal | 제외 유지 | WSL Desktop의 WSL workflow는 native 강화 |
| container | Docker Desktop 복제 | 제외 유지 | state/log/port adapter만 제공 |

포함으로 바뀐 항목의 정확한 parser, buffer, file/time 상한과 secret 정책은 v0.5.0 네이티브
우선 계획 §4를 기준으로 한다. 제외 유지 항목도 근거가 바뀌면 이후 release에서 다시 검토한다.

---

## 6. 안전 경계 (공통)

- 컨텍스트 메뉴의 파괴 동작(Kill·삭제·제거·reset)은 `danger` 스타일 + 확인.
- 클립보드에 비밀(secret·Authorization·Cookie)이 남지 않게 한다. webhook-lab은
  **마스킹 복사를 기본**으로 하고 원본 복사는 별도 항목 + 확인으로 분리한다
  (마스킹된 것만 복사할 수 있으면 재현이 불가능하므로 두 항목 다 필요하다).
- 우클릭 메뉴는 파일 시스템 접근 시 기존 `safe_join`/루트 경계를 그대로 따른다.
- 붙여넣기는 `term.paste()`/bracketed paste를 쓰고, **개행이 포함되면 확인 프롬프트**를
  띄운다 (bracketed paste 미지원 셸에서 여러 줄이 즉시 실행되는 것을 막는다).

---

## 7. 권장 구현 순서

전체 저장소 순서는 v0.5.0 네이티브 우선 계획 §7이 단일 기준이다. 이 문서가 소유한 UX
작업은 다음 의존 순서로 들어간다.

```
v0.4.1 (핫픽스 — 결함만, 기능 추가 없음)
  터미널 결함 7종        → 터미널 설계 §2
  argv 계약 + 3개 앱 수신 → 연동 설계 §5.1

v0.5.0
  P1-선행  catalog capability + 런타임 배포
  P1-UX    packages/context-menu + 기존 13개 앱
  P1-WSL   clipboard·terminal 기본기·native workspace/profile
  P1-앱    Toolbox JSON/YAML·encoding·JSON→TS, API header/cookie/multipart,
           Knowledge wikilink/backlink/rename, Manager batch, Code Pad Quick Open/LSP,
           Workbench ports/services, Webhook label/curl
  P2       Toolbox QR·security/text 도구, Knowledge capture/image,
           API OpenAPI/GraphQL/SSE/WebSocket, Everything document content,
           handoff core + Webhook/API·Life Log/Knowledge integration
  P3       detection/pipeline/templates/filter/replay/window state + 신규 Launcher/Log Lens,
           Toolbox/API와 Run/WSL·Log Lens integration
```

컨텍스트 메뉴가 카탈로그 뒤로 간 이유: "다른 앱으로 열기"를 카탈로그에서
생성하지 않으면 13개 앱에 하드코딩이 들어가고, 나중에 걷어내는 비용이 13배가 된다.
복원된 QR·stream protocol·document extractor는 새 dependency와 입력 안전 경계가 있어
작은 UX 수정과 한 PR에 섞지 않는다.

각 항목은 기능 단위 1 PR로 진행한다.

---

## 8. 테스트 계획

이 저장소의 완료 기준은 "Rust 유닛 테스트 + clippy + 프론트 빌드 통과"다. 순수 함수로
분리 가능한 것을 분리해 테스트한다 — `lib/shortcuts.ts`/`shortcuts.test.ts`가 선례다.

| 대상 | 방법 |
|---|---|
| 메뉴 포지셔닝 | 순수 함수로 분리. 뷰포트 경계 4방향에서 뒤집힘, 스크롤 오프셋 반영 |
| 메뉴 항목 생성 | 앱별 `buildMenu(target, state) -> MenuItem[]`을 순수 함수로. 비활성 조건(선택 없음 → 복사 disabled 등) 단언 |
| 키보드 이동 | ↑↓ 순환, separator 건너뛰기, disabled 건너뛰기, Esc 닫기 |
| 카탈로그 기반 "열기" | 설치되지 않은 앱 제외, `accepts` 불일치 앱 제외 |
| developer-toolbox 변환기 | 왕복 테스트 (`transformers.test.ts` 확장). 잘못된 입력에 예외 대신 오류 메시지. QR SVG/PNG golden·size 상한 |
| webhook-lab curl | 규칙 → curl 문자열 골든 테스트. 인용이 필요한 body |
| devbox-manager 일괄 설치 | #274 fixture가 부분 실패 뒤 성공분 유지·실패분 선택·exact-mode retry, setup 확인을 검증. 신규 앱 등록 뒤 `App.test.tsx`의 catalog 개수도 15개로 갱신 |

**실기 검증(Windows)** — 컨텍스트 메뉴는 WebView2 기본 메뉴 억제와 IME가 얽히므로 실기
확인이 필요하다:
- 13개 앱에서 우클릭 → 앱 고유 메뉴가 뜨고 웹뷰 기본 메뉴가 뜨지 않는다
- 텍스트 입력 영역에서 한글 입력(조합 중 포함)이 메뉴 도입 전과 동일하다
- Shift+F10으로 메뉴가 열리고 Esc로 닫으면 포커스가 원래 요소로 돌아온다

---

## 9. 단축키 일관성

13개 앱에 메뉴가 붙고 wsl-desktop에 새 단축키가 들어오므로 충돌 표를 유지한다.

| 단축키 | 의미 | 적용 |
|---|---|---|
| `Shift+F10` / Menu 키 | 컨텍스트 메뉴 열기 | 13개 앱 공통 |
| `Esc` | 메뉴/모달 닫기 | 13개 앱 공통 |
| `Ctrl+C` / `Ctrl+V` | 복사 / 붙여넣기 | 일반 앱 |
| `Ctrl+C` | **SIGINT** | wsl-desktop 터미널 팬 (복사는 드래그 자동 복사) |
| `Ctrl+Shift+C` / `Ctrl+Shift+V` | 복사 / 붙여넣기 | wsl-desktop |
| `Ctrl+Shift+F` | 스크롤백 검색 | wsl-desktop |
| `Ctrl+Shift+T/D/W`, `Ctrl+Tab`, `Alt+Arrow` | 탭·팬 조작 | wsl-desktop (기존) |
| `Ctrl+Alt+K` | global quick capture | knowledge-base, 충돌 시 설정 변경 |
| `Ctrl+Alt+Space` | devbox action launcher 열기 | devbox-launcher, 충돌 시 설정 변경 |

새 단축키를 추가하는 PR은 이 표를 함께 갱신한다.
