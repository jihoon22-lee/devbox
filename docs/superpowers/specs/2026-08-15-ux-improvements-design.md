# UX 개선 설계 — 컨텍스트 메뉴·클립보드·도구 확장

- 상태: 제안(Proposal) — 다음 단계(Post v0.4.0)
- 작성일: 2026-08-15
- 근거: `docs/product-opportunities.md` §11.1~11.3(기능 순서), §12·§13·§14(기존 앱 확장)
- 선행: PR 1~39 + Stage 4/5 (모두 완료), 13개 앱 + 공용 crates/packages

## 0. 배경

13개 앱이 기능적으로 완성된 뒤 남은 불편은 대부분 **상호작용(UX) 격차**다.
현재 전 앱에 `onContextMenu` 핸들러가 0개라 우클릭은 웹뷰 기본 반응(또는 무반응)에
그치고, wsl-desktop은 단축키가 탭/팬 이동뿐이라 복사·붙여넣기가 실질적으로 불편하다.
이 문서는 그 격차를 메우는 작업을 정리한다.

원칙:
- 각 앱의 책임을 다른 앱에서 복제하지 않는다.
- 파괴적 동작(삭제·종료·제거)은 `danger` 스타일 + 확인을 기본으로 한다.
- 공용 코드는 두 번째 실제 소비자가 생길 때 `packages/`·`crates/`로 추출한다.

## 1. 우클릭 컨텍스트 메뉴

### 1.1 공통 구현 방향

네이티브 메뉴(`tauri-plugin-menu`)는 13개 앱 전부에 붙이기 무겁고 플러그인 의존이
늘어나므로 **HTML 기반 공용 컴포넌트**를 만든다. `onContextMenu` + `preventDefault()`
로 화면에 띄우고, 각 앱이 메뉴 항목 목록만 넘기는 구조다.

```ts
type MenuItem =
  | { type: "item"; label: string; action: () => void; disabled?: boolean; danger?: boolean }
  | { type: "separator" };

// 사용 예
<ContextMenu items={[
  { type: "item", label: "경로 복사", action: () => copyPath(path) },
  { type: "separator" },
  { type: "item", label: "삭제", action: () => remove(path), danger: true },
]} />
```

- **`packages/context-menu`**로 추출: 첫 소비자(아래 1.2)에서 앱 로컬로 만들고,
  두 번째 앱에서 필요해지는 순간 공용 패키지로 옮긴다 (저장소 원칙).
- 위치: 뷰포트 기준 포지셔닝, 화면 밖 클릭/Esc로 닫기, 스크롤 시 닫기.
- 접근성: 키보드(↑↓·Enter·Esc) 지원.

### 1.2 앱별 메뉴 항목

| 앱 | 대상 | 항목 |
|---|---|---|
| port-manager | 포트/프로세스 행 | 포트 복사 · PID 복사 · `localhost:port` URL 복사 · localhost 열기(LISTEN) · 프로세스 경로 복사 · 탐색기에서 열기 · **Kill**(danger) |
| developer-toolbox | 입/출력 텍스트 영역 | 복사 · 모두 선택 · 붙여넣기 · 비우기 · 결과 파일로 저장 |
| everything-plus | 검색 결과 행 | 열기 · 폴더에서 보기(Explorer /select) · 경로 복사 · 파일명 복사 · **Code Pad로 열기**(텍스트) |
| knowledge-base | 파일 트리 노드 | 새 파일 · 새 폴더 · 이름 변경 · 삭제(danger) · 경로 복사 · 탐색기에서 열기 · **Code Pad로 열기** |
| code-pad | 에디터 탭 | 닫기 · 다른 탭 닫기 · 경로 복사 · 탐색기에서 열기 · 이름 변경/삭제 |
| run-manager | 작업/서비스/이력 행 | 작업: 지금 실행·활성화/비활성화·편집·로그 열기·삭제 / 서비스: 시작·정지·재시작·편집·삭제 / 이력: 로그 보기·재실행 |
| devbox-manager | 앱 목록 행 | 설치/업데이트 · 실행 · 이전 버전 롤백 · 설치 폴더 열기 · 제거 |
| workbench | 프로젝트 프로필 | Start Workspace · Stop What I Started · 프로필 편집 · 삭제 · 경로 복사 |
| webhook-lab | 수신 요청 history | 요청 본문 복사(masking) · 헤더 복사 · **API Playground로 변환** · 삭제 |
| repo-manager | 저장소 행 | Code Pad로 열기 · WSL Desktop로 열기 · Workbench로 열기 · worktree 생성 · 경로 복사 |
| api-playground | History/Collection 항목 | 복제 · 이름 변경 · 삭제 · curl 복사 |
| life-log | 캘린더 날짜 | 날짜 복사 · 해당 날짜 Markdown/JSON export |

> life-log는 읽기 전용 집계 앱이므로 메뉴를 최소화한다.

## 2. wsl-desktop 복사/붙여넣기

### 2.1 현재 문제

`lib/shortcuts.ts`는 탭/팬 이동(Ctrl+Shift+T/D/W, Ctrl+Tab, Alt+N)만 처리하고,
복사·붙여넣기·선택은 xterm.js 기본값에 의존한다. Tauri 웹뷰에서는 브라우저
붙여넣기(Ctrl+V)와 우클릭이 실질적으로 동작하지 않아 "붙여넣기가 힘들다"는 증상이
발생한다.

### 2.2 개선 항목

| # | 항목 | 내용 |
|---|---|---|
| 1 | 명시적 단축키 | `Ctrl+Shift+C`=복사, `Ctrl+Shift+V`=붙여넣기 (Windows Terminal과 동일) |
| 2 | Ctrl+C 스마트 처리 | `term.hasSelection()`이면 복사, 아니면 SIGINT를 PTY에 전송 |
| 3 | 우클릭 붙여넣기 | `onContextMenu`에서 preventDefault 후 클립보드 내용을 PTY에 전송 |
| 4 | 복사 시 선택 | 선택이 생기면 자동 복사 (Windows Terminal 동작) |
| 5 | 클립보드 채널 | `@tauri-apps/plugin-clipboard-manager` 또는 `navigator.clipboard` (+ CSP) |
| 6 | (선택) | 마우스 가운데 클릭 붙여넣기, 트리플클릭 줄 선택 |

- 구현 위치: `TermPane.tsx` 단일 변경. 공용화 불필요(소비자 1개).
- 주의: Ctrl+C 분기는 readline(SIGINT) 동작을 망가뜨리지 않도록 `hasSelection()`
  기준으로만 분기한다.

## 3. developer-toolbox 도구 확장

현재 14종(JSON/Encoding/Time/Text/Security/Regex/JWT). **오프라인·저비용·고빈도**
기준으로 추가를 제안한다.

| 도구 | 그룹 | 구현 | 가치 |
|---|---|---|---|
| JSON ↔ YAML 변환 | JSON | TS(`js-yaml`) | 높음 — 설정 파일 작업 |
| 진법 변환 (Hex/Dec/Bin/Oct) | Encoding | TS | 높음 — 디버깅 기본기 |
| JSON → TypeScript 타입 생성 | JSON | TS | 높음 — API 응답 → 인터페이스 |
| UUID 다량 생성 + ULID | Security | Rust(기존 `uuid` 확장) | 중간 |
| HTML Entity Encode/Decode | Encoding | TS | 중간 |
| HMAC / JWT 서명 검증(HS256) | Auth | Rust(`hmac`) | 중간 — 기존 JWT 확장 |
| Lorem Ipsum/placeholder 생성 | Text | TS | 중간 |
| Markdown 테이블 생성기 | Text | TS | 중간 |

추가 도구는 기존 `ToolDef[]` 레지스트리에 한 줄 추가만 하면 좌측 메뉴에 자동 등록되는
구조를 그대로 이용한다 (`apps/developer-toolbox/src/tools/index.tsx`).

## 4. 기타 앱별 추천

### 4.1 기존 계획에서 확정된 항목 (세부화)

| 앱 | 항목 | 범위 | 난이도 |
|---|---|---|---|
| knowledge-base | 백링크 + 역링크 패널 | `[[wikilink]]` 파싱(`core/markdown` 기반 존재) → unresolved link → 역링크 패널 | 중 |
| knowledge-base | 퀵캡처(전역 단축키)+Inbox | global shortcut → Inbox note | 중 |
| knowledge-base | 첨부파일(이미지) 관리 | root 내 attachment 폴더, 이미지 드롭·삽입·프리뷰 | 중 |
| api-playground | 파일 업로드 | `multipart/form-data`, `reqwest::multipart` | 저 |
| api-playground | 응답 헤더/쿠키 뷰어 | 응답 headers 존재 → 전용 탭 추가 | 저 |
| api-playground | OpenAPI 3 import | 스펙 파싱 → endpoint 요청 초안 | 중 |
| life-log | Markdown/JSON export | 집계 결과 직렬화 | 저 |
| port-manager | 프로세스 명령줄 표시+복사 | Win32 process command line 조회 | 중 |

### 4.2 신규 추가 항목

| 앱 | 항목 | 근거 |
|---|---|---|
| everything-plus | 검색 결과 → Code Pad로 열기 | `product-opportunities.md` P1에 명시된 크로스앱 연동 |
| code-pad | 탭 우클릭 + "다른 탭 닫기/탐색기 열기" | 파일 다중 편집기 기본 UX (§1.2와 연계) |
| run-manager | 로그 뷰어 검색/필터 | 회전 로그 tail에 검색 부재 시 장기 로그 사용 불가 |
| devbox-manager | 설치 폴더 열기·제거 | Manager 관리 완결성 (§1.2와 연계) |

### 4.3 실사용 피드백 (v0.4.0-rc1)

Windows 실기 검증에서 수집한 UX 개선 항목. 기능 버그가 아니라 편의·가독성 항목이다.

| 앱 | 항목 | 설명 |
|---|---|---|
| devbox-manager | 일괄 설치/업데이트 + 다중 선택 | 여러 앱을 체크박스로 선택해 한 번에 설치/업데이트 |
| devbox-manager | 설치 위치 표시 + 지정 | 현재 설치 경로를 표시하고, 사용자가 변경 가능하게 |
| wsl-desktop | Docker 패널 컴팩트 포맷 | 좌우 잘림을 줄이도록 트리/축약 포맷 도입 |
| code-pad | 언어 서버 패널 높이 확보 | 언어 서버 목록 패널이 좁아 가독성이 떨어짐 |
| code-pad | 빠른 열기 → 트리 + 탭/패널 | 평면 리스트 + 잘림을 탐색기형 트리로 개선 |
| code-pad | 상태 표시줄 하단 고정 | 파일 길이에 따라 움직이지 않고 항상 최하단 고정 |
| code-pad | 프리뷰/편집 영역 구분 강화 | 프리뷰 영역이 편집 영역과 시각적으로 구분되게 |
| workbench | ports/services 입력 UX + 자동 반영 | 입력 방법을 명확히 하고, WSL Desktop의 Docker/포트를 자동 반영 |
| webhook-lab | rule 필드 라벨/설명 | 각 rule 필드가 무엇인지 옆에 설명 표시 |
| webhook-lab | 규칙 저장 후 예시 curl 표시 | 규칙 설정 직후 테스트용 curl 예시 자동 생성 |

> 참고: v0.4.0-rc1에서 발견된 **기능 버그**(git 집계 실패, open_in 실행 실패, 중복 실행,
> 그리드/스크롤 레이아웃 깨짐)는 이 문서가 아니라 별도 버그 수정으로 처리한다.

## 5. 제외 확정 (다시 검토하지 않음)

이전 검토에서 비추천·부적합으로 제외한 항목. 근거를 남겨 재논의를 막는다.

| 앱 | 제외 항목 | 근거 |
|---|---|---|
| knowledge-base | Git 커밋/로그 UI | Repo Manager가 git 담당, 책임 중복 |
| developer-toolbox | Cron 표현 설명 | Run Manager cron 빌더·미리보기와 중복 |
| developer-toolbox | QR 생성 | 의존 추가 대비 가치 낮음 |
| developer-toolbox | 도구별 설정 저장 | 도구 대부분 stateless |
| port-manager | WSL 포트 연동 | wsl-dashboard 시대 구식 계획, WSL2 NAT로 의미 애매 |
| port-manager | 점유 앱 아이콘 표시 | 아이콘 추출 비용 대비 가치 낮음 (명령줄은 §4.1 추천) |
| api-playground | GraphQL | 기존 JSON body 에디터로 충분 |
| api-playground | SSE | 우선순위 최하위 |
| everything-plus | PDF/DOCX/XLSX 내용 추출 | 파서 의존 무거움, 오프라인 경량 앱 부적합 |
| everything-plus | 시맨틱 검색 | 로컬 임베딩+벡터 DB 필요, 부적합 |
| api-playground | WebSocket | reqwest 미지원, 스택 교체 필요 |
| life-log | 자동 일기(LLM) | 개인 활동 데이터 외부 전송 → privacy 경계 위배 |

## 6. 안전 경계 (공통)

- 컨텍스트 메뉴의 파괴 동작(Kill·삭제·제거·reset)은 `danger` 스타일 + 확인.
- 클립보드에 비밀(secret·Authorization·Cookie)이 남지 않게 한다 (webhook-lab masking,
  api-playground secret은 이미 평문 복사 금지 원칙).
- 우클릭 메뉴는 파일 시스템 접근 시 기존 `safe_join`/루트 경계를 그대로 따른다.

## 7. 권장 구현 순서

1. `packages/context-menu` + 앱별 우클릭 (§1) — 최대 체감 개선, 일괄 적용
2. wsl-desktop 복사/붙여넣기 (§2) — 단일 앱, 직접 호소된 불편
3. developer-toolbox: JSON↔YAML + 진법 변환 + JSON→TS 타입 (§3 저비용 3종)
4. api-playground: 응답 헤더/쿠키 뷰어 + 파일 업로드 (§4.1 저난이도)
5. knowledge-base 백링크 / everything-plus→Code Pad 열기 (§4.1·4.2 중난이도)
6. 실사용 피드백 항목 (§4.3) — devbox-manager 다중 선택·설치 위치, code-pad 레이아웃,
   webhook-lab 라벨·curl 예시, wsl-desktop Docker 포맷, workbench 포트/서비스

각 항목은 기능 단위 1 PR로 진행한다.
