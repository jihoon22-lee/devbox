# Release evidence index

## v0.9.0 — 준비 중, 공개 전

제품과 agent의 버전은 [PR #612](https://github.com/jihoon22-lee/devbox/pull/612)에서
한 번만 0.9.0으로 올렸다. 공개 완료나 실기 PASS를 뜻하지 않는다. 코드 검토·남은 점검·
후보 source와 공개 판단의 정본은 [통합 이슈 #580](https://github.com/jihoon22-lee/devbox/issues/580)이다.

준비 기준 commit `444a82e4fed8498262490bdf822c07fac99c5f51`은
[main CI](https://github.com/jihoon22-lee/devbox/actions/runs/36487547078)와
[후보 36489337168](https://github.com/jihoon22-lee/devbox/actions/runs/36489337168)를 통과했다.
후보는 assembly·네 제품 native scope·설치/복구/제거·독립 WSL2/Docker를 모두 통과했고,
다운로드한 7개 자산과 내부 구성요소의 이름·크기·SHA-256도 확인했다.
[당시 후보 근거](https://github.com/jihoon22-lee/devbox/issues/580#issuecomment-5963355875)를 보존한다.
이후 [PR #613](https://github.com/jihoon22-lee/devbox/pull/613)은 의존성 갱신과 API Studio
캡처 보기 경쟁 상태를 수정해 `07840ca82502e33f196f7098951b6dfccbf10f77`에 머지됐다.
[최종 PR CI](https://github.com/jihoon22-lee/devbox/actions/runs/37087774292/attempts/2)와
[Windows PF](https://github.com/jihoon22-lee/devbox/actions/runs/37087774349)가 성공했다.
이 변경은 이전 후보에 없으므로, 공개 문서 정리까지 머지한 최종 exact-main의 CI와 새 후보가 필요하다.

2026-10-03 코드 검토에서 확인한 공개 전 조건:

- 공개 v0.8.1의 `core/suite_package.rs`는 Knowledge WSL helper와 agent를 허용하지 않아
  v0.9.0 manifest를 `suite_package_file_invalid`로 거부한다. 실제 이전 reader와 후보 manifest로
  재현했다. 자산 7개·schema 2 유지와 이전 업데이터의 읽기 호환은 다른 조건이다.
  내장 업데이터 전환은 지원하지 않으며 Release의 setup 직접 실행을 안내한다. 중간 호환 릴리스는 만들지 않는다.
- npm 감사의 undici 11건·DOMPurify 1건에 대응하여 기존 의존성을 각각 8.11.2·3.4.16으로 갱신했다.
  undici는 jsdom 테스트 경로, DOMPurify는 Mermaid runtime 경로다. DOMPurify 경고의
  `IN_PLACE`+노드 제거 hook 조건은 현재 렌더러에서 확인되지 않았으며, 실제 앱 XSS를 재현했다고 주장하지 않는다.
  PR #613에서 npm 감사 0건, 로컬 전체 검사와 PR CI/PF 통과를 확인했다. 최종 source 근거는 #580에 기록한다.
- 2026-10-03 사용자는 실사용 중 문제를 직접 제보하며, 자체 검토·수정 후 태그와 공개까지
  진행하도록 지시했다. 신규 설치 8개와 기존 설치 데이터 보존은 **출시 후 추적/미실행**으로 남긴다.
  사용자 실기 대기는 해제하지만 CI·후보·설치/실행·공개본 자동 검증은 유지한다.
- API Studio의 새 캡처 표시 직후 조회를 지연된 상태 초기화가 취소하는 경계를 RED 테스트로
  재현하고, 화면 표시 전 초기화로 수정했다. 최종 PR CI의 Vitest 2,216개가 통과했다.
  별개 Chromium 시작 지연은 동일 commit의 실패 잡 재실행에서 통과했으며 최초 OS 원인은 미확정이다.
- 이전 PF의 Workspace startup/Source 일회 실패는 후속 동일 바이너리 진단·full PF·후보에서
  재현되지 않았다. 최초 OS 원인은 미확정이며, 재발 시 조사할 근거를 #580에 보존한다.

## v0.8.1 — 현재 공개 stable

[Release](https://github.com/jihoon22-lee/devbox/releases/tag/v0.8.1)는 2026-09-21에 공개됐다.
source는 `1c97b41ee10ca0df7c062338bfe85659af025a89`, annotated tag object는
`dd5aa6581bcb48a5bf615d17f22db599aac5dace`다.
[후보 35559779579](https://github.com/jihoon22-lee/devbox/actions/runs/35559779579)와
[공개·공개본 검증 35564563792](https://github.com/jihoon22-lee/devbox/actions/runs/35564563792)가 성공했고,
setup·네 ZIP·manifest·notices의 7개 공개 자산, draft=false, prerelease=false를 확인했다.
과거 수용 원장 #541/#542는 닫힌 역사 기록이며 v0.9.0 점검 상태로 재사용하지 않는다.

## v0.8.0 source cutover — published

[Release](https://github.com/jihoon22-lee/devbox/releases/tag/v0.8.0)는 2026-09-20에 공개됐으며
source는 `d6208b37f199545965fa1a55b473f3c4e8add299`다. 네 제품 portable ZIP + Suite setup +
manifest + notices의 7개 파일, draft=false, prerelease=false를 확인했다.
[후보 수용](https://github.com/jihoon22-lee/devbox/actions/runs/35535443667)과
[공개 파일 재다운로드·제품 실행](https://github.com/jihoon22-lee/devbox/actions/runs/35538725828)을
기록한다. 물리 IME 등 미실행 환경의 제한은 [#542](https://github.com/jihoon22-lee/devbox/issues/542)에
구분돼 있다. 아래 v0.7 기록은 v0.8.1의 검증 근거로 재사용하지 않는다.

## v0.7 및 이전 기록


AGENTS.md에서 옮긴 historical stable 기록이다. 2026-09-07 정리 시점의 기준이며,
새 릴리스 작업 시 GitHub 상태를 다시 확인한다. 아래 수치는 v0.8 목표 topology에 적용하지 않는다.

## Preserved evidence

- v0.5.0 stable evidence는 tag `efc98dd3c91b77ee7c9024010ac012a6c68f2b54`와 workflow `33216176818` 기준 15개 앱·32개 public asset·31개 manifest-declared asset·mismatch 0이다.
- v0.5.1은 #470/#473/#477/#478/#479(및 닫힌 #474 계약)를 포함한 historical stable이다.
- 당시 v0.7.0 stable은 #521~#536을 묶는다. annotated tag object는
  `ec41ceb2ed4b4864d34afe383e5ff816481b3d37`, peeled source commit은
  `3a23f49c85aa3c3d04b86f227e8aa184ef964085`, candidate workflow는 `33782002859`, release
  workflow는 `33785966618`이다. candidate는 packaged runtime 15/15와 installer lifecycle
  15/15를 통과했고, 공개 release는 15개 앱·32개 public asset·31개 manifest-declared asset·
  missing/undeclared/failure 0이며 `draft=false`, `prerelease=false`, Latest다.
- v0.6.0은 milestone #2의 W01~W11을 포함한 historical stable이다. annotated tag object
  `a974adf975862da3d5ada16c6c6efe704387ddd7`, peeled source
  `d2fa25a0a1f087459838449daded00c0b09764b4`, candidate `33384213398`, release workflow
  `33390009009`의 evidence를 보존한다.
- #518은 설치된 WSL Desktop의 user-local zellij 탐색·attach·disconnect/reconnect·session 및
  workspace 유지가 2026-09-03 사용자 실기에서 PASS해 completed로 닫혔다. #176은 닫힌 v0.5.1
  historical checklist다. RC1~RC3 tag/release는 삭제된 historical record이며 미래 RC는 사용자의
  명시 요청 전에는 만들지 않는다.


## Detailed records

- [v0.7.0 release plan](https://github.com/jihoon22-lee/devbox/blob/v0.8.1/docs/superpowers/plans/2026-09-03-v0.7.0-release.md)
- [v0.7.0 publication evidence](https://github.com/jihoon22-lee/devbox/blob/v0.8.1/workthrough/2026-09-04-v0.7.0-stable-publication.md)
- [v0.6.0 release plan](https://github.com/jihoon22-lee/devbox/blob/v0.8.1/docs/superpowers/plans/2026-08-31-v0.6.0-release.md)
- [v0.5.0 release plan](https://github.com/jihoon22-lee/devbox/blob/v0.8.1/docs/superpowers/plans/2026-08-28-v0.5.0-release.md)
- [Release status and installed-app observations](https://github.com/jihoon22-lee/devbox/blob/v0.8.1/docs/roadmap.md#release-status)
