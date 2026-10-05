# 릴리스 실행 정책

현재 Suite 공개 계약은 네 제품 portable ZIP + Suite setup + manifest + notices의 **7개 파일**이다.
Workspace WSL helper와 Control Center Suite helper는 소유 제품 ZIP에 포함하며 별도 사용자 제품이 아니다.
과거 15-app/32-asset parser와 pinned config는 v0.7 baseline/migration fixture에만 사용한다.

1. 릴리스 준비 변경의 merge와 required CI/full audit가 끝나면 정확한 current main SHA와 예정 stable tag로
   `Windows package candidate`를 main에서 dispatch한다. 이미 있는 tag/release·다른 source는 거부한다.
2. Linux에서 고정 source의 static WSL component를 만들고, 두 Windows shard에서 네 제품을
   한 번 빌드한다. Workspace+Control Center, API Studio+Knowledge를 배정한다. private LSP fixture는
   공개 payload 밖에 둔다. Windows assembly는 pinned NSIS hash와 모든 shard/source/component
   name·size·digest를 확인하고 Suite installer와 closed manifest를 조립한다.
3. 동일 후보 bytes로 네 native scope, 실제 UI 사용자 여정, WSL2/Docker, installer migration/update/undo/commit/
   removal 및 같은 VM의 성능 비교를 수행한다. 실패한 후보는 승격하지 않는다. fixture/model,
   packaged native, 실제 UI·OS evidence를 구분한다. 미실행 환경을 PASS로 합산하지 않는다.
   assembly는 비공개 중간 artifact다. 필수 40개 사용자 여정의 source·fixture·7개 자산 digest와 실제 스크린샷을 최종 집계한 뒤에만 승격용 후보 artifact를 만든다. `Verify complete user journeys and seal candidate` job이 없거나 성공하지 않았으면 resolver가 거부한다. Release 다운로드 검증도 같은 evidence를 다시 확인한다.
4. 성공한 비만료 후보와 같은 commit에 annotated stable tag를 만든다. Release는 tag·commit·repo·
   workflow run·7개 digest를 재확인하고 **재빌드 없이** draft에 올린다. 후보 없음/만료 시 닫힌 실패로
   끝나며 새 build를 대신 사용하지 않는다. code/package input이 바뀌면 새 exact-main 후보가 필요하다.
5. draft fresh-download name/size/SHA-256/component 검증 후 공개한다. 공개 후 다시 다운로드하고
   네 portable 기본 native 실행을 확인한다. 결과는 이번 단일 PR 본문·Release notes·Actions artifact에 기록한다. 닫힌 #580에 실기 요청을 남기지 않는다.
   결과 문서만을 위한 source 변경 PR로 이미 검증한 candidate SHA를 바꾸지 않는다.

## v0.9.0 공개 전 조건

- 제품·agent 버전은 #612에서 이미 0.9.0으로 올렸다. 공개 전 수정 때문에 0.9.1로 다시 올리지 않는다.
- 의존성·제품·패키지 입력이 바뀐 뒤에는 이전 후보의 성공을 새 source의 성공으로 재사용하지 않는다.
  exact-main CI → 새 후보의 전체 자동 수용 성공 → 동일 commit tag 순서를 따른다.
  CI는 main push에 자동 실행되지 않으므로, 최종 준비 변경을 모두 머지한 뒤 `ci.yml`을
  main에서 수동 실행하여 그 commit의 입력·검증 범위에 맞는 성공 근거를 확인한다.
  CI가 신뢰 가능한 이전 성공과 동일한 compiler/test 입력·범위를 확인하면 출처를 연결하여
  재사용한다. main SHA가 달라졌다는 이유만으로 같은 컴파일·테스트를 반복하지 않는다.
  정상 조회 결과에서 입력이 바뀌었거나 성공 근거가 없는 범위는 실행한다. 근거 조회·전송·
  해석 오류는 scope gate에서 진단과 함께 중단하고, 전체 컴파일로 자동 전환하지 않는다.
  주간 전체 감사와 현재 의존성·advisory 검사는 재사용하지 않는다. 이는 exact-main 후보의 40개 사용자 여정·
  패키지 수용·sealing이나 동일 commit stable 승격을 대신하지 않는다.
- 2026-10-03 철회된 v0.9.0은 현재 [제품 재정비 계획](superpowers/plans/2026-10-03-product-readiness/00-roadmap.md)에 따라 다시 출시한다.
  모든 개선·문서 준비를 통합 브랜치의 PR 1개로 마치고 필수 설치/UI/데이터 보존 수용을 공개 전에 완료한다.
  통합 PR #616 이후 후보에서 드러난 출시 차단 결함은 사용자 후속 승인에 따라 최소 보정 PR에 묶어 자율 처리한다.
  기존 후보의 패키지·실기 성공을 다른 source의 성공으로 바꾸지 않으며, PR 수 예외를 매번 다시 승인받지 않는다.
  동일 버전 철회본 위의 수정본 재설치와 데이터 보존도 필수다. 사용자 기본 실기 응답을 완료 조건으로 두지 않는다.
- 기존 v0.9.0 태그는 재출시 후보 직전에 Release 부재와 ref object를 확인하고 명시적 lease로 제거한다.
  철회본 tag object `f31f111977956bd665cd432accdf54677444027f`와 source `e499ac7127269bf67863bf0fdc42eaf53236b9f3`를
  역사 근거로 남긴다. 검증 성공한 최종 main에 동일 이름의 새 annotated tag를 생성한다.
  다른 태그를 바꾸거나 candidate의 tag-exists 검사를 우회하지 않는다.
- v0.8.1 내장 업데이터의 새 구성요소 거부는 확인된 호환성 제한이다. 공개 manifest에서 구성요소를
  숨기거나 검증을 완화하지 않는다. 중간 호환 릴리스는 만들지 않고, v0.9.0 Release에서 setup을
  직접 받아 실행하는 경로를 안내한다. 기존 데이터 보존은 합성 fixture의 실제 설치 수용으로 공개 전에 확인한다.
- 후보 성공 후 7일 이내에 태그한다. 만료됐으면 같은 commit으로 후보 workflow를 다시 실행하여
  검증한다. 실패한 후보를 원인 수정 없이 재빌드해 덮지 않는다.
- 결과 문서만 바꾸는 추가 PR은 만들지 않는다. 준비 문서는 최종 PR에 포함하고 실제 결과는 동일 PR 본문·Release notes·artifact에 기록한다.
  제품/의존성 수정이 필요한 경우에는 이전 후보를 승격 대상에서 제외하고 새 source로 준비를 다시 한다.

stable tag push 또는 명시적 workflow_dispatch를 사용한다. 공개 RC/prerelease는 사용자의 명시 요청
없이 만들지 않는다. prerelease tag push는 거부하고, 요청된 경우에만 dispatch의
`allow_prerelease: true`를 사용한다. 그 별도 build를 stable candidate라고 취급하지 않는다.

[과거 release evidence](release-evidence.md), [최종 수용 추적](https://github.com/jihoon22-lee/devbox-archive/blob/main/docs/v0.8-acceptance.md).
