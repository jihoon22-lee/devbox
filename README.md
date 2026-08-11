# devbox

Tauri 8개 데스크톱 앱을 하나의 모노레포로 관리하는 저장소. 각 앱은 **독립적으로 실행되고 독립적으로 `.exe`가 만들어집니다.**

## 앱 소개

| 앱 | 설명 |
|---|---|
| 🔥 **Port Manager** | 포트·프로세스 조회/검색/필터, 프로세스 종료, localhost 열기 |
| 🧰 **Developer Toolbox** | 개발용 소형 도구 14종 — JSON/Base64/URL/타임스탬프/Case, Hash/UUID/Regex/Diff( Rust), JWT 디코더 |
| 🐧 **WSL Dashboard** | WSL 배포판·Docker·git 상태 대시보드, 컨테이너 start/stop/restart |
| 🧪 **API Playground** | REST 요청 빌더. Rust가 직접 요청해 **CORS 제약 없음**, 응답 확인, 요청 history |
| 🖥 **Activity Timeline** | PC 사용 기록 자동 추적(트레이 상시 실행), 하루 타임라인·앱별 사용 통계 |
| 🔍 **Everything+** | 파일명 초고속 검색(FTS5 인덱스), 검색 루트 관리 |
| 🗂 **Knowledge** | 마크다운 기반 지식 저장소 — 태그, 본문 검색, 데일리 노트 |
| 🕐 **Life Log** | 다른 앱의 활동·git 데이터를 모아 하루 요약 제공 |

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
| [로드맵](./docs/roadmap.md) | 진행 상황 / 계획 |
| [프로젝트 요약](./docs/projects.md) | 앱별 상세 요약 |
| [공통 규약](./CONVENTIONS.md) | 스택, 개발 워크플로, git 규칙 |
| [변경 이력](./CHANGELOG.md) | 버전별 변경점 |
