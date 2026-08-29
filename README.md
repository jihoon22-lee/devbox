# devbox

Tauri 15개 데스크톱 앱(기존 안정판 13개 + v0.5.0에서 추가된 Devbox Launcher·Log Lens)을 하나의
모노레포로 관리하는 저장소. 각 앱은 **독립적으로 실행되고 독립적으로 `.exe`가 만들어집니다.**

## 앱 소개

| 앱 | 설명 |
|---|---|
| 🔥 **Port Manager** | 포트·프로세스 조회/검색/필터, 프로세스 종료, localhost 열기 |
| 🧰 **Developer Toolbox** | 개발용 소형 도구 13종 — JSON/Base64/URL/타임스탬프/Case, Hash/UUID/Regex/Diff( Rust), JWT 디코더 |
| 🐧 **WSL Desktop** | 앱 안의 임베디드 WSL 터미널 — 경로 지정 실행, 분할 레이아웃, 동시 명령, distro·Docker 상태 패널 |
| 🧪 **API Playground** | REST 요청 빌더. CORS 제약 없음, 응답 확인, 요청 history, curl 생성 |
| 🔍 **Everything+** | 파일명/내용 초고속 검색(FTS5), 정규식 모드, re-index 진행률 |
| 🗂 **Knowledge** | 마크다운 기반 지식 저장소 — 태그, 본문 검색, 데일리 노트 |
| 🕐 **Life Log** | 활동·git 데이터로 하루/주/월 요약 (활동 자동 추적 흡수), 캘린더 이동 |
| 📦 **Devbox Manager** | devbox 앱 버전 체크·설치·업데이트·실행 (휴대용/설치 패키지) |
| ✍️ **Code Pad** | CodeMirror 6 경량 코드 에디터 — 문법 하이라이팅, 탭·분할 2뷰, LSP(진단·자동완성·hover·정의·참조·이름 변경·포맷), 프리뷰 |
| ⏱ **Run Manager** | 예약 실행(크론 잡)과 상시 실행(서비스) 관리 — 실행 이력, 회전 로그 tail, 서비스 재시작·헬스체크 |
| 🛠 **Workbench** | 프로젝트 기반 orchestration 셸 — Git/WSL/포트/서비스 사전 점검, Run Manager·WSL Desktop·Code Pad 시작, Stop What I Started |
| 🔁 **Webhook Lab** | 로컬 웹훅/콜백 서버 — 수신 요청 history, 응답 rule·delay·오류 재현, 민감 헤더 masking |
| 🗂 **Repo Manager** | Git repository 탐색·브랜치/worktree/상태 목록, worktree 생성, Code Pad·WSL Desktop·Workbench로 열기 |
| 🚀 **Devbox Launcher** | Devbox 앱과 제공될 때 검증된 profile·repo·job·saved query snapshot 검색, 안전한 AppLink 실행 및 명시적 clipboard preview |
| 🔎 **Log Lens** | local/WSL/journal/container read-only 로그 tail·merge·filter·export, bounded in-memory ring |

## 다운로드 / 설치

Windows 11에서 실행 파일만 받아 바로 쓰려면 **Releases** 페이지를 이용하세요.

```
https://github.com/jihoon22-lee/devbox/releases
```

- **현재 공개 최신 안정판:** [`v0.5.1`](https://github.com/jihoon22-lee/devbox/releases/tag/v0.5.1)
- **v0.5.1 stable source/bundle:** #470 Windows acceptance inventory, #473 Run reader,
  #477 release gate, #478 Manager 보강, #479/#474 Run Manager named sidecar 계약을 포함한다.
  정확한 tag commit·workflow 결과·release asset 수와 digest·Latest metadata는
  [GitHub v0.5.1 release](https://github.com/jihoon22-lee/devbox/releases/tag/v0.5.1)가, Windows
  수동 acceptance는 #176이 권위 있는 source다. release contract는 15개 앱·32개 public asset·
  31개 manifest-declared asset·mismatch 0이다.
- **v0.5.0 stable evidence (historical):** source tag `efc98dd3c91b77ee7c9024010ac012a6c68f2b54`, release
  workflow `33216176818` 성공, 15개 앱·32개 public asset·31개 manifest-declared asset·mismatch 0,
  `draft=false`, `prerelease=false`, GitHub Latest였다. 이 수치는 v0.5.0 공개 package의 historical
  evidence이며 v0.5.1 publication metadata를 대신하지 않는다.
- **v0.4.2 검증:** 공식 Windows build/publish/manifest workflow, 13개 앱의 27 release
  assets 독립 size·SHA-256 대조와 exact stable API Playground portable의 packaged
  H1-A~D·cleanup을 통과했다. [상세 release plan](./docs/superpowers/plans/2026-08-24-v0.4.2-release.md)에서
  RC1 historical failure, RC2 수정 검증과 stable evidence를 함께 확인할 수 있다.
- **#176 수동 경계:** 63개 항목은 확인됐고 7개 physical Windows-only 항목은 아직 미확인이다.
  이는 공개 package의 asset count/hash 증거와 별도의 수동 acceptance 상태다.
- **RC 역사:** `v0.5.0-rc1`은 PR #464/CI `33173371194`, release workflow `33175165583`와
  32-asset 독립 검증을 남겼고 source audit에서 3/15 single-instance 누락으로 W4를 시작하지
  않았다. fix PR #465/CI `33178381902`는
  `a5256fe252fb0c2115adfd02d303c277aaf7bccb`에 병합됐다. `v0.5.0-rc2`는 PR #466/CI
  `33190371594`, release workflow `33192179195`와 32-asset 검증을 남겼고, Workbench
  preflight 경계 보완 후 fix PR #467/CI `33201855818`이
  `9dc237e23717bc294da0ff66d86df1bdce3cb595`에 병합됐다. RC1~RC3 tag/release는 사용자 지시로
  삭제됐으며 workflow/evidence만 historical record로 보존한다. 향후 RC는 사용자가 명시적으로
  요청한 경우에만 만든다.

- 각 앱의 `*-setup.exe`를 내려받아 설치하면 됩니다. WebView2 런타임(Windows 11 기본 포함)만 있으면 별도 도구 설치가 필요 없습니다.
- 자세한 사용/설치/트러블슈팅: [docs/windows-guide.md](./docs/windows-guide.md)

## 문서

| 문서 | 내용 |
|---|---|
| [사용 가이드](./docs/windows-guide.md) | Windows 11에서 설치·사용·빌드·문제 해결 |
| [개발자 가이드](./docs/development.md) | 구조, 시작하기, 개발 워크플로 |
| [아키텍처](./docs/architecture.md) | 모노레포 구조, 레이어, 데이터 흐름 |
| [로드맵](./docs/roadmap.md) | 진행 상황 / v0.5.0 history / v0.5.1 stable bundle |
| [v0.5.0 네이티브 우선 계획](./docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md) | P1·P2·선택 P3, 신규 앱, 앱 간 handoff, 테스트·릴리스 gate |
| [v0.5.0 릴리스 계획](./docs/superpowers/plans/2026-08-28-v0.5.0-release.md) | 목표 version, RC asset, Windows W1~W4, stable 승격·정리 gate |
| [의존성·제3자 고지 정책](./docs/dependency-policy.md) | Cargo·pnpm allowlist, advisory 예외 만료, notices 생성·배포 규칙 |
| [프로젝트 요약](./docs/projects.md) | 앱별 상세 요약 |
| [UX 개선 설계](./docs/superpowers/specs/2026-08-15-ux-improvements-design.md) | v0.5.0 컨텍스트 메뉴·클립보드·도구 확장 |
| [제품 기회 및 실행 계획 (완료·보존)](./docs/product-opportunities.md) | v0.1~v0.4 결정 근거 + 실행 계획 |
| [공통 규약](./CONVENTIONS.md) | 스택, 개발 워크플로, git 규칙 |
| [변경 이력](./CHANGELOG.md) | 버전별 변경점 |
