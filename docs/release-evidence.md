# Release evidence index

## v0.8.1 patch release

[PR #569](https://github.com/jihoon22-lee/devbox/pull/569)에서 리뷰 결함 수정과 네 제품의
0.8.1 버전·설치/portable 검증 설정을 함께 준비한다. 게시 완료 여부, exact-main source,
후보·게시 workflow와 Windows 수용 결과는
[v0.8.1 Release](https://github.com/jihoon22-lee/devbox/releases/tag/v0.8.1) 및 #541/#542의
후속 evidence를 원장으로 삼는다. 이 준비 문서는 게시 완료 또는 Windows PASS를 뜻하지 않는다.

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
- 현재 v0.7.0 stable은 #521~#536을 묶는다. annotated tag object는
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

- [v0.7.0 release plan](./superpowers/plans/2026-09-03-v0.7.0-release.md)
- [v0.7.0 publication evidence](../workthrough/2026-09-04-v0.7.0-stable-publication.md)
- [v0.6.0 release plan](./superpowers/plans/2026-08-31-v0.6.0-release.md)
- [v0.5.0 release plan](./superpowers/plans/2026-08-28-v0.5.0-release.md)
- [Release status and installed-app observations](./roadmap.md#release-status)
