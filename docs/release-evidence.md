# Release evidence index

## v0.9.0 — 수정본 확인과 철회 이력

2026-10-03 설치/UI 문제로 Release와 공개 자산을 삭제했다. #580은 NOT_PLANNED로 닫혔다.
수정본의 최종 source SHA·후보 run·수용 결과는 [GitHub Release](https://github.com/jihoon22-lee/devbox/releases/tag/v0.9.0)와 연결된 통합 PR·Actions artifact가 원장이다. 이 문서에 적힌 아래 결과는 **철회본의 역사적 자동 검사 결과**이며 수정본의 PASS로 재사용하지 않는다.
[현재 실행 계획](superpowers/plans/2026-10-03-product-readiness/00-roadmap.md)을 따른다.

### 수정본 후보의 재검증 출처

후보 `37306773490`의 실제 빌드 source는 `5d25aa75c3637e9d2e3ec1d0a5b25c2642960436`이다.
네 native scope·WSL2/Docker와 설치 UI 36개는 통과했지만 layout 및 Agent 종료 증거가
완료되지 않아 이 run 자체는 승격 대상이 아니다. fixture-only 보정 후 원본 7개 자산을
재빌드 없이 새로 수용·sealing하는 절차는 [릴리스 정책](release-policy.md#fixture만-바뀐-후보의-제한적-재검증)을 따른다.
빌드 SHA와 새 fixture SHA/run은 receipt와 Release에 각각 남기며, 새 seal·공개본 검증의
성공 전에는 재출시 완료로 기록하지 않는다.

재검증 `37322897417`에서는 전체 migration의 generation update가 `checkpoint_expired`로
실패하여 게시를 차단했다. checkpoint 파일 처리의 120초 제한 초과를 별도 진단하고 있으며,
제품 처리 코드를 변경한 보정은 fixture-only 예외에 해당하지 않는다. 원본 bytes의 통과 기록과
새 제품 bytes의 검증 기록을 구분하며, 최종 게시 상태는 위 원장의 최신 결과를 따른다.

### 철회 전 공개 이력

[Release](https://github.com/jihoon22-lee/devbox/releases/tag/v0.9.0)는 `2026-10-03T05:31:16Z`에 공개됐으며
당시 Latest=true, draft=false, prerelease=false였다. 현재는 철회됐다.

- source: `e499ac7127269bf67863bf0fdc42eaf53236b9f3`.
- annotated tag object: `f31f111977956bd665cd432accdf54677444027f`.
- 준비: #612 버전 0.9.0, #613 의존성·API Studio 캡처 보기 수정, #614 공개 조건·안내 정리.
- [exact-main CI](https://github.com/jihoon22-lee/devbox/actions/runs/37094287290):
  Frontend·Linux/Windows Rust·의존성 정책·catalog 모두 SUCCESS.
- [후보 37095144741](https://github.com/jihoon22-lee/devbox/actions/runs/37095144741):
  첫 실행 11개 job 모두 SUCCESS. assembly·네 native scope·설치/세대 전환/되돌리기/재설치/
  데이터 복구/제거·독립 WSL2/Docker 수용 완료. 핵심 결과 JSON 8개의 source와 성공 상태를 대조했다.
- [공개·공개본 검증 37100052952](https://github.com/jihoon22-lee/devbox/actions/runs/37100052952):
  후보를 재빌드 없이 승격하고 draft 검증 후 공개했다. 공개 파일을 다시 받아 네 제품 startup,
  native route, replay/다른 설치 요청 거부, 같은 설치 중복 실행 종료와 별도 설치 격리를 확인했다.
- 별도 공개 파일 재다운로드: setup·네 ZIP·manifest·notices **7개 모두 후보와 크기·SHA-256 동일**.
  manifest 네 제품·declared 6, verified 7, missing/undeclared/failures 0.
- [공개 근거와 7개 SHA-256](https://github.com/jihoon22-lee/devbox/issues/580#issuecomment-5966029569),
  [후보 상세 근거](https://github.com/jihoon22-lee/devbox/issues/580#issuecomment-5965966427)를 보존한다.

### 철회 당시 제한과 미완료 실사용 추적(역사 기록)

- 공개 v0.8.1 reader는 새 Knowledge helper·agent 구성요소를 거부한다. v0.9.0은
  Release의 setup을 직접 받아 실행한다. 중간 호환 릴리스나 manifest 검증 우회는 하지 않았다.
  [Windows 설치 안내](windows-guide.md#v090-설치와-알려진-제한)를 따른다.
- 2026-10-03 사용자 지시에 따라 신규 설치 8개·기존 데이터 보존 등 사용자 실기는 출시 후
  제보/추적으로 전환했다. 자동 수용 결과와 구분하며, 미실행을 PASS로 기록하지 않는다.
- #613은 npm 경고 12건을 0건으로 줄이고 캡처 보기 경쟁 상태를 RED→GREEN으로 수정했다.
  공개 전 전체 코드 검토·회귀·CI/PF 근거는 해당 PR에 있다. 이전 startup/Source와 Chromium
  시작 일회 지연의 최초 OS 원인은 당시 미확정으로 [#580](https://github.com/jihoon22-lee/devbox/issues/580)에 남겼다. 현재 재정비에서는 새 수용 근거를 별도로 확보한다.
- 이전 source `444a82e4`의 [후보 36489337168](https://github.com/jihoon22-lee/devbox/actions/runs/36489337168)는
  이후 의존성/코드 변경으로 승격 대상에서 제외했다. 그 성공을 이번 source의 근거로 재사용하지 않았다.

## v0.8.1

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
