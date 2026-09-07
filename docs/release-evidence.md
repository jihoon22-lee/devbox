# Release evidence index

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
