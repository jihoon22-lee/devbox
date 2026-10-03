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
3. 동일 후보 bytes로 네 native scope, 실제 WSL2/Docker, installer migration/update/undo/commit/
   removal 및 같은 VM의 성능 비교를 수행한다. 실패한 후보는 승격하지 않는다. fixture/model,
   packaged native, physical-device evidence를 구분한다. 미실행 환경을 PASS로 합산하지 않는다.
4. 성공한 비만료 후보와 같은 commit에 annotated stable tag를 만든다. Release는 tag·commit·repo·
   workflow run·7개 digest를 재확인하고 **재빌드 없이** draft에 올린다. 후보 없음/만료 시 닫힌 실패로
   끝나며 새 build를 대신 사용하지 않는다. code/package input이 바뀌면 새 exact-main 후보가 필요하다.
5. draft fresh-download name/size/SHA-256/component 검증 후 공개한다. 공개 후 다시 다운로드하고
   네 portable 기본 native 실행을 확인한다. 결과는 [통합 점검 이슈 #580](https://github.com/jihoon22-lee/devbox/issues/580)과 Actions artifact에 기록한다.
   결과 문서만을 위한 source 변경 PR로 이미 검증한 candidate SHA를 바꾸지 않는다.

## v0.9.0 공개 전 조건

- 제품·agent 버전은 #612에서 이미 0.9.0으로 올렸다. 공개 전 수정 때문에 0.9.1로 다시 올리지 않는다.
- 의존성·제품·패키지 입력이 바뀐 뒤에는 이전 후보의 성공을 새 source의 성공으로 재사용하지 않는다.
  exact-main CI → 새 후보의 전체 자동 수용 성공 → 동일 commit tag 순서를 따른다.
  CI는 main push에 자동 실행되지 않으므로, 최종 준비 변경을 모두 머지한 뒤 `ci.yml`을
  main에서 수동 실행하여 그 commit의 성공을 확인한다. 문서 준비 중 같은 전체 검사를 반복하지 않는다.
- 2026-10-03 사용자는 자체 코드 검토·수정·문서 정리 후 태그와 공개까지 진행하고, 실사용 중
  문제는 직접 제보하겠다고 지시했다. 사용자 실기 응답을 기다리는 조건은 해제한다.
  [P3-01](superpowers/plans/2026-09-23-review-remediation/p3-01-release-v0.9.0.md)의 신규 설치 8개와
  기존 데이터 보존 항목은 출시 후 추적으로 남기며, 미실행을 PASS로 기록하지 않는다.
- v0.8.1 내장 업데이터의 새 구성요소 거부는 확인된 호환성 제한이다. 공개 manifest에서 구성요소를
  숨기거나 검증을 완화하지 않는다. 중간 호환 릴리스는 만들지 않고, v0.9.0 Release에서 setup을
  직접 받아 실행하는 경로를 안내한다. 기존 데이터 보존의 실사용 확인은 별도 미실행으로 명시한다.
- 후보 성공 후 7일 이내에 태그한다. 만료됐으면 같은 commit으로 후보 workflow를 다시 실행하여
  검증한다. 실패한 후보를 원인 수정 없이 재빌드해 덮지 않는다.
- 결과 문서만 바꾸는 PR은 검증된 후보부터 태그까지 main을 움직이지 않도록 공개 뒤 머지한다.
  제품/의존성 수정이 필요한 경우에는 이전 후보를 승격 대상에서 제외하고 새 source로 준비를 다시 한다.

stable tag push 또는 명시적 workflow_dispatch를 사용한다. 공개 RC/prerelease는 사용자의 명시 요청
없이 만들지 않는다. prerelease tag push는 거부하고, 요청된 경우에만 dispatch의
`allow_prerelease: true`를 사용한다. 그 별도 build를 stable candidate라고 취급하지 않는다.

[과거 release evidence](release-evidence.md), [최종 수용 추적](https://github.com/jihoon22-lee/devbox-archive/blob/main/docs/v0.8-acceptance.md).
