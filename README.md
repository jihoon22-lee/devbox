# devbox

Tauri 13개 데스크톱 앱을 하나의 모노레포로 관리하는 저장소. 각 앱은 **독립적으로 실행되고 독립적으로 `.exe`가 만들어집니다.**

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

## 다운로드 / 설치

Windows 11에서 실행 파일만 받아 바로 쓰려면 **Releases** 페이지를 이용하세요.

```
https://github.com/jihoon22-lee/devbox/releases
```

- 각 앱의 `*-setup.exe`를 내려받아 설치하면 됩니다. WebView2 런타임(Windows 11 기본 포함)만 있으면 별도 도구 설치가 필요 없습니다.
- 자세한 사용/설치/트러블슈팅: [docs/windows-guide.md](./docs/windows-guide.md)

## 문서

| 문서 | 내용 |
|---|---|
| [사용 가이드](./docs/windows-guide.md) | Windows 11에서 설치·사용·빌드·문제 해결 |
| [개발자 가이드](./docs/development.md) | 구조, 시작하기, 개발 워크플로 |
| [아키텍처](./docs/architecture.md) | 모노레포 구조, 레이어, 데이터 흐름 |
| [로드맵](./docs/roadmap.md) | 진행 상황 / v0.5.0 확정 범위 |
| [v0.5.0 네이티브 우선 계획](./docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md) | P1·P2·선택 P3, 신규 앱, 앱 간 handoff, 테스트·릴리스 gate |
| [프로젝트 요약](./docs/projects.md) | 앱별 상세 요약 |
| [UX 개선 설계](./docs/superpowers/specs/2026-08-15-ux-improvements-design.md) | v0.5.0 컨텍스트 메뉴·클립보드·도구 확장 |
| [제품 기회 및 실행 계획 (완료·보존)](./docs/product-opportunities.md) | v0.1~v0.4 결정 근거 + 실행 계획 |
| [공통 규약](./CONVENTIONS.md) | 스택, 개발 워크플로, git 규칙 |
| [변경 이력](./CHANGELOG.md) | 버전별 변경점 |
