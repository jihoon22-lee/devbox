# Devbox v1 설계 (설계 원장)

- 상태: v3 확정 · 교차 검증 반영 · 권장안 확정(2026-10-08, 00-roadmap §5) · 세 계획 통합·검수 반영(2026-10-08, [08 §8·§9](08-review-record.md)) · 구현 미착수
- 작성: 2026-10-07 · 기준 원격 `main@72ff50c7`(v0.9.0 공개본). 이 PC의 로컬 main은 그 위에 원격에 없는 커밋 2개(끝난 계획 정리·의존성 보안 업데이트)가 더 있다
- 입력:
  - 본질 기준 전면 리뷰(2026-10-07). 결론은 이 문서 §0·§2·§3에 흡수했고, 원본 보고서(`~/projects/devbox-review-2026-10-07/`)는 2026-10-08에 지웠다.
  - 아래 §0의 사용자 결정
  - [교차 검증 기록](08-review-record.md)
  - 다른 세션의 v0.9.0 리뷰 교차 검증 두 건(CV·AR, 2026-10-07). 결론·결함·재현 입력값은 08 §8과 00-roadmap §4·§7에 옮겼고, 원본 폴더는 통합 뒤 지웠다(08 §9)
- 이 문서는 하위 프로젝트 S0a–S6 전체가 따르는 공통 설계다. 실행 순서와 작업은 [00-roadmap](00-roadmap.md)과 각 계획 문서가 정한다.
- 외부 검토 번호는 출처를 앞에 붙인다: `RV-`(본질 리뷰), `CV-`, `AR-`, `AU-`(10-03 감사: `docs/superpowers/plans/2026-10-03-product-readiness/08-traceability.md`(v0.9.0 공개본 커밋 `72ff50c7`에 있음: `git show 72ff50c7:<경로>`)의 S·W·AS·AX·K·C 번호에 `AU-`를 붙임), 교차 검증 소견 `UX-`·`FS-`·`SE-`·`PD-`·`PL-`.

## 0. 결정 요약

| ID | 결정 | 근거 |
|---|---|---|
| D1 | **본질:** Windows 11 + WSL에서 혼자 개발하는 사람이 여러 프로젝트를 AI 에이전트와 함께 열고·돌리고·지켜보고·기록하는 일을 한곳에서 잇는 개인 개발 허브. 개발에 필요한 도구를 한곳에 모으는 것도 목적에 포함한다. | 사용자 답 1 |
| D2 | 앱 개수는 기준이 아니다. 기준은 **높은 품질과 쉬운 사용성**이다. 이 설계는 Windows 앱 하나와 WSL 데몬 하나로 구성한다. | 답 1·4 |
| D3 | 사용 빈도: **A 에이전트 ≫ B 실행·관찰 = C 터미널 ≫ E 기록 > D 편집 = F API = G 변환 도구**. D·F·G는 빼지 않는다. | 우선순위 답 |
| D4 | 편집기는 파일 편집 + 언어별 하이라이트 + LSP 검사면 된다. 형태는 상관없다. | 답 2 |
| D5 | 개발 도구는 "정말 쓸 일이 없을 것"만 뺀다. | 답 3 |
| D6 | 설치·배포는 관리·배포가 쉬운 방식으로 한다. | 답 5 |
| D7 | 과도한 검증을 줄이고, 검증 비용을 실제 품질 기여에 맞춘다. | 답 6 |
| D8 | v0.9.0(및 이전) 데이터는 가져오지 않는다. | 답 7 |
| D9 | **WSL 중심:** 실제 작업은 WSL 안의 Linux 데몬이 하고, Windows에는 얇은 Tauri 앱이 화면·트레이·단축키·알림·활동 기록을 맡는다. 프로젝트는 `/home/<user>/projects` 아래에 있다. Windows 앱 개발도 "WSL에서 개발, Windows에서 테스트"로 한다. | 답 + 승인 |
| D10 | 기술 스택은 Tauri v2·React 19·TypeScript·Rust·pnpm 9을 유지한다. | 가정, 이의 없음 |
| D11 | 같은 저장소의 `main`에서 빈 구조로 다시 짓는다. 기존 코드는 `v0.9.0` 태그로 보존하고 참고용 worktree로만 연다. | 권장안 |
| D12 | 버전은 모든 하위 프로젝트가 끝난 뒤 한 번만 올린다(v1.0.0). 중간 공개 릴리스는 없다. 그 사이에는 로컬 빌드로 직접 써 본다. | 기존 사용자 규칙 |
| D13 | 보안은 개인용 범위(ADR 0016)를 유지한다. 비밀 값은 DPAPI 봉인 키로 보호한다. | 기존 사용자 규칙, Q1 확정 |
| D14 | 제외 후보 X-1–X-10은 모두 v1에서 뺀다. | Q2 확정 |
| D15 | 앱 identifier는 `io.github.jihoon22lee.devbox`, 노트 vault 기본 위치는 `~/notes`다. | Q3·Q4 확정 |
| D16 | 계획은 subagent-driven-development로 실행한다(과제별 구현 + 검토 하위 에이전트). | 권장안 확정 |

### 0.1 점진 개선 의견과의 관계

다른 세션의 검토(CV·AR)는 "현 구조를 유지하고 결함과 흐름을 단계적으로 고치자"고 권고했다. 그 판단의 전제는 네 가지였다.
- 기존 데이터·설치·연동의 보존
- Windows·WSL의 대칭 지원
- 네 제품 경계
- 현재 검증 정책

이번 결정 D8·D9·D2·D7로 네 전제가 모두 바뀌었다. 바뀐 조건에서는 다음 계층이 필요 없다.
- 설치 세대·활성화
- 제품 간 연결·전달
- Windows 측 프로세스·파일 처리
- WSL helper 중계

측정(2026-10-08, 08 §8.2)으로는 이 계층의 전용 코드와 각 엔진의 Windows·WSL 이중 경로 파일이 전체 약 49.7만 줄의 약 25%(파일 단위 하한)이고, `.github`·`scripts`가 5.6만 줄이다. 더 큰 이유는 본질의 중심인 에이전트 기능(앱을 닫아도 유지되는 세션, hook 상태 감지, 알림, 재개)이 현 구조에 없고, 두 검토가 확정한 결함 약 20건 중 에이전트·터미널 결함은 0건이라는 점이다. 결함을 고쳐도 사용 빈도 1위 영역은 나아지지 않는다. 그래서 재구축을 택한다. 다만 그 의견에서 다음은 받아들인다.

1. **구조 확인 실험:** S0a에서 새 구조의 핵심 가정을 측정한다. 같은 여정(에이전트 시작 → 알림, 터미널 입력 지연)을 v0.9.0과 수치로 비교하고, 실패하면 구조를 바꾼다.
2. **확정 결함은 회귀 조건으로:** 두 검토가 확정한 결함 전부(CV-F01–F09, AR-F01·F02·F04·F06–F10·F12·F13·F15·F16·D01, 재시작 backoff 정책 AR-F14)를 새 구조의 회귀 조건으로 넣는다(§9.1). 결함별 시험 위치와 입력값은 [00-roadmap §7](00-roadmap.md) 추적표에 있다.
3. **근본 원인은 설계 규칙으로:** 결함들의 공통 원인(본 것과 실행하는 것의 분리, 성공한 변경 뒤 조회 실패, 관찰 신호와 데이터 변경의 혼동, 화면 수명과 작업 수명의 혼동, 전환 때 초안 버림)을 §8.6 정합성 규칙으로 둔다.
4. **기존 보호를 다시 만들지 않기:** 검증된 로직은 테스트와 함께 그대로 옮긴다(§9.3 이식 표).
5. **v0.9.0 사용 중 회피:** 재구축 기간 동안 v0.9.0에서 피할 동작 목록을 [00-roadmap §4](00-roadmap.md)에 둔다.

## 1. 목표와 성공 기준

**목표.** 사용자가 하루를 시작할 때 devbox 하나를 열면 다음이 끊김 없이 이어진다.

1. 진행 중인 에이전트 작업의 상태(입력 대기·실패·검토 대기)를 보고 바로 대응한다.
2. 프로젝트의 개발 서버·작업을 띄우고 로그·포트·오류를 지켜본다.
3. 터미널에서 일한다.
4. 필요할 때 파일을 고치고, API를 시험하고, 변환 도구를 쓰고, 기록을 남긴다.

**성공 기준.** 최종 측정은 S6에서 한다. 각 하위 프로젝트의 완료 조건에도 해당 항목을 넣는다.

| ID | 기준 | 측정 방법 |
|---|---|---|
| SC1 | 첫 설치: WSL(systemd 켜진 배포판)이 있는 계정에서 devbox 흔적을 지운 뒤, setup 실행 → 첫 화면까지 사용자가 할 일은 배포판 확인 1번 이하 | 수동 1회 |
| SC2 | 트레이 상주 중 전역 단축키 → 명령 팔레트 표시 300ms 이내. 콜드 스타트 → 첫 화면 데이터 표시 3초 이내(WSL 실행 중), 10초 이내(WSL 정지 상태) | 앱 계측 로그 |
| SC3 | 새 에이전트 작업: 제목 입력 + 도구 선택 + 시작 1번으로 worktree 준비·세션 생성·도구 실행까지. 입력 대기·턴 종료가 2초 안에 에이전트 목록과 Windows 알림에 반영 | L3 + 수동 |
| SC4 | 앱 창 닫기·데몬 재시작·앱 업데이트 뒤에도 에이전트·터미널 세션과 서비스가 유지 (WSL 종료·PC 재부팅은 제외. 재부팅 뒤에는 에이전트 "재개"로 이어감) | L2 + 수동 |
| SC5 | 사용자에게 보이는 오류는 모두 정해진 코드와 "원인 + 다음 행동" 문구로 끝난다. `internal`은 진단 ID와 함께만 나온다 | 생성 타입의 전수 검사(컴파일 오류) |
| SC6 | 개발 루프: 화면 코드 변경 → 실제 데몬에 연결된 브라우저 화면 반영 2초 이내. 데몬 코드 변경 → 재시작·재연결 15초 이내 | 개발 스크립트 |
| SC7 | 검증 비용: PR CI 15분 이내, 로컬 전체 테스트(L3 포함) 10분 이내, 릴리스 워크플로 40분 이내. `.github` + `scripts` 합계 1,500줄 이하, 워크플로 3개 이하. flaky 테스트는 발견한 PR에서 고치거나 지운다(재시도 래퍼 금지) | CI 기록, 줄 수 |
| SC8 | 규모 예산(관찰용 트리거, 게이트 아님): 제품 코드(Rust + TS, 테스트 제외) S1 끝 6만, S3 끝 12만, S6 끝 20만 줄 이하. 초과하면 다음 S 전에 단순화를 검토 | 줄 수 통계 |
| SC9 | 데몬 유휴 자원: 실행 중인 작업이 없을 때 RSS 60MB 이하, CPU 0.5% 이하 | `systemctl --user status`, `/proc` |
| SC10 | 터미널 반응: 입력 1바이트 → 같은 바이트의 출력 수신(브리지·데몬·PTY·tmux 경유, 화면 그리기 제외) p50 10ms·p95 25ms 이하. 한 터미널이 100MB를 출력하는 동안 다른 창의 `system.ping` p95 100ms 이하, 출력이 끝난 뒤 그 터미널 입력이 1초 안에 반영 | S0a 측정(IR-6), S1 Task 24 계측 |

## 2. 기능 범위

표기:
- **유지:** 같은 사용자 가치를 새 구조로 다시 제공
- **단순화:** 가치는 유지하되 형태를 줄임
- **신규:** v0.9.0에 없던 기능
- **제외:** v1에서 빼는 것(Q2에서 모두 빼기로 확정)
- **폐기:** 새 구조에서 의미가 없어진 것

### 2.1 핵심 (A·B·C)

| 영역 | 기능 | 판정 | 비고 (v0.9.0 출처) |
|---|---|---|---|
| 프로젝트 | WSL 경로 프로젝트 등록·자동 탐색(`~/projects`, 에이전트 worktree 위치는 제외), clone으로 추가, worktree 목록, 즐겨찾기 | 유지 | `projects-engine`, `workspace-core`, ProjectWizard(clone만) |
| 프로젝트 | 시작 구성 `[launch]`: 서비스·터미널·worktree 없는 에이전트 작업·열 편집기를 한 번에 시작 | 단순화 | profiles/templates/Development Sessions를 하나로 |
| 프로젝트 | 프로젝트 설정 파일 `.devbox/devbox.toml`(선택, Git 공유). 다른 `.devbox/` 파일(v0.9.0의 `project.json` 등)은 무시 | 신규 | 두 PC 간 공유 수단 |
| 에이전트 | 작업 생성: 제목·도구 프로필·기준 브랜치 선택 → worktree(또는 "worktree 없이 현재 체크아웃에서") → 준비(`[agent] copy`·`setup`) → tmux 세션 → 도구 실행 + 초기 지시문 | 유지·강화 | `agent_hub` |
| 에이전트 | 목록: 긴급순(입력 대기 → 실패 → 검토 대기 → 실행 중 → 준비 중 → 닫힘). 행마다 hook 메시지 한 줄, 경과 시간, CPU·메모리, 토큰 합계, 변경 파일 수, 기준 대비 뒤처짐 | 강화 | `agent_resources`, `agent_usage` |
| 에이전트 | 격자 보기: 여러 에이전트 터미널을 읽기 전용으로 동시에(분리 창 가능) | 신규 | |
| 에이전트 | 상태 감지: Claude Code hooks(`--settings`로 실행마다 주입, 사용자 설정은 건드리지 않음)·Codex `notify`(`-c`로 주입, 턴 완료만) → `devbox agent event` → 데몬. Codex의 입력 대기와 hook이 없는 도구는 tmux 출력 감지가 기본. `DEVBOX_AGENT_ID`가 없으면 hook은 아무것도 하지 않음 | 신규 | 화면 긁기 대신 공식 hook(PL-5) |
| 에이전트 | 세션 ID 기록과 재개(`claude --resume <id>`, `codex resume <id>`). 지시문·이어서 지시 이력·토큰 합계·커밋 목록을 종료 시 DB에 고정 | 신규 | Claude transcript는 30일 뒤 삭제됨 |
| 에이전트 | 검토: 변경(파일별 '봤음', 줄 인용으로 지시) · 커밋 · 실행(테스트) 탭 → 병합(merge·squash·rebase) · PR(gh) · 폐기 · 이어서 지시 | 강화 | Source 검토 흐름 |
| 에이전트 | 기준 반영(rebase)과 뒤처짐 표시. 충돌 시 충돌 화면 또는 에이전트에게 해결 지시 | 신규 | |
| 에이전트 | 정리: 병합·폐기 뒤 tmux 세션·worktree·로컬 브랜치 정리(기본 켬, teardown 명령 실행), 연결 없는 worktree·병합된 브랜치 정리 목록 | 유지 | `CleanupPanel` |
| 에이전트 | 도구 프로필(이름 + 명령줄, 예: `claude --permission-mode plan`), 지시문 템플릿(`.devbox/prompts/*.md` + 변수 치환), 작업별 포트(`DEVBOX_PORT`부터 10개) | 신규 | |
| 에이전트 | Windows 알림: 입력 대기·턴 종료·실패. 작업 표시줄 오버레이 배지·창 깜빡임, 같은 작업 알림은 교체 | 신규 | |
| 에이전트 | MCP 서버: 프로젝트·실행·로그·문제·노트 조회, 작업 실행·노트 추가(쓰기는 설정에서 켬) | 유지 | `apps/devbox-agent/src/mcp` |
| 에이전트 | 넣지 않음: 달러 비용·한도 대시보드(llm-usage와 겹침), 동시 실행 상한·대기열(wsl-resource-guard와 겹침), transcript 전체 뷰어(스크롤백·재개로 충분) | — | FS-17 |
| 터미널 | tmux 기반 세션(앱을 닫아도 유지), 탭·분할(분할 칸 하나 = tmux 세션 하나), 프로젝트별 시작 위치, 프로필, 검색·링크·복사, 분리 창, 빠른 호출 전역 단축키 | 유지 | `terminal-engine` |
| 터미널 | 셸 연동 최소 범위(OSC 7 현재 위치 → 새 탭·분할 위치, 벨 표시) | 단순화 | ShellIntegration |
| 터미널 | 기존 zellij 세션에 붙기 | 단순화 | 새 세션은 tmux로 |
| 실행 | 작업(일회성)·서비스(상시)·예약(cron, DST 고려). 의존 순서, 재시작 정책(systemd), 환경 변수·비밀 참조, 실행할 worktree 선택 | 유지 | `runtime-engine` |
| 실행 | 서비스 준비 상태: `ports_hint` 포트가 수신 중이면 "준비됨"(관찰만). 서비스 의존 순서는 앞 서비스의 준비됨을 기다린다(기본 60초, 넘으면 실패). health 실패로 재시작하지 않는다 | 단순화 | v0.9.0 health probe·유예·health 재시작 폐기(AR-F02, IR-4) |
| 실행 | 작업 자동 감지(package.json·Makefile·justfile·Cargo·docker compose) | 유지 | tasks ImportDialog |
| 실행 | 로그: 실시간 tail, 검색·필터, 여러 소스 병합 보기. 소스는 devbox 실행·파일 tail·Docker/Podman 컨테이너 | 유지 | `logs-engine` |
| 실행 | Docker 컨테이너 목록·상태·시작/중지/재시작, 포트의 컨테이너 표시 | 유지 | `terminal-engine` docker_ps·docker_action |
| 실행 | 포트: 수신 포트 → 프로세스·실행·컨테이너 연결, 브라우저로 열기, 점유 프로세스 종료 | 유지 | `ports-engine` |
| 실행 | 프로세스: 프로젝트 관련 우선 목록, 종료 | 유지 | |
| 실행 | 문제: 로그·빌드 출력·LSP 진단에서 file:line 추출 → 편집기로 | 유지 | `problems_host` |
| 실행 | WSL 상태 요약: 메모리·스왑·PSI·디스크 | 단순화 | wsl-resource-guard와 겹쳐 요약만 |
| 실행 | Windows에서 실행: `target = "windows"` 작업은 Windows 사본 폴더(`windows_dir`)로 동기화한 뒤 PowerShell로 실행 | 신규 | `cmd.exe`는 UNC 작업 폴더 불가, WSL `node_modules`는 Linux용(FS-14) |
| 실행 | 알림: 예약 실패, 서비스 재시작 한도 초과 | 신규 | |

### 2.2 기록 (E)

| 기능 | 판정 | 비고 |
|---|---|---|
| 노트: vault 폴더(WSL, 기본 `~/notes`, Git 동기화 권장), Markdown, 폴더 트리, 태그, 위키링크·백링크, 템플릿, 이미지 붙여넣기, Mermaid, 살균된 미리보기 | 유지 | `knowledge-vault-engine`, `markdown` |
| 자동 저장(1초 멈춤) + 복구 저널 + 외부 변경 비교 | 유지 | 문서 모델(§8.4) |
| 일일 기록 + "오늘 한 일" 요약 삽입(활동·커밋·닫힌 에이전트 작업·실행 결과). 요약은 PC 이름 소제목 아래에 넣는다 | 단순화 | 두 PC 동시 기록 충돌 방지 |
| 빠른 캡처: 전역 단축키 → 작은 창 → inbox | 유지 | |
| 노트에서 에이전트 작업 만들기(선택한 내용을 초기 지시문으로) | 신규 | S3 선택 과제 |
| 활동 기록: Windows 전면 창(앱·제목) 수집, 동의·일시중지, 개인정보 규칙(fail-closed), idle 세션화, 프로젝트 귀속, 일·주 보기, 내보내기 | 유지 | `activity-engine` |
| 통합 검색: 노트 + 프로젝트 코드 + 파일 이름, 저장된 검색, 제외 규칙. 진입점 둘(Ctrl+Shift+F 검색 탭, 편집기의 프로젝트 검색)이 같은 `search` crate를 쓴다 | 단순화 | `content-index-engine`, `search` |

### 2.3 편집 (D)

| 기능 | 판정 | 비고 |
|---|---|---|
| CodeMirror 6 편집기, 언어 하이라이트(`@codemirror/language-data`), 다크·라이트 테마, 파일 트리·탭·빠른 열기·찾기/바꾸기 | 유지·강화 | `packages/editor`, files |
| 파일 미리보기: Markdown·Mermaid·SVG(노트 렌더러 공유) | 유지 | `PreviewPane` |
| LSP: 진단·호버·정의 이동·서식·이름 바꾸기·자동완성. 데몬이 WSL에서 서버를 실행하고 메시지를 중계하며 진단을 엿봐 문제 패널로 보낸다 | 단순화 | editor-engine LSP 관리 30K줄 → 중계 + `@codemirror/lsp-client` |
| 언어 서버 설치: 감지 + 설치 명령을 터미널 세션에서 실행(sudo 비밀번호 입력 가능) | 단순화 | 관리형 설치기 폐기 |
| 저장: 원자적 쓰기, mode·EOL·BOM 보존, 외부 변경 감지, 초안 복구 | 유지 | |
| 외부 편집기로 열기(VS Code·Cursor `--remote wsl+<distro>`, Zed) | 신규 | |
| Git: 상태·hunk stage/discard·commit/amend·branch·stash·log·blame·fetch/pull/push·3-way 충돌 해결(외부 변경 감지)·worktree·PR·안전 정리. 에이전트 검토와 같은 diff·병합·충돌 컴포넌트를 쓴다 | 유지 | `repositories-engine` |
| Dependency Lens: 로컬 의존성 목록·중복 버전 + OSV 취약점 조회 | 단순화 | X-3 |

### 2.4 개발 도구 (F·G)

| 기능 | 판정 | 비고 |
|---|---|---|
| API 요청: HTTP·GraphQL·WebSocket·SSE·gRPC(reflection·proto)·MCP 클라이언트(stdio·HTTP, OAuth 2.1). 만드는 순서: HTTP·GraphQL·가져오기 → WS·SSE → gRPC·MCP·OAuth | 유지 | `http-client-engine`, `api-protocols` |
| 공통: 환경·변수·비밀 참조, 인증(Basic·Bearer·API key·OAuth2 client credentials·auth code + PKCE), 쿠키, TLS(클라이언트 인증서·CA·검증 끄기), multipart, 바이너리 응답, assertion·캡처(체이닝)·세션 변수·JSONPath, 컬렉션 실행기, 코드 생성, 응답 비교 | 유지 | AU-AS-01/02/07 수정본에서 이식 |
| 가져오기: cURL·Postman·Insomnia·Bruno·HAR·OpenAPI | 유지 | importers |
| 저장: 프로젝트 컬렉션은 `<프로젝트>/.devbox/api/`, 개인 컬렉션은 `~/.config/devbox/api/`(둘 다 파일, Git·dotfiles로 공유). 기록 본문은 파일, 색인은 DB | 단순화 | API Workspace 묶음 폐기(X-9) |
| API 응답을 노트에 붙이기 | 단순화 | 초안 보관함·전달 기록은 폐기 |
| 웹훅·모의 서버: 리스너(포트·LAN 허용), 실시간 수신 기록, 응답 규칙(경로·메서드·순서 응답), fixture 저장·재전송, chunked·100-continue·바이너리 | 유지 | `webhook-core` |
| 변환 도구 21종(JSON 4, 인코딩 7, 시간 1, 텍스트 3, 보안 3, 정규식 1, 차이 1, JWT 1) + 자동 감지·도구 제안 + 최근 사용 도구. 명령 팔레트에서 클립보드 값으로 실행, 편집기·로그·터미널 선택을 변환 도구로 보내기 | 유지 | `api-studio-tools.json`, transforms, `selection_send` |
| 개발 환경 점검: WSL·systemd·linger(권장)·WSL 유휴 종료 설정(`instanceIdleTimeout`·`vmIdleTimeout`)·tmux 버전·inotify 한도·잡아 둔 사용자 환경(PATH)·git·node·rust·docker·gh·claude·codex 버전과 인증 상태·hook 연결 상태. 관련 도구 설치(Windows: winget, WSL: 터미널 세션에서 명령 실행) | 유지 | `installation-tools` |

### 2.5 공통

- 명령 팔레트, 빠른 열기, 전역 단축키(§8.5)
- 트레이, 알림 센터(제목 표시줄)
- 설정, **앱 진단**(데몬 상태·로그·`devbox doctor` 결과·다시 설정·지원 번들. 진단 화면은 하나)
- 업데이트 확인

### 2.6 제외

v1에서 빼는 것이다. 2026-10-08 Q2에서 모두 빼기로 확정했다. 다시 필요해지면 v1 뒤에 별도 계획으로 다룬다.

| ID | 대상 | 빼는 이유 |
|---|---|---|
| X-1 | Office·PDF 문서 내용 색인 | 개발 허브의 검색은 노트·코드·파일 이름으로 충분 |
| X-2 | 변환 파이프라인 저장 목록 | 자동 감지·최근 사용 도구로 충분 |
| X-3 | Dependency Lens의 deps.dev 메타데이터 보강 | OSV 취약점 조회로 핵심 가치 충족 |
| X-4 | 개발 환경 구성 내보내기·적용(winget configuration) | 점검의 "없는 도구 + 설치"로 충분 |
| X-5 | OpenAPI → 모의 응답 규칙 초안 생성 | OpenAPI → 요청 생성은 유지 |
| X-6 | 터미널 입력 브로드캐스트 | 사용 빈도 낮음 |
| X-7 | 활동 요약 전달 이력·재생성 관리 | "일일 기록에 요약 삽입" 하나로 |
| X-8 | Windows 경로 프로젝트의 1급 지원(Windows Git·ConPTY·Windows LSP) | 사용 패턴상 거의 없음. Windows 대상 작업(§2.1)으로 대체 |
| X-9 | API Workspace 묶음 | 프로젝트·개인 컬렉션 파일로 대체 |
| X-10 | 로그 북마크·저장된 보기 | 검색·필터로 충분 |

### 2.7 폐기

- Suite 설치 세대·활성화·복구, 설치별 데이터 namespace, portable 배포
- 제품 간 연결·전달·수신 검토
- route 허용표, session·replay·provenance 검사
- 모든 이전 버전 데이터 가져오기
- Control Center 제품 화면, 브라우저 미리보기 전용 v0.7 화면
- 후보 승격·seal·fixture receipt 릴리스 절차
- 여러 WSL 배포판 동시 지원, 포트 관찰 타임라인

## 3. 시스템 구조

```
Windows 11                                        WSL2 (배포판 하나, systemd 켜짐)
┌─────────────────────────────┐                   ┌──────────────────────────────────────────┐
│ Devbox.exe (Tauri v2)       │                   │ devbox daemon   (devbox@<인스턴스>.service) │
│  ├ 창: 메인·분리 창·빠른 캡처 │                   │  ├ projects · git · agents · terminal     │
│  ├ 트레이·전역 단축키·알림    │ wsl.exe --exec    │  ├ runtime · logs · observe · lsp         │
│  ├ 활동 수집(전면 창)         │ devbox bridge     │  ├ notes · search · activity              │
│  ├ 외부 열기·업데이트         │◄──── stdio ──────►│  ├ api · webhooks · tools · secrets       │
│  └ 프레임 라우팅(crates/mux) │ (채널 헤더만 읽음) │  ├ devbox.db · index.db · 파일             │
└─────────────────────────────┘                   │  └ $XDG_RUNTIME_DIR/devbox-<인스턴스>.sock │
                                                  │                                          │
 개발 중: 브라우저 ◄─ WebSocket ─ devbox dev-gateway│ devbox exec (실행 래퍼, systemd-run 안)     │
          (127.0.0.1:1451, 토큰, 개발 전용)        │ tmux -L devbox-<인스턴스> (devbox-tmux@)    │
                                                  │ devbox mcp ◄─ Claude Code·Codex           │
                                                  │ devbox <CLI 명령>                         │
                                                  └──────────────────────────────────────────┘
```

| 구성 요소 | 책임 | 하지 않는 것 |
|---|---|---|
| `devbox daemon` | 모든 도메인 상태·데이터·프로세스, RPC·이벤트·스트림 | 화면 |
| `devbox bridge` | stdio ↔ unix socket 바이트 중계 | 메시지 해석 |
| `devbox exec` | 실행 래퍼: 자식 출력에 줄별 시각·스트림 표시, 로그 회전, 종료 코드를 파일과 데몬에 보고 | 상태 저장 |
| `devbox mcp` | MCP stdio 서버, 데몬 RPC 호출 | 자체 상태 |
| `devbox <명령>` | CLI(`run`, `agent new`, `agent event`, `note add`, `doctor`, `setup`, `secrets export-key` 등) | |
| `devbox dev-gateway` | 개발·E2E 전용 WebSocket ↔ unix socket | 배포 환경에서 실행 |
| `Devbox.exe` | 창·트레이·단축키·알림·활동 수집·외부 열기·업데이트, 데몬 배치·기동, 프레임 라우팅 | 도메인 로직, 데이터 저장 |

**경계 규칙**
- 도메인 로직은 Rust 도메인 crate에만 둔다. Tauri에 의존하지 않는다.
- Windows 앱의 Rust 코드는 Windows 기능과 프레임 라우팅만 한다. 라우팅 로직은 플랫폼 독립 crate `crates/mux`에 두고 Linux에서 시험한다.
- 화면은 데몬이 준 데이터를 보여 주고 의도를 RPC로 보낸다. 같은 판단을 화면과 데몬에서 두 번 하지 않는다.

**인스턴스.** 환경 변수 `DEVBOX_INSTANCE`(기본 `prod`. 개발은 `dev`, 테스트는 `test-<난수>`)가 다음을 함께 정한다.
- 소켓 `devbox-<i>.sock`
- tmux 서버 `-L devbox-<i>`
- systemd unit 인스턴스 `devbox@<i>`, `devbox-tmux@<i>`, 실행 unit 접두사 `devbox-<i>-run-`
- 데이터 경로 `~/.local/share/devbox/<i>/`

인스턴스가 다르면 서로의 세션·실행을 보지 않는다.

## 4. 데몬

### 4.1 기동과 수명

- **바이너리 배치:** Windows 앱이 `wsl.exe -d <distro> --exec /bin/sh -c 'cat > <tmp> && chmod 755 <tmp> && <sha256 확인> && mv -f <tmp> <home>/.local/share/devbox/bin/devbox-<버전>'`에 내장 바이너리를 표준 입력으로 보낸다(`\\wsl.localhost` 쓰기는 9p 경유라 실행 비트·중단 처리를 보장하지 못해 쓰지 않는다, PL-11). 그다음 새 바이너리로 `devbox setup`을 실행한다.
- `devbox setup [--instance <i>]`이 다음을 한다.
  1. `devbox` 심볼릭 링크를 새 버전으로 교체한다.
  2. **사용자 환경 잡기:** `bash -lc 'env -0'`으로 로그인 셸 환경(특히 `PATH`의 `~/.local/bin`·`~/.cargo/bin`·nvm 경로)을 잡아 `~/.config/devbox/<i>/env`(EnvironmentFile 형식)에 쓴다. systemd user manager의 기본 PATH에는 이 경로들이 없어서, 그대로면 실행·tmux에서 `pnpm`·`cargo`·`claude`·`codex`를 못 찾는다(PL-위험1, 실측). 사용자가 셸 설정을 바꾸면 앱 진단의 [환경 다시 잡기]로 갱신한다.
  3. 템플릿 unit을 `~/.config/systemd/user/`에 쓴다: `devbox@.service`, `devbox@.socket`, `devbox-tmux@.service`. 모두 `EnvironmentFile=%h/.config/devbox/%i/env`를 쓴다.
  4. `systemctl --user daemon-reload`를 실행하고, `devbox@<i>.socket`과 `devbox@<i>.service`를 `enable --now`한다.
  5. linger를 확인한다. WSL은 사용자 로그인 세션을 직접 만들어 user manager를 띄우므로 linger는 필수가 아니라 보험이다(PL-2). 꺼져 있으면 doctor가 "권장"으로 표시하고, 사용자가 누르면 Windows 앱이 `wsl.exe -d <distro> -u root loginctl enable-linger <user>`로 켠다(비밀번호 불필요, 실측).
- `XDG_RUNTIME_DIR`이 없으면 bridge·CLI는 `/run/user/$(id -u)`로 계산한다.
- **기동 시점:** enable로 데몬은 WSL user manager가 시작될 때 함께 뜬다. socket activation은 데몬이 죽었을 때의 보조 경로다.
  - WSL은 Windows 앱(로그인 자동 시작)이 브리지를 열 때 켜진다. 사용자가 터미널에서 WSL을 열어도 켜진다.
- **예약:** 데몬이 실행 중일 때만 동작한다. 놓친 예약은 예약마다 설정한 대로 "1회 보충" 또는 "건너뜀"(기본)으로 처리한다.
- `devbox@.service`: `Type=notify`(sd_notify READY), `Restart=on-failure`, `KillMode=mixed`. 데몬 자신만 종료되고, 실행·tmux는 별도 unit이라 유지된다.
- **WSL 유휴 종료 대응(PL-1):**
  - 배포판은 연결된 `wsl.exe` 클라이언트가 없으면 `instanceIdleTimeout`(기본 15초) 뒤, VM은 `vmIdleTimeout`(기본 60초) 뒤 멈춘다. systemd 서비스는 "사용 중"으로 치지 않는다.
  - 트레이 상주 Windows 앱이 브리지 연결을 계속 유지해 배포판이 멈추지 않게 한다. 앱이 비정상 종료하면 약 15초 뒤 배포판이 멈출 수 있다.
  - 앱을 완전히 종료할 때 실행 중인 에이전트·서비스가 있으면 "WSL이 멈추면 함께 종료됩니다"를 알리고 확인받는다.
  - 로그인 자동 시작은 기본 켬이다(첫 실행에서 묻는다).
  - 앱이 꺼져 있어도 세션을 유지하고 싶은 사용자를 위해 `.wslconfig`의 `instanceIdleTimeout=-1`을 앱 진단에서 선택 사항으로 안내한다(앱이 직접 바꾸지 않음).
  - WSL은 이 밖에도 Store 자동 업데이트, 사용자의 `wsl --shutdown`(VHDX 압축 등)으로 멈춘다. 다음 기동 때 끊긴 세션·실행을 "WSL 정지로 끊김"으로 표시한다(§9.2 `session_lost`).
- systemd가 꺼진 배포판은 지원하지 않는다. 첫 실행 화면이 켜는 방법을 안내한다(§6.5).

### 4.2 내부 구성

- `tokio` 멀티스레드 런타임 하나. musl 정적 빌드의 기본 할당자는 멀티스레드에서 느리므로 `mimalloc`을 쓴다.
- `Hub`가 도메인 서비스와 이벤트 버스를 가진다.
  - 이벤트 버스는 토픽별 `rev`를 붙여 발행한다.
  - 연결마다 제한 큐를 두고, 넘치면 해당 토픽을 `Resync{topic}` 하나로 합친다.
  - Hub는 어떤 연결 때문에도 기다리지 않는다.
- 각 도메인 crate는 `Service`(async 메서드), 발행 이벤트 타입, `method!`·`topic!` 선언과 `export` 함수(생성기용)를 제공한다.
- RPC 계층은 요청을 도메인 메서드에 연결만 한다. 권한·라우트 검사는 없다.
- 오래 걸리는 작업은 **진행 작업**(operation) ID를 돌려주고, 진행 이벤트로 알린다. 취소는 `CancellationToken`으로 한다.

### 4.3 프로세스·세션 관리

**실행(작업·서비스·예약 실행)**
- `systemd-run --user --unit devbox-<i>-run-<runId> --collect --property=... -- <home>/.local/share/devbox/bin/devbox exec --run <runId> -- <명령>` 형태로 띄운다.
- 속성: `WorkingDirectory`, `EnvironmentFile`(잡아 둔 사용자 환경) + `Environment`(작업 변수, 기본 `PYTHONUNBUFFERED=1`·`FORCE_COLOR=1`. TTY가 없어 버퍼링·색 꺼짐을 막는다), `KillMode=control-group`, `MemoryAccounting=yes`. CPU 사용량은 항상 제공되는 `CPUUsageNSec`로 읽는다(`CPUAccounting`은 systemd 259에서 무시됨, PL-3).
- `ExecStopPost=<devbox> run-exited --run <runId>`를 붙여 `$SERVICE_RESULT`·`$EXIT_CODE`·`$EXIT_STATUS`를 `.exit` 파일에 남긴다. 래퍼가 강제 종료돼 직접 기록하지 못해도 결과가 남는다. 정상 종료한 transient unit은 바로 정리되므로(기본 `CollectMode`) 결과는 파일로만 신뢰한다.
- 서비스는 `Restart=`(정의의 `restart`: `no`·`on-failure`·`always`), `RestartSec=1s`, `RestartSteps=5`, `RestartMaxDelaySec=30s`, `StartLimitBurst`, `StartLimitIntervalSec`를 붙인다. 빠르게 반복해 죽으면 간격이 1초에서 30초까지 늘어난다(systemd 254 이상, 이 PC는 259. IR-5). 재시작 주체는 systemd 하나뿐이고, 데몬은 표시만 한다.
- 재시작 원인은 프로세스 종료뿐이다. health 실패로 재시작하지 않는다(AR-F02). 준비 상태는 `observe`가 `ports_hint` 포트의 수신(`127.0.0.1`·`::1`·`0.0.0.0`·`::` 모두)을 보고 표시만 한다(§2.1).
- **`devbox exec` 래퍼:**
  - 자식의 stdout·stderr를 줄 단위로 읽어 `<시각>\t<o|e>\t<줄>` 형식으로 `runs/<runId>.log`에 쓴다.
  - 64MiB를 넘으면 앞부분을 잘라내고 표시를 남긴다.
  - 종료 코드를 `runs/<runId>.exit`에 쓰고, 데몬에 `runtime.exited`로 알린다. 데몬이 없으면 파일만 쓴다(`ExecStopPost`가 보충).
- **데몬 시작 시 맞춤:** 데몬이 시작할 때 `systemctl --user list-units 'devbox-<i>-run-*'`와 `.exit` 파일로 실행 상태를 맞춘다. 평소에는 래퍼 보고를 받으므로 폴링하지 않는다.

**터미널·에이전트 세션**
- tmux 서버는 setup이 쓰는 `devbox-tmux@<i>.service`로 띄워 데몬 재시작과 무관하게 유지한다. tmux가 `-D`(포그라운드 서버)를 지원하면 `Type=exec` + `tmux -D -L devbox-<i> -f <conf>`로, 아니면 `Type=forking`으로 띄운다(S0a에서 배포판 tmux 버전 확인). 서버의 전역 환경은 잡아 둔 사용자 환경으로 채운다(`set-environment -g`). 설정 파일은 devbox 소유이다.

```
set -g prefix None
unbind -a
set -g mouse off
set -g status off
set -ga terminal-overrides ',*:smcup@:rmcup@'
set -g window-size latest
set -g history-limit 50000
set -g allow-passthrough on
```

- 화면의 분할 칸 하나 = tmux 세션 하나이다. tmux 안의 분할은 쓰지 않는다.
- 에이전트 명령은 pane 안에서 `systemd-run --user --scope --unit devbox-<i>-agent-<id> -- <도구 명령>`으로 실행해 에이전트별 CPU·메모리를 잰다(cgroup v2, user manager가 cpu·memory·pids 위임, 실측). scope로 옮긴 프로세스는 tmux unit을 멈춰도 같이 죽지 않으므로, 에이전트 작업을 닫을 때 scope를 명시적으로 멈춘다.
- **화면 터미널:**
  - 데몬이 PTY(`portable-pty`)로 `tmux -L devbox-<i> attach -t <세션>` 클라이언트를 띄우고, 출력은 스트림(§5.1)으로 보낸다.
  - 입력도 스트림으로 받는다.
  - 다시 붙을 때는 `capture-pane -p -e -S -2000`으로 스크롤백을 먼저 채운다.
  - 격자 보기는 `attach -f read-only,ignore-size` 클라이언트를 원래 크기로 만들고 CSS로 축소한다.
- tmux 하한은 3.2(OSC 8 링크는 3.3)이다. doctor가 확인한다.
- 테스트(L2)는 인스턴스 접두사와 테스트별 slice(`devbox-test-<난수>.slice`)를 써서 끝나면 한 번에 정지한다. unit은 user manager의 HOME으로 실행되므로 임시 HOME에 기대지 않고 `HOME`·`XDG_*`를 명시해 넘긴다(PL-위험3).

**Windows에서 실행**
- `target = "windows"` 작업은 실행 전에 프로젝트를 `windows_dir`(예: `C:\dev\devbox`)로 동기화한다. 대상은 `git ls-files -co --exclude-standard` 결과다.
- 동기화한 뒤 `powershell.exe -NoProfile -Command`로 실행한다. 출력은 래퍼가 받는다.
- 종료는 interop 프로세스 종료이고, Windows 쪽 자식까지는 보장하지 않음을 화면에 표시한다.
- 데몬(systemd 서비스)에서 interop이 동작하는지는 S0a에서 확인한다(§14 R10).

### 4.4 저장소

| 파일 | 내용 | 백업·마이그레이션 |
|---|---|---|
| `devbox.db` | 프로젝트·에이전트 작업·실행 기록·예약·웹훅 규칙·활동·설정 중 DB 항목·암호화한 비밀 | 마이그레이션 전 `VACUUM INTO backups/devbox-pre-<버전>.db` 1개 보관 |
| `index.db` | 노트 FTS5·파일 이름 색인·API 기록 색인 같은 파생 데이터 | 백업·마이그레이션 없음. 스키마가 바뀌면 지우고 다시 만든다 |
| `runs/`, `api-history/`, `webhooks/` | 실행 로그, API·웹훅 본문 파일 | 보관 기한(기본 30일)으로 정리 |

- `rusqlite`(bundled), WAL. DB 파일마다 쓰기 전용 연결 1개(전용 스레드) + 읽기 연결 풀.
- 마이그레이션 버전은 도메인별로 둔다(`schema_versions(domain, version)`). 여러 worktree에서 병렬로 개발해도 번호가 충돌하지 않는다.
- 활동처럼 쓰기가 잦은 데이터는 1초 또는 100건 단위로 묶어 쓴다.
- 이벤트는 커밋 뒤에 발행하고, 그 커밋의 `rev`를 담는다.
- enum 컬럼은 공통 `ToSql/FromSql` 파생 매크로로 처리한다(문자열 수작업 parse 금지).
- 테이블 이름은 `<domain>_<name>`이다. 다른 도메인의 테이블은 그 도메인 서비스를 통해서만 접근한다.

### 4.5 로그와 진단

- `tracing` + `tracing-appender`(일 단위 회전, 7일): `~/.local/state/devbox/<i>/logs/daemon.log`.
- 기록하는 것: 메서드·소요 시간·오류 코드·진행 작업 ID. 인자·본문·비밀·파일 내용은 기록하지 않는다.
- panic hook: 버전·스레드·위치·최근 이벤트 50개를 `crash-<시각>.log`로 남긴다.
- 릴리스 빌드는 `debug = "line-tables-only"`로 한다.
- `devbox doctor`와 앱 진단 화면은 같은 점검 목록을 쓴다.

### 4.6 업데이트·재시작

1. Windows 앱이 새 바이너리를 배치하고 `systemctl --user restart devbox@<i>.service`를 실행한다.
2. 실행 unit·tmux 세션은 유지된다.
3. 재시작 중 요청은 `daemon_restarting`(적용 안 됨)을 받는다. 연결이 응답 전에 끊기면 클라이언트는 `connection_lost`(결과 불명)를 만든다.
4. 화면은 query만 자동 재시도한다. mutation은 "결과 확인 필요"로 표시하고 상태를 다시 읽는다.

## 5. 통신

### 5.1 전송·프레임·흐름 제어

- 데몬은 unix socket(`$XDG_RUNTIME_DIR/devbox-<i>.sock`, 0600)만 연다. TCP 예외는 다음 셋뿐이다.
  - 사용자가 켠 웹훅 리스너
  - OAuth PKCE 콜백 리스너(127.0.0.1, 인증 진행 중에만)
  - 개발 전용 dev-gateway
  - (R3 대체 경로를 쓰게 되면 그것도 예외에 추가한다.)
- **프레임:** `u32 LE 길이`(뒤따르는 바이트 수, 0 금지) + `u8 종류` + `u32 채널` + 본문.
  - 종류 0 JSON 메시지: 평소 1MiB 이하, 하드 상한 16MiB. 큰 결과는 페이지나 스트림으로 보낸다.
  - 종류 1 스트림 조각: `u32 스트림 ID` + `u8 플래그(FIN)` + 최대 64KiB.
  - 종류 2 제어: `ChannelOpen`, `ChannelClose`, `StreamCredit{stream, bytes}`.
  - 잘못된 프레임을 받으면 그 연결을 끊는다.
- **채널:**
  - 창(또는 WebSocket·CLI 연결) 하나가 채널 하나다.
  - Tauri는 프레임 헤더의 채널 번호만 보고 창으로 보낸다. JSON은 해석하지 않는다.
  - 창이 닫히거나 새로 고쳐지면 `ChannelClose`를 보낸다. 데몬은 그 채널의 구독·스트림·문서 lease를 일괄 정리한다.
- **흐름 제어:**
  - 스트림마다 신용 창(256KiB)을 둔다. 화면은 실제로 소비한 뒤(xterm `write` 콜백 등) `StreamCredit`를 보낸다.
  - 신용이 바닥나면 생산자가 멈춘다. 터미널은 PTY 읽기를 멈추고(tmux가 다시 그림), 로그 tail은 파일 오프셋에서 기다린다. SSE·WebSocket은 상한까지 쌓은 뒤 버리고 `dropped n`을 표시한다. LSP는 그 세션을 끊는다.
  - 데몬 송신기는 채널별 큐를 라운드로빈으로 돌리고, JSON 응답을 스트림보다 먼저 보낸다.
- **메시지(JSON):**
  - `Hello{version, client, instance}` → `Welcome{version, daemonId}`
  - `Request{id, method, params, clientRequestId?}` → `Response{id, result}` 또는 `Response{id, error, diagnosticId?}`
  - `Cancel{id}`
  - `Subscribe{id, topic, params}` → 응답 `{subId, rev}`, 이후 `Event{subId, rev, payload}` / `Resync{subId}` / `Unsubscribe{subId}`
  - 스트림은 `kind: stream` 메서드의 응답으로 `{streamId}`를 받아 연다. 데이터는 종류 1 프레임, 끝은 FIN으로 표시한다.
- **버전:** 앱·데몬·CLI는 같은 빌드에서 나온다. `Hello` 버전이 다르면 앱이 데몬을 다시 배치한다. 호환 협상은 하지 않는다.

### 5.2 타입 계약과 생성

- `crates/protocol`의 `method!`·`topic!`(macro_rules)로 메서드와 구독 주제를 하나씩 선언한다. 문자열 이름, 종류, 확인 정책, 파라미터·결과·오류 타입을 함께 적는다. 도메인 오류는 `domain_error!`(§5.3)로 만든다. 정의는 03 Task 4의 코드가 원장이다.

```rust
method!(pub AgentsCreate = "agents.create", mutation, CreateAgentTask => AgentTask, AgentsError);
method!(pub AgentsList = "agents.list", query, ListAgentTasks => AgentTaskList, AgentsError);
method!(pub AgentsDiscard = "agents.discard", mutation(always), DiscardAgentTask => AgentTask, AgentsError);
topic!(pub AgentsChanged = "agents.changed", AgentChanged);
```

- **종류:** `query`, `mutation`, `subscription`, `stream`.
- **확인 정책 `confirm`:** `none`, `always`, `reversible`(되돌리기 알림). 화면은 이 값으로 확인 대화상자나 되돌리기 알림을 자동으로 붙인다.
- **본 것과 실행할 것의 결속(IR-11, AR-F01):** `confirm = always` 메서드의 파라미터는 확인 대화상자가 보여 준 대상 상태의 `expect`를 담는다(예: 병합·push·pull은 `{ branch, headOid, upstreamOid }`, 폐기는 `{ headOid, unmergedCount }`, 프로세스 종료는 `{ pid, startTimeMs }`). 데몬은 실행 직전 현재 상태와 비교해 다르면 실행하지 않고 `<domain>.state_changed`(세부 = 지금 상태, 예: `{ head, base, unmerged }`)를 돌려준다. 화면은 새 상태로 확인을 다시 받는다. 외부 프로그램과의 완전한 원자성을 주장하지 않는다. 확인 이후 바뀐 상태를 실행하지 않는 것이 목적이다.
- **매크로가 만드는 것:**
  - 메서드마다 `impl Method { const NAME; const KIND; const CONFIRM; type Params; type Output; type Error; }`, 주제마다 `impl Topic { const NAME; type Payload; }`
  - 데몬은 `Router::add::<M, _, _>(handler)`로 메서드를 등록하고, `Router::names()`로 등록 목록을 낸다. 선언했는데 등록하지 않은 메서드는 데몬의 `every_declared_method_is_routed` 시험이 잡는다(03 Task 13).
- **생성기 `cargo run -p xtask -- gen-ts [--check]`:**
  - `ts-rs` 12와 v0.9.0 `TypeExporter` 방식을 쓴다. 설정은 `Config::new().with_large_int("number")`.
  - 결과는 `app/src/rpc/gen/rpc.ts` 한 파일이다: 모든 타입 선언, `interface Methods { "agents.create": { kind; confirm; params; result; error } }`, `interface Topics`, `METHOD_INFO`, `RPC_ERROR_CODES`, `type RpcErrorCode`. 같은 이름에 다른 정의가 있으면 실패한다. 손으로 고치지 않는다.
- **타입 규칙:**
  - 응답 결과는 실제 타입으로 직렬화한다(`serde_json::Value`를 경계 타입으로 쓰지 않음).
  - 2^53을 넘을 수 있는 정수와 ID·해시는 문자열로 보낸다.
  - 시간은 `*_ms: i64`로 보낸다(ts-rs에는 `Duration` 구현이 없음).
- CI는 `gen-ts --check`와 `tsc`를 돌린다.

### 5.3 오류와 결과

- 도메인 오류는 `domain_error!` 매크로로 만든다.
  - `#[derive(thiserror::Error, Serialize, TS)] #[serde(tag = "code", content = "detail")]`
  - 변형마다 `rename = "<domain>.<code>"`
  - `const CODES`
  - 오류는 생기는 곳에서 이 enum으로 만든다. 세부 필드 이름은 한 단어 소문자다(TS와 그대로 맞추기 위해).
  - **`Result<T, String>` 허용 범위:** RPC 응답 오류와 도메인 crate가 다른 crate에 내보내는 함수는 이 enum(또는 crate 오류 타입)을 쓴다. `Result<T, String>`은 ① crate 안에서 바로 도메인 오류나 응답의 `message` 세부 필드로 바꾸는 helper, ② `crates/cli`의 setup·doctor 단계 결과, ③ Windows 앱(Tauri 명령 반환 포함)에서만 쓴다. 리뷰로 확인한다.
- 내부 원인(io 오류 문장 등)은 로그에만 남긴다. 응답에는 `Response.diagnosticId`만 붙인다.
- 공통 오류:
  - `internal`, `invalid_params`, `not_found`, `cancelled`
  - `daemon_restarting`(적용 안 됨)
  - `secrets_locked`, `secrets.key_mismatch`
  - 클라이언트 전용 `connection_lost`(결과 불명)
- **커밋 뒤 부분 실패:** Err로 돌려주지 않는다. `Ok({ value, warnings: Warning[] })`로 돌려준다(예: 저장은 됐고 색인은 미뤄짐).
- **잘린 결과:** 상한에 걸려 잘렸으면 `truncated: { reason }`을 붙인다(ADR 0005 유지).
- **멱등성:** 생성형 mutation(`agents.create`, `runtime.start` 등)은 `clientRequestId`로 멱등 처리한다. 데몬은 10분간 결과를 기억한다. 재시작 뒤에는 기억이 없으므로 화면은 "결과 확인 필요" 후 상태를 다시 읽는다.
- **문구:** `app/src/rpc/messages.ts`는 `{ [C in RpcErrorCode]: Message | ((detail) => Message) }` 객체 리터럴이다. 빠진 키와 남는 키가 모두 컴파일 오류가 된다.

### 5.4 브리지와 Tauri 연결

- Windows 앱은 `wsl.exe -d <distro> --exec <home>/.local/share/devbox/bin/devbox bridge --instance <i>`를 `CREATE_NO_WINDOW`로 띄운다. 앱 프로세스당 하나이고, 모든 창이 공유한다.
  - 실측 성능: 64B 왕복 p50 0.16ms·p99 0.25ms, 처리량 77–93 MiB/s, 8MiB 무작위 바이트 왕복 일치(PL-8).
  - 브리지는 시작하자마자 고정 표지 바이트열(`DEVBOX-BRIDGE/1\n`)을 먼저 보낸다. 앱은 이 표지가 없으면 출력을 `wsl.exe` 자체 오류(UTF-16LE 현지화 문장)로 보고 오류 코드(`WSL_E_*`)로 판정한다(PL-위험4).
  - `wsl.exe` 자식은 Job Object(`KILL_ON_JOB_CLOSE`)에 넣어 앱이 죽어도 정리되게 한다.
- `crates/mux`(플랫폼 독립):
  - 프레임 경계·헤더 읽기, 창 ↔ 채널 배정, 재연결(0.2·0.5·1·2·5초), 연결 상태 이벤트를 맡는다.
  - Tauri는 창마다 `ipc::Channel`을 만들어 mux에 등록하기만 한다.
  - 새 메서드를 추가해도 Tauri 코드는 바뀌지 않는다.
- 끊긴 동안 화면은 마지막 데이터를 "마지막 갱신 hh:mm"과 함께 유지한다. 문서 편집은 계속할 수 있다(§8.4).

### 5.5 화면 쪽 상태 갱신 규칙

- 서버 상태는 TanStack Query로 관리한다. 화면 코드는 생성된 `Methods`·`Topics` 타입을 쓰는 공용 훅 `useRpcQuery(method, params)`·`useRpcMutation(method)`·`useTopicInvalidation(topic, keys)`·`confirmPolicy(method)`(03b Task 18)로만 서버 데이터를 다룬다. 메서드별 훅은 생성하지 않는다.
- **rev 규칙:**
  - R4: 조회 결과와 `*.changed` 이벤트는 `rev`를 담는다. DB 상태는 커밋 순번, 메모리 상태는 데몬 카운터다. `daemonId`가 바뀌면 캐시 전체를 버린다.
  - R5: 캐시의 rev가 이벤트 rev 이상이면 이벤트를 무시한다. mutation 응답으로 캐시를 쓸 때(`setQueryData`)는 응답 rev가 캐시 rev보다 클 때만 쓴다. 응답과 이벤트의 도착 순서에 의존하지 않는다.
  - R6: 재연결 순서는 `Welcome` → 구독 재등록 응답 대기 → 활성 쿼리 전부 다시 조회다.
  - R7: 데몬은 커밋 뒤에만 이벤트를 낸다.
  - R8: 관찰 시각(`observedMs`·`lastSyncedMs` 등)만 바뀐 상태는 rev를 올리지 않고 `*.changed`도 내지 않는다. 관찰 시각은 상태 표시용 별도 필드로만 보낸다(CV-F07).
- 폴링은 기본적으로 쓰지 않는다. 이벤트 원천이 없는 관찰 대상(포트·프로세스·WSL 자원)만, 화면이 보일 때 2–5초 간격으로 조회한다.

### 5.6 dev-gateway

- `devbox dev-gateway --instance dev --port 1451`: 127.0.0.1에만 연다.
- 시작할 때 무작위 토큰을 출력하고, `Origin`이 `http://localhost:1450`(화면 개발 서버, strictPort)일 때만 받는다.
- WebSocket 하나 = 채널 하나로 프레임을 중계한다.
- 개발과 L3 E2E에서만 쓴다.

## 6. Windows 앱

### 6.1 책임

| 기능 | 구현 |
|---|---|
| 식별자·폴더 | identifier `io.github.jihoon22lee.devbox`(v0.9.0의 `com.devbox.v08.*`와 겹치지 않음). 설치는 NSIS 사용자 단위 기본 폴더, 앱 데이터는 `%LOCALAPPDATA%\io.github.jihoon22lee.devbox\` |
| 창 | 메인 창 + 분리 창(터미널·에이전트 상세·에이전트 격자·노트·요청) + 빠른 캡처 창 + 명령 팔레트 오버레이. 기본 크기는 작업 영역의 80%(최대 1600×1000, 최소 1180×760), 최소 960×600 |
| WebView2 | 브라우저 가속키를 끈다(`AreBrowserAcceleratorKeysEnabled=false`: Ctrl+P 인쇄, F5 새로고침, Ctrl+± 확대 차단). 확대는 앱 UI 배율로 처리 |
| 트레이 | 열기·빠른 캡처·활동 일시중지·실행 요약(에이전트 n, 서비스 n)·종료. 처음 창을 닫으면 "트레이에서 계속 실행됩니다"라고 알림 |
| 전역 단축키 | `tauri-plugin-global-shortcut`. §8.5. 등록에 실패하면 트레이·설정에 표시 |
| 알림 | Tauri 알림 플러그인은 데스크톱에서 클릭 이벤트를 주지 않는다(PL-7). 그래서 `crates/win`이 WinRT `ToastNotification`을 직접 띄우고, 클릭은 protocol activation(`devbox://agents/<id>`) + `tauri-plugin-deep-link` + single-instance로 기존 창의 해당 화면을 연다. 같은 작업은 tag로 교체, 동시에 여러 개는 묶음, [열기] 버튼. 작업 표시줄 오버레이 배지(입력 대기 수)와 `FlashWindowEx`(방해 금지 모드 대비). Windows 토스트는 설치본(AUMID 등록)에서만 동작하므로, 개발 실행에서는 배지·깜빡임·앱 안 알림 센터로 확인한다 |
| 자동 시작 | `tauri-plugin-autostart`. 첫 실행에서 묻고 기본 켬 |
| 단일 인스턴스 | `tauri-plugin-single-instance`. 두 번째 실행은 기존 창을 활성화 |
| 활동 수집 | `crates/win`: `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` + 제목 변경 + `GetLastInputInfo`. 개인정보 규칙을 Windows 쪽에서 먼저 적용한 뒤 보낸다(§6.2). 연결이 끊긴 동안은 앱 메모리에 최대 1,000건 보관하고, 넘치면 버리고 버린 수를 표시 |
| 외부 열기 | VS Code/Cursor(`code --remote wsl+<distro> <path>`), 탐색기(`\\wsl.localhost\<distro>\...`), Windows Terminal(`wt.exe -p <profile> -d <path>`), 브라우저 |
| 파일 선택 | 화면 안 WSL 폴더 선택기(데몬 목록 조회). 필요하면 Windows 대화상자로 `\\wsl.localhost` 경로를 받아 변환 |
| 업데이트 | §10. 진행 상태는 앱이 소유하고 모든 창에 알린다 |
| 첫 실행·복구 | §6.5 |

### 6.2 활동 수집의 개인정보 규칙

- 규칙(프로세스 제외·제목 마스킹/치환·정규식)은 데몬 설정에 저장한다. 앱은 연결할 때 받아 캐시한다.
- 규칙을 받지 못했거나 정규식 컴파일에 실패하면 **제목을 보내지 않는다**(fail-closed). 앱 이름과 시간만 보낸다.
- 일시중지 중에는 보내지 않는다. 동의 전에는 수집기를 시작하지 않는다. 동의를 철회하면 대기열을 비운다.
- v0.9.0 Knowledge의 활동 수집이 켜져 있으면, 첫 실행에서 한쪽만 켜도록 안내한다.

### 6.3 비밀

- 비밀 값은 `devbox.db`에 XChaCha20-Poly1305로 암호화해 저장한다. AAD는 `secret_id | key_id`.
- **데이터 키 생성:** 32바이트 무작위. 첫 설정 때 만든다.
  - DPAPI(CurrentUser)로 봉인한 blob을 `~/.local/share/devbox/<i>/keys/data.key.dpapi`에 둔다.
  - 지문 `key_id`를 DB에 기록한다.
- **키 사용:**
  - 데몬이 시작할 때 interop으로 `powershell.exe -NoProfile -NonInteractive`를 띄워 blob을 표준 입출력으로 풀고, 키는 메모리에만 둔다.
  - interop이 안 되는 환경(S0a 확인 결과에 따름)에서는 Windows 앱이 연결 직후 대신 풀어 `secrets.unlock`으로 전달한다. 그 전까지 비밀이 필요한 동작은 `secrets_locked`를 반환한다.
- **복구:**
  - 첫 설정 때 `devbox secrets export-key`로 복구 문자열을 보여 주고 보관을 권한다.
  - 키를 풀 수 없거나(`key_id` 불일치) DPAPI가 실패하면 `secrets.key_mismatch`를 한 번 알리고, "복구 문자열 입력" 또는 "비밀 다시 입력"을 제공한다.
  - 이런 경우는 다른 PC로 WSL을 옮기거나, Windows를 재설치하거나, 계정 비밀번호가 재설정된 경우다.
- **비밀이 나가지 않는 곳:** 파일(`.devbox/`·`~/.config/devbox/`)·로그·프로세스 인자·MCP 응답에 평문으로 나가지 않는다.
  - 실행 시에는 래퍼가 환경 변수로만 주입한다.
  - 파일에는 `{{secret:<이름>}}` 참조만 둔다.
  - 비밀 이름 목록은 설정 파일로 두 PC가 공유하고, 값이 없는 PC에서는 "이 PC에 값 없음 · 입력"을 표시한다.
- DPAPI 봉인으로 확정했다(2026-10-08 Q1). 0600 키 파일 방식은 만들지 않는다.

### 6.4 WebView

- CSP: `default-src 'self'`.
- Markdown은 데몬에서 `pulldown-cmark` + `ammonia`로 살균한 HTML만 받는다.
- Mermaid는 `securityLevel: "strict"`로 렌더한다. 원격 이미지는 막는다.

### 6.5 첫 실행과 장애 상태 화면

| 상태 | 화면 | 사용자 동작 |
|---|---|---|
| WSL 미설치 | "WSL이 필요합니다" + 설명 | [WSL 설치](관리자 권한 `wsl --install`) → 재부팅 → 자동 시작으로 이어서 진행 |
| 배포판 여러 개 | 배포판 선택(기본 배포판 미리 선택). 도구 내부용 배포판(`docker-desktop*`·`rancher-desktop*`·`podman-machine*`)은 세지도 보이지도 않는다. 이 PC처럼 Ubuntu + docker-desktop이면 고르기 없이 Ubuntu | 선택 |
| WSL1 배포판 | "WSL2만 지원" + 변환 명령 안내 | — |
| systemd 꺼짐 | 설명 + [자동으로 켜기] | `/etc/wsl.conf` 수정 후 `wsl --shutdown`. 이때 실행 중인 모든 WSL 프로그램이 종료된다는 확인을 받음 |
| tmux 없음·3.2 미만 | 설치 명령 안내 | [터미널에서 설치] |
| linger·setup 실패 | 실패 단계와 로그 열기 | [다시 시도][로그 열기][진단] |
| 데몬 버전 재배치 중 | 진행 표시 | — |
| 데몬 끊김 | 상단 배너 "다시 연결 중". 터미널 위에는 "입력 전송 안 됨"을 덮어 표시. 5회 실패하면 버튼 표시 | [다시 시도][WSL 재시작][진단] |
| 업데이트 설치 중 | 진행 창 | 실패하면 이전 버전 실행 안내 |
| 비밀 키 불일치 | §6.3 | [복구 문자열 입력][비밀 다시 입력] |
| hook 미연결 | 에이전트 목록 상단 배너 "상태 알림이 연결되지 않았습니다" | [연결 확인](doctor의 hook 점검) |

## 7. 데이터와 설정

| 위치 | 내용 | 공유 |
|---|---|---|
| `~/.local/share/devbox/<i>/devbox.db`, `index.db` | §4.4 | PC별 |
| `~/.local/share/devbox/<i>/runs/`, `api-history/`, `webhooks/`, `backups/`, `keys/` | 실행 로그, 본문, 백업, 봉인 키 | PC별 |
| `~/.local/state/devbox/<i>/logs/` | 데몬 로그 | PC별 |
| `~/.config/devbox/config.toml` | 공유할 설정: 단축키, 터미널·에이전트 도구 프로필, 비밀 이름 목록, 개인정보 규칙 | dotfiles로 공유 |
| `~/.config/devbox/config.local.toml` | PC별 설정: 배포판, Windows 사본 폴더, PC 이름 | PC별 |
| `~/.config/devbox/api/` | 개인 API 컬렉션·환경(비밀은 참조만) | dotfiles로 공유 |
| `<프로젝트>/.devbox/devbox.toml` | 작업·서비스·예약·시작 구성·에이전트 준비 | Git |
| `<프로젝트>/.devbox/api/`, `.devbox/prompts/` | 프로젝트 API 컬렉션, 지시문 템플릿 | Git |
| `~/notes`(설정 가능) | 노트 vault | Git |
| `%LOCALAPPDATA%\io.github.jihoon22lee.devbox\` | 창 상태, 앱 로그(7일) | PC별 |

- 설정은 파일이 원장이다. 화면에서 바꾸면 해당 파일에 쓴다. DB에는 화면 상태처럼 파일로 둘 필요가 없는 것만 둔다.
- 앱에서 작업을 만들면 "프로젝트 파일에 저장"(기본)과 "이 PC에만 저장" 중 고른다. 같은 ID가 둘 다에 있으면 프로젝트 파일이 우선하고, 화면에 출처를 표시한다.

`.devbox/devbox.toml` 예:

```toml
[[task]]
id = "test"
command = "pnpm test"

[[task]]
id = "win-build"
target = "windows"
command = "pnpm tauri build"     # windows_dir은 config.local.toml의 [windows] 사본 루트 + 프로젝트 이름

[[service]]
id = "web"
command = "pnpm dev --port $DEVBOX_PORT"
restart = "on-failure"

[[schedule]]
task = "test"
cron = "0 9 * * 1-5"
missed = "skip"                   # 또는 "run-once"

[agent]
copy = [".env", ".env.local"]
setup = ["pnpm install --frozen-lockfile"]
teardown = []

[launch]
services = ["web"]
terminals = [{ name = "shell" }]
agents = [{ tool = "claude", worktree = false }]
```

## 8. 화면

### 8.1 프레임과 범위

```
┌──────────────────────────────────────────────────────────────────────────┐
│ ▣ devbox   [devbox ▾ · main]           ⌕ 명령·검색 (Ctrl+Shift+P)   🔔2  ●  │ 제목 표시줄 36px
├────┬─────────────────────┬───────────────────────────────────────────────┤
│ 에 │ (섹션별 목록)         │                                               │
│ 터 │                     │              본문 (분할 가능)                  │
│ 실 │                     │                                               │
│ 소 │                     ├───────────────────────────────────────────────┤
│ 노 │                     │ 터미널 │ 로그 │ 문제 3          하단 패널(접기)  │
│ 도 │                     │                                               │
│ ⚙  │                     │                                               │
├────┴─────────────────────┴───────────────────────────────────────────────┤
│ 데몬 ● · 에이전트 3(입력 대기 1) · 실행 2 · 포트 5173,8080 · WSL 6.1/20GB    │ 상태 표시줄 24px
└──────────────────────────────────────────────────────────────────────────┘
```

- **활동 막대(48px):** 에이전트·터미널·실행·소스·노트·도구(Ctrl+1–6), 아래에 설정.
  - 아이콘 + 툴팁 + 배지(입력 대기·실패·변경 수).
  - 시작 화면은 마지막으로 쓴 섹션이고, 처음 실행할 때는 에이전트다.
- **목록 영역:** 기본 260px, 크기 조절, Ctrl+Shift+B로 접기.
- **하단 패널:** 터미널·로그(출처 선택·고정)·문제 세 탭. Ctrl+\`로 토글. 소스·노트 섹션에서는 기본으로 닫혀 있다.
- **상태 표시줄:** 데몬 연결, 에이전트·실행 요약, 포트, WSL 메모리. 클릭하면 해당 화면.
- **제목 표시줄:**
  - 프로젝트 전환기(Ctrl+Shift+O)를 연다. 전환기는 프로젝트 카드(브랜치·변경 수·실행 중 서비스·포트·에이전트·최근 노트)와 [시작 구성 실행]을 보여 준다.
  - 명령 팔레트 입구, 알림 센터(🔔), 데몬 연결 점(●, 초록 연결·노랑 재연결 중·빨강 끊김)이 있다.
  - 배포판 이름은 툴팁으로 보여 준다.
- **뒤로·앞으로:** Alt+←/→, 마우스 4·5번 버튼.
- **분리 창:** 탭을 끌거나 "새 창으로"를 누르면 별도 창으로 연다.
- **창 크기에 따른 접기:** 1100px 미만이면 목록 영역, 760px 높이 미만이면 하단 패널을 자동으로 접는다. 본문은 기본 창에서 화면의 65% 이상을 가진다.
- **범위 규칙:**
  - 에이전트·알림·상태 표시줄은 모든 프로젝트를 보여 주고, 필터 칩으로 좁힌다.
  - 터미널·실행·소스·노트는 현재 프로젝트만 보여 준다.
  - 에이전트 검토는 전역 프로젝트 맥락을 바꾸지 않고 상세 안에서 한다.

### 8.2 섹션별 화면

| 섹션 | 배치 | 핵심 동작 |
|---|---|---|
| 에이전트 | [목록 \| 격자] 전환. 목록: 긴급순 그룹(입력 대기·실패·검토 대기·실행 중·준비 중·닫힘 24시간), 행에 hook 메시지 한 줄. 상세: 머리줄(작업·프로젝트·브랜치·도구·상태·경과·CPU·메모리·토큰·뒤처짐) + 동작 버튼 + 탭(터미널·변경·커밋·실행) + 지시 작성란(Enter 줄바꿈, Ctrl+Enter 보내기) | 새 작업(Ctrl+Shift+N), 다음 입력 대기로(Ctrl+Shift+J), 이어서 지시, 테스트 실행, 기준 반영, 병합▾·PR·폐기, 재개, 새 창 |
| 터미널 | 목록: 프로젝트별 세션. 본문: 탭 + 분할(칸마다 tmux 세션) | 새 세션, 분할, 분리 창, 검색, 선택을 변환 도구로 |
| 실행 | 목록: 서비스·작업·예약·컨테이너 그룹(상태 점). 본문: 선택한 실행의 로그(가상 스크롤, 시각·스트림 열) + 위쪽 포트·자원 칩 | 시작·중지·재시작, 로그 검색·필터·소스 병합, 포트 열기·점유 종료, 문제 이동 |
| 소스 | 목록 위 [파일 \| 변경 \| 브랜치] 전환. 본문: 편집기 탭·diff 탭·충돌 해결 | 파일 편집, stage/commit/push, 충돌 해결, 정리, 미리보기 |
| 노트 | 목록: 폴더 트리 + 태그 + 활동(일·주). 본문: 편집기(미리보기 분할), 오른쪽 백링크(접기) | 새 노트, 일일 기록(Ctrl+Alt+D), 빠른 캡처, 노트에서 에이전트 작업 |
| 도구 | 목록: API 컬렉션 트리 · 웹훅 · 변환 도구 · 개발 환경 점검. 본문: 요청 \| 응답 2분할(1100px 미만이면 응답이 아래로), 요청 종류에 맞는 패널만 | 보내기(Ctrl+Enter), 저장, 코드 생성, 가져오기, 응답을 노트에 |
| 검색(본문 탭) | Ctrl+Shift+F로 열리는 통합 검색 탭(노트·코드·파일 이름) | 결과 열기, 저장된 검색 |
| 설정 | 일반·단축키·에이전트(도구 프로필·hook)·실행·노트·활동·비밀·앱 진단·정보 | 다시 설정, 지원 번들, 업데이트 |

### 8.3 디자인 시스템 (`app/src/ui`)

**토큰(CSS 변수, 의미 기반, 다크·라이트 두 벌, 시스템 설정 따르기 옵션)**
- 배경 4단계, 표면, 경계 2단계, 글자 3단계
- 강조, 상태(성공·경고·위험·정보·진행), 선택·포커스 링, 그림자 2단계
- 행 높이(24·28·32), 컨트롤 높이(24·28·32)

**크기·글꼴·아이콘**
- 간격은 4px 격자다.
- 글자 크기는 12·13·14·16·20·24이고, 본문은 13px이다.
- 모서리는 4·6·8이다.
- 글꼴은 Pretendard Variable(OFL, 포함)이다.
- 코드 글꼴은 Cascadia Mono → D2Coding(OFL, 포함)이다. 한글 셀 폭을 맞추기 위해서다. xterm은 `addon-unicode11`을 쓴다.
- 아이콘은 lucide-react다. 상태는 색 + 모양(아이콘)으로 구분한다.

**컴포넌트:** Radix Primitives 위에 CSS Modules로 스타일을 입힌다. 필요할 때 만들고, S1에서 쓰는 것부터 만든다.
- 기본: Button·IconButton·Toolbar(넘침 메뉴)·Tabs·SplitPane·Tree·VirtualList·DataTable(가상 스크롤·고정 헤더)
- 대화·알림: Dialog·ConfirmDialog·Popover·Menu·ContextMenu·Tooltip·Toast(되돌리기)·Banner·NotificationCenter
- 표시: Badge·StatusDot·Kbd·EmptyState·ErrorState·Skeleton·Progress·PathLabel·Timeline
- 입력: Field·TextArea(자동 높이)·Select·Combobox·Checkbox·Radio·Switch·SegmentedControl·KeyValueEditor
- 복합: CommandPalette·DiffView(공용)·TerminalView(연결 상태 덮개 포함)

**상태 표시:** loading(300ms 넘을 때만 스켈레톤)·empty(주 동작 하나)·error(제목 + 다음 행동)·stale(마지막 갱신 시각)·disconnected·forced-colors 대응·reduced-motion 존중.

**편집기·터미널 테마:** CodeMirror·xterm·diff 색을 같은 토큰에서 만든다. 테마 모듈은 하나다.

### 8.4 상태 관리와 문서 모델

- **서버 데이터:** TanStack Query(§5.5).
- **화면 상태:** 섹션별 Zustand store.
- **라우팅:** TanStack Router(타입 있는 경로). 분리 창은 같은 경로를 별도 창에서 연다.
- **문서(노트·코드 파일):** 공통 `DocumentStore`가 소유한다.
  - **편집 lease:** 같은 문서는 데몬의 편집 lease를 가진 한 창에서만 편집한다(`documents.claim{path} → {leaseId, rev, text, journal?}`). 다른 창은 읽기 전용으로 보여 주고 [여기로 가져오기]를 제공한다. 저장과 저널 쓰기는 `leaseId`와 `baseRev`로 CAS한다.
  - **문서 필드:**
    - `key{rootId, realRelPath}`, `leaseId`, `diskRev{hash, size, mtimeNs, ino}`
    - `editSeq`, `savedSeq`, `inflight{seq, baseRev}`, `journalSeq`
    - 문서별 `EditorState`(undo 포함), `policy`
  - **undo:** 문서를 바꿀 때 `view.setState()`로 그 문서의 `EditorState`로 전환한다. 텍스트만 교체하지 않는다(AU-K1).
  - **`DocumentPolicy`:**
    - 저장: 노트는 1초 자동, 코드는 수동(설정 가능)
    - 저장 뒤: 노트는 링크 색인, 코드는 LSP `didSave`
    - rename: 노트는 링크 재작성, 코드는 이동만
    - 크기 상한: 종류별로 다름
  - **연결이 끊겼을 때:** 편집은 계속한다. 저널은 화면 쪽 임시 저장소(IndexedDB, 문서당 최신 1개)에 두고, 재연결하면 같은 `baseRev`로 저장한다.

**DocumentStore 상태 기계**

| 상태 | 이벤트 | 동작 | 다음 상태 |
|---|---|---|---|
| Closed | open | `documents.claim` | Loading |
| Loading | 저널 없음 | 디스크 내용 로드 | Clean |
| Loading | 저널 있음 | 저널과 디스크 비교 표시 | Recovering |
| Loading | 이진·비 UTF-8·상한 초과 또는 lease가 다른 창에 있음 | — | ReadOnly |
| Recovering | 복원 | `baseRev = diskRev`로 버퍼 적용 | Dirty |
| Recovering | 버림 | 저널 삭제(CAS) | Clean |
| Clean | edit | `editSeq++`, 저널 300ms 예약, auto 정책이면 저장 1초 예약 | Dirty |
| Dirty | 저널 tick | `documents.journal{lease, seq, baseRev, text}` | Dirty |
| Dirty | save | `inflight = editSeq`, `documents.save{lease, baseRev, text}` | Saving |
| Saving | edit | `editSeq++` | Saving |
| Saving | 외부 변경 이벤트 | 보류 | Saving |
| Saving | ok(newRev) | 보류한 이벤트 중 rev == newRev(자기 저장 echo)는 버림. `editSeq == inflight`면 저널 삭제(CAS ≤ inflight) | Clean, 아니면 Dirty(`baseRev = newRev`) |
| Saving | conflict(diskRev, diskText) | 버퍼·저널 보존, 비교 화면 | Conflict |
| Saving | io 오류·연결 끊김 | 오류 배지, 재연결 뒤 같은 `baseRev`로 재시도 | Dirty |
| Clean | 외부 변경 | undo 가능한 트랜잭션으로 다시 읽기 | Clean |
| Dirty | 외부 변경 | — | Conflict |
| Clean·Dirty | 외부 삭제 | 버퍼 유지. 저장은 `expect: absent`로 다시 만들기 | Orphaned |
| Conflict | 내 것 유지 | `baseRev = diskRev`로 저장 | Saving |
| Conflict | 디스크 것 | 버퍼 교체(undo 가능), 저널 삭제(CAS ≤ 현재 `journalSeq`). 다음에 열 때 Recovering으로 가지 않는다(AR-F13) | Clean |
| Conflict | 병합 | 편집 | Dirty |
| Clean·Dirty | rename | 새 경로 저널 확정 → `RENAME_NOREPLACE` → (노트) 링크 재작성 → 옛 저널 삭제. 실패하면 원래 상태로 | 원래 상태 |
| 모든 상태 | lease 빼앗김 | — | ReadOnly |
| Dirty | close | 저장 / 버림(저널 삭제, CAS) / 초안 유지(저널 보존) 중 선택 | Closed |

- 저널을 지우는 모든 행은 "그 문서의 저널이 다음 시작에 복구 후보로 다시 나오지 않는다"를 시험으로 고정한다(00-roadmap §7, CV-F03·AR-F13).

### 8.5 키보드

**포커스 범위 규칙**
- 터미널이나 편집기에 포커스가 있으면, 앱은 Ctrl+Shift+문자, Ctrl+Alt+문자, Ctrl+숫자, F키만 가져간다. 셸은 Ctrl+Shift+X와 Ctrl+X를 같은 바이트로 받으므로 잃는 키가 없다.
- 단독 Ctrl+문자는 터미널·편집기 밖에서만 별칭으로 동작한다.

**매칭과 등록**
- 단축키는 `event.code`(물리 키)로 매칭한다. 한글 입력 상태에서도 같은 키로 동작한다.
- 레지스트리 하나에 범위(전역·앱·섹션·터미널·편집기)와 함께 정의한다. 설정 화면에서 충돌을 검사한다.

**IME**
- 조합 중 Enter는 무시한다.
- 제출 단축키(Ctrl+Enter)는 `compositionend` 뒤 한 번 실행한다.
- 에이전트 지시는 xterm이 아니라 작성란에서 받고 bracketed paste로 보낸다.

| 기능 | 기본값 |
|---|---|
| 섹션 이동 | Ctrl+1–6 |
| 명령 팔레트 | Ctrl+Shift+P, F1 (터미널·편집기 밖에서는 Ctrl+K도) |
| 빠른 열기 | 터미널·편집기 밖에서 Ctrl+P (팔레트와 같은 창, 파일·노트·프로젝트·요청) |
| 프로젝트 전환 | Ctrl+Shift+O |
| 목록 접기 | Ctrl+Shift+B |
| 하단 패널 | Ctrl+\` |
| 새 에이전트 작업 | Ctrl+Shift+N |
| 다음 입력 대기 | Ctrl+Shift+J |
| 일일 기록 | Ctrl+Alt+D |
| 검색 | 터미널 포커스면 터미널 검색, 그 밖에서는 통합 검색 (Ctrl+Shift+F) |
| 확대·축소 | 터미널 포커스면 글꼴, 그 밖에서는 UI 배율 (Ctrl+= / Ctrl+-) |
| 보내기 | Ctrl+Enter (CodeMirror에는 `Prec.highest`로 등록) |
| 되돌리기 알림 실행 | 입력란 밖에서 Ctrl+Z (10초) |
| 전역: 팔레트 | Ctrl+Alt+Space (개발 빌드는 Ctrl+Alt+Shift+Space) |
| 전역: 빠른 캡처 | Ctrl+Alt+N (개발 빌드는 Ctrl+Alt+Shift+N) |
| 전역: 터미널 빠른 호출 | 설정에서 지정(기본 없음) |

### 8.6 동작 규칙과 문구

**확인과 되돌리기**
- `method!`의 확인 정책 값으로 자동으로 붙인다(화면은 `confirmPolicy(method)`로 읽음).
- `reversible` → 바로 실행하고 10초 되돌리기 알림을 띄운다. 예: 서비스 중지, hunk stage, 노트 휴지통 이동.
- `always` → 대상이 적힌 확인을 받는다. 예: 프로세스 종료, 커밋하지 않은 변경 버리기, force push, 병합하지 않은 커밋이 있는 에이전트 작업 폐기.
  - 확인 버튼은 "worktree 삭제"처럼 동사 + 대상으로 쓴다. 기본 포커스는 취소에 둔다.

**오류 위치**
- 입력 오류는 입력란 아래에 보인다.
- 배경 작업의 오류는 토스트로 띄우고 알림 센터에 보관한다.
- 영역 전체가 실패하면 ErrorState를 보인다.
- 섹션 본문마다 React 오류 경계(`SectionBoundary`)를 둔다. 한 섹션의 그리기 오류가 다른 섹션이나 열린 문서의 초안을 지우지 않고, ErrorState에 [다시 시도]·[다른 섹션으로]를 준다. 비동기·이벤트 오류는 경계가 잡지 않으므로 위 규칙(토스트·결과 확인 필요)을 따른다(AR U07).
- 결과를 모르는 mutation은 "결과 확인 필요"로 표시한다.

**정합성 규칙(CV·AR 결함의 근본 원인, 08 §8.2)**

새 기능을 설계할 때 아래 다섯 가지를 확인한다. 각 규칙에 대응하는 v0.9.0 결함을 괄호에 적는다.

1. **본 것과 실행할 것을 묶는다.** 확인을 받는 변경은 사용자가 본 상태의 `expect`를 함께 보내고, 데몬이 실행 직전 비교한다(§5.2. AR-F01, CV-F01).
2. **성공한 변경과 뒤따르는 조회를 나눈다.** 생성·변경 응답으로 화면 상태(새 ID, 편집 모드)를 먼저 확정한다. 이어지는 목록 조회가 실패하면 조회 오류로만 보이고 변경을 다시 하지 않는다. 다시 누르면 같은 `clientRequestId`를 쓴다(AR-F04·F15).
3. **관찰 신호와 데이터 변경을 나눈다.** heartbeat·관찰 시각은 rev를 올리지 않는다(§5.5 R8). 갱신 중에는 마지막 성공 결과와 선택을 유지한다(CV-F07, AR-F11·F12).
4. **화면 수명과 작업 수명을 나눈다.** 오래 걸리는 작업은 데몬·Windows 앱이 진행 작업 ID로 소유하고, 화면은 다시 붙는다. 결과는 그 작업을 시작한 요청의 출처(메서드·대상·요청 ID)와 함께 보여 준다(CV-F09, AR-F08·F16).
5. **전환은 초안을 버리지 않는다.** 종류·대상·필터를 바꿔도 편집 중인 초안은 종류·대상별로 메모리에 남긴다. 보내기·저장할 때만 활성 항목으로 줄인다. 사용자가 버리기를 고른 초안은 복구 저장소에서도 지운다(AR-F06·F07·F13, CV-F03).

**용어**
- 작업 = `[[task]]`, 서비스, 예약
- 실행 = 작업·서비스를 한 번 돌린 것
- 에이전트 작업
- 진행 작업 = 오래 걸리는 데몬 작업
- 작업 폴더 = worktree
- 전체 용어 표는 `docs/ui-terms.md`에 둔다.

**문구**
- 한국어로 쓴다. 작업·대상·결과 중심으로 쓴다.
- 내부 용어와 v0.7 앱 이름을 쓰지 않는다. 단축키는 `Ctrl`로 적는다.
- CI의 금지어 검사가 화면 문자열에서 `Workbench`·`Code Pad`·`Repo Manager`·`Run Manager`·`Log Lens`·`Port Manager`·`⌘`·`route`·`generation`·`receipt`·`provenance`를 찾는다.

## 9. 도메인별 설계

각 도메인 crate의 구성:
- `lib.rs`: Service·공개 타입·`method!`·`topic!`
- `model.rs`: 순수 로직
- `store.rs`: SQLite
- `sys.rs`: OS·외부 명령
- `tests/`

| crate | 책임 | 주요 메서드(예) | 이벤트 |
|---|---|---|---|
| `protocol` | 프레임·메시지·`method!`·`topic!`·`domain_error!`·라우터·공통 오류 | — | — |
| `mux` | 프레임 헤더 라우팅·채널·재연결 상태(플랫폼 독립) | — | — |
| `core` | 인스턴스·경로·설정(toml)·DB(writer·마이그레이션·백업)·로그·원자적 쓰기·`systemd-run`·`systemctl` 헬퍼 | — | — |
| `secrets` | 데이터 키(DPAPI blob via interop)·AEAD·비밀 CRUD·복구 문자열 | `secrets.list/set/delete/export_key/import_key` | — |
| `projects` | 프로젝트·worktree 목록, 탐색, clone, `.devbox/devbox.toml` 읽기·감시, 시작 구성 | `projects.list/add/clone/remove/launch` | `projects.changed` |
| `git` | git CLI(porcelain v2): 상태·diff·hunk·commit·branch·stash·log·blame·remote·충돌·worktree·rebase·정리·gh PR | `git.status/diff/stage/commit/...` | `git.changed`(inotify로 `.git` 감시) |
| `terminal` | tmux 서버·세션 관리, PTY attach 스트림, 스크롤백 선채움, 격자용 읽기 전용 attach, 프로필, OSC 7 | `terminal.list/create/attach/resize/close` | `terminal.changed`, 스트림 |
| `agents` | 에이전트 작업 상태 기계(§9.2), 준비·hook·재개·기록 고정·기준 반영·정리, 자원·토큰, 도구 프로필·템플릿·포트 | `agents.create/list/send/resume/rebase/merge/pr/discard/cleanup` | `agents.changed`, `agents.attention` |
| `runtime` | 작업·서비스·예약 정의(DB + toml 병합), `systemd-run` 실행, 실행 래퍼 보고, cron(DST), 놓친 예약, Windows 대상 동기화·실행, Docker 컨테이너 | `runtime.list/start/stop/restart/logs/containers` | `runtime.changed`, 로그 스트림 |
| `observe` | 포트(`/proc/net/tcp*`)·프로세스(`/proc`)·문제 추출·WSL 자원(PSI·meminfo·df)·로그 소스(파일 tail·컨테이너 로그) | `observe.ports/processes/problems/system` | `observe.problems.changed` |
| `lsp` | 서버 수명(프로젝트·언어별, 세션 0개부터 15분 유휴 종료), 메시지 단위 중계(데몬이 Content-Length 프레이밍을 벗기고 붙여 JSON 문자열을 주고받음, `@codemirror/lsp-client`의 `Transport`와 맞춤) + `publishDiagnostics` 엿보기, URI 권위, UTF-16 위치만 광고, 재시작 백오프(5분 3회 초과면 실패), 서버 감지·설치 명령. `/mnt/c`의 Windows 쪽 변경은 inotify가 감지하지 못함 | `lsp.open/close/servers/install` | 스트림, `lsp.restarted` |
| `documents` | 문서 lease·읽기·저장(CAS·원자적 교체·mode/EOL/BOM 보존)·저널·rename·외부 변경 감시. 저널은 데이터 폴더(`<data>/journals/`)에만 둔다. 원자적 교체의 같은 폴더 임시 파일은 `.devbox-tmp-<난수>` 이름을 쓰고, 트리·감시·색인·링크 재작성에서 제외하며, 데몬 시작 때 1시간 넘은 것을 지운다(CV-F06) | `documents.claim/release/save/journal/rename` | `documents.changed` |
| `notes` | vault·frontmatter·위키링크·백링크·템플릿·첨부·링크 재작성·일일 기록·살균 HTML | `notes.tree/daily/render/links/...` | `notes.changed` |
| `search` | 노트 FTS5(`index.db`) + 코드 검색(`grep-searcher`·`ignore`) + 파일 이름, 저장된 검색 | `search.query/saved` | — |
| `activity` | 이벤트 저장(묶어 쓰기), sessionizer(idle), 개인정보 규칙 검증·저장, 프로젝트 귀속, 요약·내보내기 | `activity.ingest/day/range/rules/summary` | `activity.changed` |
| `api` | HTTP·GraphQL·WS·SSE·gRPC·MCP 실행, 환경·변수·비밀, 인증·쿠키·TLS, assertion·캡처, 실행기, 코드 생성, 가져오기, 기록 | `api.send/cancel/collections/import/codegen/...` | 스트림 |
| `webhooks` | 리스너(hyper), 수신 기록, 응답 규칙, fixture·재전송 | `webhooks.start/stop/rules/history/replay` | `webhooks.received` |
| `tools` | 개발 환경 점검(doctor)·도구 설치 명령·지원 번들. 변환 도구 21종은 화면(TS)에 둔다(해시·HMAC는 WebCrypto) | `tools.doctor/install_command/support_bundle` | — |
| `cli` | 바이너리 조립: daemon·bridge·exec·mcp·dev-gateway·setup·doctor·secrets·CLI | — | — |
| `win` | Windows 앱 전용: 활동 수집, 오버레이 배지·FlashWindow, (대체 경로) DPAPI | — | — |

### 9.1 회귀 조건 (도메인별로 꼭 지킬 동작)

1. **Git 충돌 해결 (CV-F01):**
   - 미리보기를 만든 시점의 작업 파일 해시와 index stage OID를 기록한다. 적용 직전에 다시 비교하고, 달라졌으면 적용하지 않고 비교로 돌아간다.
   - 작성 중인 해결안은 보존한다.
2. **문서 저장 (CV-F02·F03·F06, AR-F13, PD-M5):**
   - §8.4 상태 기계를 따른다. 원자적 교체는 `renameat2(RENAME_EXCHANGE)`, 새 파일은 `RENAME_NOREPLACE`를 쓴다.
   - mode·EOL·BOM을 보존하고, symlink는 realpath 대상에 쓴다.
   - 이진·비 UTF-8·크기 초과 파일은 읽기 전용으로 연다.
   - 저장 충돌은 생성된 오류 코드(`documents.conflict`)로만 분류한다. 문구·정규식으로 분류하지 않는다(CV-F02).
   - 저널은 데이터 폴더에만 둔다. 같은 폴더 임시 파일(`.devbox-tmp-*`)과 vault `.trash/`는 트리·색인·백링크·링크 재작성에서 제외한다(CV-F06).
   - 사용자가 버린 초안(Recovering → 버림, Conflict → 디스크 것, close → 버림)과 rename의 옛 경로 저널은 다음 시작에 복구 후보로 다시 나오지 않는다(CV-F03·AR-F13).
3. **여러 파일 링크 재작성 (AR-D01):**
   - 노트 rename의 링크 재작성은 먼저 대상 파일 목록과 각 `baseRev`를 확정하고, 진행 저널을 기록한 뒤 파일별로 CAS 저장한다.
   - 중간에 프로세스가 종료되면, 다음 시작 때 진행 저널로 남은 파일을 다시 시도하거나 사용자에게 목록을 보여 준다.
   - 대상이 열려 있고 dirty이면 그 버퍼에 적용하거나 rename을 거부한다.
4. **API 실행 (CV-F04·F05, AU-AS-01·02·07):**
   - 편집 초안과 실행 snapshot을 분리한다.
   - 캡처 값은 실행 ID·생산 요청 ID를 출처로 가진다. 생산 요청이 건너뛰기·취소·실패면 그 값을 쓰지 않는다.
   - 활성 헤더의 이름·값은 RFC 9110 문법으로 전송 전에 검사한다.
   - 비활성 본문·인증은 보내지 않는다. query·form은 URL serializer로 인코딩한다.
5. **검색·관찰 표시 (CV-F07, AR-F11·F12):** 갱신 중에는 마지막 성공 결과와 선택을 유지한다. 데이터 rev가 바뀌었을 때만 다시 조회한다(관찰 시각만 바뀐 것은 R8). 수집 상태(`activity.status`)처럼 제어 버튼의 의미를 정하는 값은 목록 조회와 별도 query로 두어, 늦게 끝난 목록 조회가 덮지 못하게 한다.
6. **활동 저장 실패 (CV-F08, AR-F05):**
   - 데몬 DB 오류는 화면과 트레이에 "저장 중단"으로 표시한다.
   - 묶어 쓰기에서 실패한 묶음은 버리지 않고 데몬 메모리 재시도 큐에 둔다(상한 10,000건, 넘치면 오래된 것부터 버리고 버린 수를 표시, 06 Task 8). 묶음 중 한 건의 실패가 나머지를 잃게 하지 않는다. 중복은 세션 ID로 막는다.
   - 수집 중지 요청은 저장 실패와 관계없이 바로 따른다.
   - 연결이 끊긴 동안의 대기는 §6.1의 1,000건 규칙을 따른다.
7. **긴 작업 다시 붙기 (CV-F09):**
   - 진행 작업 ID로 다시 붙을 수 있다.
   - 업데이트 진행은 Windows 앱이 소유하고 모든 창에 알린다.
8. **노트 전환 (AU-K1):** 문서마다 `EditorState`를 둔다.
9. **에이전트 병합:**
   - 병합 전에 대상 브랜치 체크아웃의 작업 트리가 깨끗한지 확인한다.
   - 충돌이 나면 병합을 중단하고 충돌 해결로 간다.
   - 폐기할 때 병합되지 않은 커밋이 있으면 개수를 보여 주고 확인받는다.
10. **서비스 재시작 (AR-F14):** 재시작 간격은 systemd `RestartSec=1s`·`RestartSteps=5`·`RestartMaxDelaySec=30s`로 늘어난다. 재시작은 systemd 한도(`StartLimitBurst`)를 따른다. 한도를 넘으면 `failed`로 두고 알린다. 수동 재시작은 한도를 초기화한다(`reset-failed`).
11. **터미널 입력 순서:** 붙여넣기·Enter·동시 입력은 스트림 순서대로 쓴다. 전송에 실패하면 남은 입력을 버리고 알린다(v0.9.0 `orderedInput` 이식).
12. **확인한 상태로만 실행 (AR-F01):** 에이전트 병합·PR·폐기, Git push·pull·force push·브랜치 삭제, 프로세스 종료는 §5.2의 `expect`를 받는다. 확인 뒤 브랜치·HEAD·upstream이 바뀌었으면 `git.state_changed`로 거절하고 원격 ref를 바꾸지 않는다.
13. **생성 결과와 조회 실패의 분리 (AR-F04·F15):** 생성 mutation이 성공하면 화면은 응답의 ID로 편집 상태를 바로 바꾼다. 이어지는 목록 조회 실패는 조회 오류로만 보이고, 저장 버튼을 다시 눌러도 같은 `clientRequestId`로 보내 새로 만들지 않는다. 열린 상세는 `*.changed` rev로 갱신한다(처음 읽은 값에 고정하지 않음).
14. **API 초안과 결과 출처 (AR-F06·F07·F08):** 본문 종류(GraphQL·JSON·raw·form·multipart)와 gRPC (대상, 메서드)마다 초안을 메모리에 따로 둔다. 송신 snapshot과 파일 저장에는 활성 종류만 넣고 비밀 정화 규칙을 그대로 지킨다. 결과는 그것을 만든 요청(메서드·요청 ID)을 표시하고, 지금 편집기와 출처가 다르면 드러낸다. 컬렉션 요청은 "저장"(같은 파일 갱신)과 "새 요청으로 저장"을 나누고 dirty를 표시한다(AR U04).
15. **텍스트 범위의 단위 (AR-F09):** 텍스트 안의 위치를 데몬과 화면 사이로 보낼 때는 UTF-16 code unit으로 보내고 필드 이름에 단위를 적는다(`*_utf16`). 변환 도구 정규식은 화면(TS)에서 실행한다. LSP는 UTF-16만 광고한다(§9 `lsp`).
16. **노트 제목 대체 (AR-F10):** frontmatter `title`이 없거나 공백뿐이면 색인·위키링크 해석의 제목은 파일 이름(확장자 제외)이다. 원본 노트는 고치지 않는다.
17. **서비스 준비와 재시작 원인 (AR-F02):** 재시작 원인은 프로세스 종료뿐이고 `restart` 정책을 systemd가 그대로 따른다(`no`면 다시 띄우지 않음). 준비 상태는 `ports_hint` 포트 수신 관찰로만 정한다(`127.0.0.1`·`::1`·`0.0.0.0`·`::`). 서비스 의존 순서는 앞 서비스의 준비됨을 기다리고(기본 60초), 넘으면 뒤 서비스를 시작하지 않고 실패로 표시한다.

### 9.2 에이전트 작업 상태 기계

| 상태 | 뜻 | 들어오는 조건 |
|---|---|---|
| `preparing` | worktree 생성·copy·setup 실행 중 | 생성 직후 |
| `running` | 도구가 일하는 중 | 준비 완료 후 도구 시작, 지시 전송, hook `UserPromptSubmit`·도구 사용 |
| `waiting` | 사용자 입력 필요(권한 요청·질문) | Claude hook `Notification`, (보조) 일정 시간 출력 없음 + 프롬프트 패턴 |
| `idle` | 턴 종료, 다음 지시를 기다림 | Claude hook `Stop`, Codex `notify`(agent-turn-complete) |
| `failed` | 준비 실패, 도구 비정상 종료 | setup 실패, scope 종료 코드 ≠ 0 |
| `review` | 사용자가 "검토 시작"을 눌렀거나 도구가 정상 종료 | 사용자 동작, scope 종료 코드 = 0 |
| `closed` | 병합·PR 생성 후 닫기·폐기 | 사용자 동작 |

- 목록의 "입력 대기" 그룹은 `waiting`과 `idle`이다. 행에 어느 쪽인지 표시한다. 세션이 없고 `closed`가 아니면 [재개]를 보인다.
- **전이 규칙:**
  - 도구 프로세스 종료는 scope unit 종료로 안다. 종료 코드가 128+신호이면 `failed(reason=killed:<신호>)`로 둔다. 다른 도구(이 PC의 wsl-resource-guard는 사용자 scope 프로세스를 화면에서 끝낼 수 있다)나 커널 OOM이 끝낸 경우다. 화면은 "외부에서 종료됨"과 [재개]를 보인다.
  - 재부팅 뒤 데몬이 시작하면, `running`·`waiting`·`idle`인 작업 중 tmux 세션이 없는 것을 `failed(reason=session_lost)`로 바꾸고 [재개]를 제공한다.
- **worktree 위치:** `<프로젝트 상위 폴더>/<프로젝트 이름>-agents/<slug>`.
  - 프로젝트 자동 탐색에서 `*-agents` 폴더는 제외한다.
  - 이 폴더는 등록한 프로젝트의 쓰기 허용 범위(§12)에 포함된다.
- **브랜치:** `agent/<slug>`. 겹치면 `-2…-99`를 붙인다(`agent_hub/plan.rs` 이식).
- **hook 주입(PL-5):** 사용자 설정 파일(`~/.claude/settings.json`, `~/.codex/config.toml`)은 건드리지 않고 실행 인자로만 넣는다.
  - Claude: `claude --settings <데이터 폴더>/agent-hooks/claude.json`. `Notification`(stdin의 `notification_type`이 `permission_prompt`·`agent_needs_input` → `waiting`, `idle_prompt` → `idle`. `idle_prompt`는 턴이 끝난 뒤 60초 넘게 입력이 없을 때 오는 알림이라 질문이 아니다), `Stop`(→ `idle`), `UserPromptSubmit`(→ `running`), `SessionStart`(세션 ID 기록)을 `<devbox> agent event --from claude`로 보낸다. stdin JSON의 `session_id`·`message`를 쓴다.
  - Codex: `codex -c 'notify=["<devbox>","agent","event","--from","codex"]'`. 마지막 인자의 JSON(`type = agent-turn-complete`, `last-assistant-message`)으로 `idle`을 안다. Codex의 승인 대기는 notify에 없고, Codex hook(`PermissionRequest`)은 사용자가 신뢰 승인을 해야 동작하므로 기본으로 쓰지 않는다. 대신 tmux 출력 감지(일정 시간 출력 없음 + 프롬프트 패턴)로 `waiting`을 추정한다.
  - 환경 변수 `DEVBOX_AGENT_ID`, `DEVBOX_INSTANCE`, `DEVBOX_PORT`를 넣는다. hook 명령은 `DEVBOX_AGENT_ID`가 없으면 아무것도 하지 않는다.
  - S0a에서 두 도구의 현재 버전(claude 2.1.x, codex-cli 0.160.x)으로 실제 동작을 확인한다.
- **기록 고정:** `closed`가 될 때 지시문·이어서 지시 이력·세션 ID·토큰 합계·커밋 목록·결과를 DB에 남긴다.

### 9.3 이식 표

| 출처(v0.9.0) | 대상 | 방식 |
|---|---|---|
| `crates/runtime-engine/src/core/cron.rs` (+테스트) | `runtime::cron` | 그대로 이식 |
| `crates/activity-engine/src/core/{sessionizer,privacy,idle,attribution}.rs` (+테스트) | `activity::model` | 그대로 이식. v0.9.0에는 AU-K2(반복 관찰 뒤 idle 종료 시 마지막 입력 뒤 298초 집계) 수정이 이미 들어 있다. 회귀 테스트 `idle_retroactively_excludes_all_last_input_tail_intervals` 등을 함께 옮긴다 |
| `apps/devbox-workspace/src-tauri/src/agent_hub/plan.rs`의 slug·branch (+테스트) | `agents::model` | 그대로 이식 |
| `crates/wsl-helper/src/agent_usage.rs`, `agent_resources.rs` | `agents::usage` | 이식 후 정리(Linux 직접 접근) |
| `crates/wsl-helper/src/document.rs`, `crates/knowledge-vault-engine/src/platform/document_publish.rs`의 RENAME_EXCHANGE | `documents::sys` | 그대로 이식 |
| `crates/knowledge-vault-engine/src/core/{rename,vault}.rs` | `notes` | 참고(링크 재작성 규칙) |
| `crates/terminal-engine/src/core/{multiplexer,shell_integration,parsers}.rs`, `packages/workspace-features/src/terminal/lib/orderedInput.ts` | `terminal`, 화면 | 참고 + `orderedInput` 그대로 |
| `crates/repositories-engine/src/commands/conflicts.rs`, `core/cleanup.rs`, `core/history_diff.rs` | `git` | 참고 |
| `packages/api-studio-features/src/requests/lib/importers/*`, `lib/{codegen,assertions,captures,jsonPath,graphql,openapi}.ts` (+테스트) | 화면 `tools/api` | 그대로 이식(TS) |
| `packages/api-studio-features/src/transforms/tools/*` (+테스트) | 화면 `tools/transforms` | 그대로 이식(TS) |
| `crates/http-client-engine/src/commands/request.rs`(AU-AS 수정본) | `api::http` | 참고 + 테스트 이식 |
| `crates/webhook-core` | `webhooks` | 이식 후 정리 |
| `crates/product-ipc/src/lib.rs`의 `TypeExporter`, `issue_codes!` | `protocol`, `xtask` | 참고 |
| `crates/agent-protocol`의 프레임 디코더(부분 수신·poisoning) | `protocol::frame` | 참고 + 테스트 이식 |
| `crates/markdown`(ammonia 설정) | `notes::render` | 그대로 이식 |

## 10. 설치·업데이트·릴리스

- **빌드:**
  - Linux 바이너리: `x86_64-unknown-linux-musl` 정적 빌드(Linux CI).
  - Windows 앱: Tauri NSIS(`installMode: currentUser`), Windows CI 러너. Linux 바이너리를 리소스로 포함한다.
- **설치:**
  1. setup 실행 → 사용자 폴더에 설치 → 앱 시작
  2. §6.5 확인 → `devbox setup` → 연결
  3. 첫 화면(에이전트)이 열린다.
  - 관리자 권한은 필요 없다(WSL 설치가 필요한 경우는 예외).
- **업데이트(릴리스 빌드만):**
  1. 앱이 하루 한 번과 요청 시 GitHub Releases `latest`를 조회한다.
  2. 새 버전이면 상태 표시줄·설정에 알린다.
  3. "업데이트" → `devbox-<v>-setup.exe` 다운로드 → GitHub 자산 digest(SHA-256)와 비교 → 실행 중 작업 요약을 보여 주고 확인받는다.
  4. 앱이 종료되고 `/S`로 setup이 실행된 뒤 앱이 다시 열린다.
  5. 새 앱이 데몬을 다시 배치한다(§4.6).
  - 서명 기반 Tauri updater는 쓰지 않는다(ADR 0016).
  - 개발 빌드(`1.0.0-dev`)는 업데이트 확인을 끈다.
- **자산 이름:** `devbox-<v>-setup.exe`. v0.9.0 업데이터가 찾는 `Devbox_{v}_x64-setup.exe`·`release-manifest.json`과 겹치지 않는다. v1.0.0 릴리스 노트에 "새 앱 수동 설치 → v0.9.0 수동 제거"를 적는다.
- **제거:** NSIS 제거 프로그램이 Windows 쪽을 지운다. WSL 쪽 데이터는 남긴다. 앱 진단의 "WSL 쪽 정리" 버튼이 unit과 바이너리를 지운다.
- **self-check:** `Devbox.exe --self-check`는 실행 여부와 내장 Linux 바이너리 버전 일치만 확인한다.
- **릴리스:**
  1. `v*` 태그를 push한다.
  2. 워크플로가 Linux 테스트·빌드 → Windows 빌드 → self-check → GitHub Release(setup 하나) 순으로 진행한다.
  - 문제가 있으면 다음 패치 버전을 낸다.
- **개발:**
  - WSL에서 `pnpm dev`: Vite(1450, strictPort) + `devbox daemon --instance dev` + dev-gateway(1451).
  - Windows 껍데기를 확인할 때만 `scripts/win-dev.ps1`(UNC 경로에서 `pnpm tauri dev`, 인스턴스 `dev`)을 쓴다.
  - 직접 써 보기(dogfood) 설치 파일은 기본으로 CI(`windows.yml`) 산출물을 `scripts/dogfood.sh`로 받는다. Windows 쪽 Rust·MSVC·pnpm을 설치했다면 `scripts/win-build.ps1`로 로컬에서 만들 수도 있다.

## 11. 검증과 개발 방식

**원칙:** 검증은 결함이 실제로 생기는 곳에 둔다. 검증 코드가 제품 코드보다 빨리 자라지 않게 한다(SC7).

| 층 | 무엇 | 어디서 | 언제 |
|---|---|---|---|
| L1 | Rust 단위: 도메인 순수 로직, `mux` 라우팅, 이식한 기존 테스트 | Linux | 매 PR |
| L2 | 데몬 통합: `DEVBOX_INSTANCE=test-<난수>`로 실제 git·tmux·systemd user에서 데몬을 띄워 RPC 시나리오 실행 | Linux(WSL·CI) | 매 PR |
| L3 | 화면 E2E: Playwright(Chromium) + dev-gateway + 실제 데몬. 섹션당 대표 여정 3–5개, 전체 40개 이하, 병렬 | Linux | 매 PR 전부 |
| L4 | Windows: `crates/win` 단위 테스트, 앱 빌드, self-check | Windows CI | 태그, 수동 실행, `app/src-tauri/**`·`crates/win/**`를 바꾼 PR |
| L5 | 사용자 실사용(dogfood) | 사용자 PC | S마다. 완료 조건의 수동 항목은 10개 이하이고 사용자가 1회 확인 |

- CI의 systemd user 세션: GitHub Ubuntu 러너에서 `sudo loginctl enable-linger $USER`와 `XDG_RUNTIME_DIR`을 설정한다. S0a에서 확인하고, 안 되면 systemd에 의존하는 L2 시나리오만 WSL 로컬 필수로 옮긴다.
- **flaky 테스트:** 발견한 PR에서 고치거나 지운다. 재시도 래퍼·격리 목록은 만들지 않는다.
- **PR 규칙:**
  - 하위 프로젝트 안에서 사용자 흐름 단위로 PR을 나눈다.
  - 필수 CI(Linux: fmt·clippy·test·L2·biome·vitest·tsc·`gen-ts --check`·금지어·L3)가 통과하면 squash 머지한다. 보호 규칙의 필수 검사는 이 job들을 모은 `ci-ok` 하나다(IR-1). L4 Windows 워크플로는 필수 검사가 아니다.
  - 진행 상태의 원장은 PR과 각 계획 문서 머리의 "상태" 줄이다. 체크박스는 작업 보조일 뿐이다.
- **에이전트 작업 규칙(새 AGENTS.md, S0b 첫 과제):**
  - 한 장 이내로 쓴다.
  - 로직은 실패 테스트부터 작성한다.
  - 바꾼 crate·화면의 테스트만 먼저 돌리고, PR 끝에 전체를 한 번 돌린다.
  - 새 방어 장치·검증 단계를 더하기 전에 구조로 없앨 수 있는지 먼저 본다.
  - 파일 600줄, 컴포넌트 300줄을 넘으면 나눈다.
  - RPC 경계에 `serde_json::Value`를 쓰지 않는다. `Result<T, String>`은 §5.3의 허용 범위에서만 쓴다.
  - 하위 프로젝트·과제마다 전용 git worktree와 브랜치를 쓴다.
- **자원:**
  - `CARGO_BUILD_JOBS=4`로 둔다.
  - `scripts/sweep.sh`(오래된 incremental 결과물 + `cargo sweep`)를 각 하위 프로젝트 완료 때와 target-dir이 150 GiB를 넘을 때 돌린다(00-roadmap §3 자원).
  - 무거운 검증은 한 번에 하나만 돌린다.

## 12. 보안 범위 (ADR 0016 유지)

**유지하는 것**
- 데몬은 unix socket(0600)만 연다. TCP 예외는 §5.1의 셋뿐이다.
- dev-gateway는 개발 전용이고 토큰 + Origin 검사를 한다.
- 웹훅 LAN 허용은 사용자가 켤 때만 한다.
- 비밀은 DPAPI 봉인 키로 암호화한다. 로그·파일·인자·MCP 응답에 평문을 남기지 않는다.
- 개인정보 규칙은 fail-closed로 동작한다.
- Markdown을 살균하고, CSP를 적용한다.
- 파일 쓰기·삭제는 등록한 프로젝트(에이전트 worktree 폴더 포함)·vault·devbox 데이터 폴더 안에서만 한다(`realpath` 확인 + `O_NOFOLLOW`).
- MCP 쓰기 도구는 기본으로 끈다.
- 외부 프로젝트를 처음 등록할 때 Git 실행 설정(`core.fsmonitor`·`core.hooksPath`·`filter.*`·`diff.external`)을 한 번 검사하고 경고한다.

**하지 않는 것**
- 코드 서명·업데이트 서명
- 렌더러 대상 session·route·replay·provenance 검사
- 네이티브 확인 대화상자
- 소켓 외 추가 접근 제어

## 13. 기존 ADR 처리

- 새 ADR은 **0017 "재구축: 단일 앱 + WSL 데몬"** 하나다. 이 설계(01-design)를 원장으로 가리킨다.
- 옛 ADR은 상태 줄만 갱신한다.
  - 대체됨(0017): 0001, 0002, 0003, 0004, 0007, 0009, 0010, 0011, 0012, 0014, 0015
  - 유지: 0005, 0006(Linux 형태로 축소), 0008(DPAPI 키 형태로 변경), 0013(모든 이전 데이터로 확대), 0016

## 14. 위험과 대응

| ID | 위험 | 대응 | 확인 시점 |
|---|---|---|---|
| R1 | WSL 배포판 유휴 종료(연결 없으면 15초)로 데몬·tmux가 멈춤 | 트레이 앱이 브리지 연결 유지, 자동 시작 기본 켬, 종료 시 경고, `instanceIdleTimeout=-1` 선택 안내. 합격 기준: 앱이 연결된 상태로 30분 유휴 뒤에도 tmux 세션 유지 | S0a |
| R2 | systemd user manager 동작 차이 | WSL이 로그인 세션으로 user manager를 띄움(실측), linger는 보험. 합격 기준: `wsl --shutdown` 뒤 앱이 다시 띄우고 데몬이 정상 복귀 | S0a |
| R3 | `wsl.exe` stdio 브리지의 지연·처리량·바이너리 안전성 | `cat` echo 실측은 목표의 수십 배(p50 0.16ms, 77–93 MiB/s, 바이너리 일치). S0a에서는 실제 bridge + 데몬 경로로 한 번만 확인. 미달이면 WSL localhost TCP + 토큰으로 전환 | S0a |
| R4 | Tauri Windows 빌드가 Windows에서만 가능 | 화면은 브라우저 + dev-gateway로 개발. 껍데기 변경은 드묾 | S0b |
| R5 | Claude Code·Codex의 hook·인자·로그 형식 변경 | `agents/adapters/{claude,codex}`로 격리, hook이 없으면 tmux 출력 활동으로 대체 감지 | S0a·S1 |
| R6 | 재구축이 기능 동등에 이르기 전에 멈춤 | 사용 빈도 순서. 각 S 끝의 빌드가 단독으로 쓸 만해야 함. **전환점:** S2가 끝나면 에이전트·실행·터미널은 새 빌드로 옮기고 나머지는 v0.9.0을 씀 | 매 S |
| R7 | WSL 안 LSP 메모리 | 지연 시작, 세션 0개부터 15분 유휴 종료, 메모리 표시 | S4 |
| R8 | 다시 비대해짐 | SC8 예산 점검, "구조 먼저" 규칙 | 매 S |
| R9 | 같은 체크아웃에서 여러 에이전트 세션이 동시에 작업 | 하위 프로젝트·과제마다 전용 worktree와 브랜치 | 상시 |
| R10 | systemd가 띄운 데몬에서 WSL interop(Windows 실행 파일 호출)이 안 될 수 있음 | S0a에서 확인. 안 되면 Windows 대상 작업·DPAPI 풀기·외부 열기를 Windows 앱이 대신 실행(브리지로 요청) | S0a |
| R11 | v0.9.0과 공존(단축키·트레이·활동 수집·업데이트 자산) | 개발 빌드 단축키 분리, 활동 수집 한쪽만, 자산 이름 분리, 개발 빌드 업데이트 끔 | S0b·S6 |
| R12 | Tauri 알림 플러그인은 데스크톱에서 클릭 이벤트가 없음(확인됨) | WinRT 토스트 + protocol activation + deep-link를 `crates/win`에서 직접 구현. 설치본에서만 동작 | S1 |
| R13 | systemd user manager 환경이 빈약해 실행·tmux에서 사용자 도구를 못 찾음(확인됨) | setup이 로그인 셸 환경을 잡아 `EnvironmentFile`로 주입, doctor 점검, [환경 다시 잡기] | S0b |
| R14 | inotify instances 한도(기본 128)가 LSP·감시기를 함께 쓰면 부족 | doctor가 한도와 사용량을 보이고, 늘리는 명령을 터미널에서 실행하도록 안내 | S4 |

## 15. 하위 프로젝트

| ID | 이름 | 완료 기준 요약 |
|---|---|---|
| S0a | 구조 확인 실험 | R1·R2·R3·R5(hook 주입)·R10·R12 합격 기준 측정, v0.9.0과 같은 여정 수치 비교, 결과를 ADR 0017 초안에 기록. 실패 항목은 대체 경로로 설계 갱신 |
| S0b | 걷는 뼈대 | 저장소 초기화(옛 코드·CI·지침 교체) · protocol·mux·core·cli(daemon·bridge·dev-gateway·setup) · `gen-ts` · 화면 골격(프레임·라우팅·RPC 클라이언트·연결 배너·오류 문구·팔레트 최소) · Windows 껍데기 최소(창·브리지·mux) · `system.ping`과 `projects.list/add`가 브라우저와 Tauri 양쪽에서 E2E 통과 · CI · 새 AGENTS.md·ADR 0017 |
| S1 | 에이전트·터미널·프로젝트 | 에이전트 전 과정(생성·준비 → 실행 → 입력 대기 알림 → 변경·커밋 검토 → 기준 반영 → 병합·PR·폐기·정리·재개), 격자 보기, 터미널 세션 유지·분리 창, 프로젝트 전환기·시작 구성(터미널·에이전트), 트레이·전역 단축키·알림, MCP 기본 도구, 이 섹션에 필요한 디자인 시스템 컴포넌트, SC3·SC4 |
| S2 | 실행·관찰 | 작업·서비스·예약·로그(소스 병합)·포트·프로세스·컨테이너·문제·WSL 상태, `.devbox/devbox.toml`, Windows 대상 실행, 비밀, 시작 구성(서비스), MCP 실행·로그 도구. **전환점** |
| S3 | 기록 | 노트·일일 기록·빠른 캡처·활동 기록·통합 검색, DocumentStore(노트 정책), MCP 노트 도구 |
| S4 | 편집·Git | 편집기 + 미리보기 + LSP, DocumentStore(코드 정책), Git 전체·충돌 해결·정리, Dependency Lens |
| S5 | 개발 도구 | API(순서대로 세 묶음)·웹훅·변환 도구 21종·개발 환경 점검·도구 설치 |
| S6 | 마무리·출시 | 설치기·업데이트·릴리스 워크플로, 성공 기준 측정, 문서 정리, v1.0.0 |
