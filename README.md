# devbox

Tauri 15개 데스크톱 앱(기존 안정판 13개 + v0.5.0에서 추가된 Devbox Launcher·Log Lens)을 하나의
모노레포로 관리하는 저장소. 각 앱은 **독립적으로 실행되고 독립적으로 `.exe`가 만들어집니다.**

## 앱 소개

| 앱 | 설명 |
|---|---|
| 🔥 **Port Manager** | 포트·프로세스 조회/종료, Run·Workbench binding correlation, session timeline과 Log Lens 이동 |
| 🧰 **Developer Toolbox** | 개발용 소형 도구 13종, 탐지·pipeline·recent/favorite, API/Knowledge typed handoff |
| 🐧 **WSL Desktop** | 임베디드 WSL 터미널, 분할·broadcast, distro-user tool 탐지, profile/runtime snapshot |
| 🧪 **API Playground** | REST·GraphQL·SSE·WebSocket와 MCP HTTP/stdio/OAuth, dynamic gRPC/TLS/mTLS Protocol Lab |
| 🔍 **Everything+** | FTS5 파일명/본문·문서 검색, 고급 filter/saved query, Windows와 WSL root reconcile |
| 🗂 **Knowledge** | 마크다운 vault, 검색·template·quick capture·image·wikilink, WSL-native edit/watch |
| 🕐 **Life Log** | Windows/WSL Git·activity를 provenance와 함께 일/주/월 집계하고 Markdown/JSON/CSV export |
| 📦 **Devbox Manager** | 설치·업데이트·제거, 환경 capability, guarded Dev Setup, Data Inspector와 local quality |
| ✍️ **Code Pad** | CodeMirror 6 multi-file editor, offline managed LSP, Windows/WSL read-write-watch와 preview |
| ⏱ **Run Manager** | cron/service와 trusted workspace Task Runner, process-tree ownership, history/log/receipt |
| 🛠 **Workbench** | 프로젝트 orchestration, WSL/Git/dependency health, typed task control과 port correlation |
| 🔁 **Webhook Lab** | bounded local webhook, deterministic rule/conflict preview, OpenAPI draft와 sanitized handoff |
| 🗂 **Repo Manager** | repository/worktree 일상 흐름, safe cleanup과 WSL-native Git, offline/opt-in Dependency Lens |
| 🚀 **Devbox Launcher** | 앱·profile·repo/worktree·job·saved query typed source 검색과 stale-safe AppLink 실행 |
| 🔎 **Log Lens** | local/WSL/journal/container 로그 tail·merge·saved view·reconnect·filter·handoff·export |

## 다운로드 / 설치

Windows 11에서 실행 파일만 받아 바로 쓰려면 **Releases** 페이지를 이용하세요.

```
https://github.com/jihoon22-lee/devbox/releases
```

- **현재 공개 최신 안정판:** [`v0.5.1`](https://github.com/jihoon22-lee/devbox/releases/tag/v0.5.1)
- **v0.6.0 준비 상태:** milestone W01~W10 source는 main에 반영됐고 W11 통합 회귀·package
  checkpoint·Windows/WSL acceptance를 진행한다. v0.6.0 tag/release는 아직 만들지 않았으며
  [통합 릴리스 계획](./docs/superpowers/plans/2026-08-31-v0.6.0-release.md)과
  [release issue #493](https://github.com/jihoon22-lee/devbox/issues/493)이 현재 gate의 기준이다.
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
| [v0.6.0 통합 릴리스 계획](./docs/superpowers/plans/2026-08-31-v0.6.0-release.md) | W01~W11, 앱별 version, 비공개 package checkpoint, Windows/WSL acceptance와 stable publication |
| [의존성·제3자 고지 정책](./docs/dependency-policy.md) | Cargo·pnpm allowlist, advisory 예외 만료, notices 생성·배포 규칙 |
| [프로젝트 요약](./docs/projects.md) | 앱별 상세 요약 |
| [UX 개선 설계](./docs/superpowers/specs/2026-08-15-ux-improvements-design.md) | v0.5.0 컨텍스트 메뉴·클립보드·도구 확장 |
| [제품 기회 및 실행 계획 (완료·보존)](./docs/product-opportunities.md) | v0.1~v0.4 결정 근거 + 실행 계획 |
| [공통 규약](./CONVENTIONS.md) | 스택, 개발 워크플로, git 규칙 |
| [변경 이력](./CHANGELOG.md) | 버전별 변경점 |
