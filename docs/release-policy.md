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
  exact-main CI → 새 후보의 전체 성공 → 같은 후보의 실기 → 동일 commit tag 순서를 따른다.
- 현재 사용자 PC의 필수 실기는 신규 설치 기준이다. 세부 8개 항목과 기존 설치에서만 가능한
  데이터 보존 검증은 [P3-01](superpowers/plans/2026-09-23-review-remediation/p3-01-release-v0.9.0.md)과 #580에서 구분한다.
  필수 점검 응답 또는 사용자의 명시적인 “점검 생략”을 받기 전에는 태그하지 않는다.
- v0.8.1 내장 업데이터의 새 구성요소 거부는 확인된 호환성 제한이다. 공개 manifest에서 구성요소를
  숨기거나 검증을 완화하지 않는다. 기존 설치의 지원 경로를 확정하고 문서·검증 근거를 갖춘 뒤 공개한다.
- 후보 성공 후 7일 이내에 태그한다. 만료됐으면 같은 commit으로 후보 workflow를 다시 실행하여
  검증한다. 실패한 후보를 원인 수정 없이 재빌드해 덮지 않는다.
- 결과 문서만 바꾸는 PR은 검증된 후보부터 태그까지 main을 움직이지 않도록 공개 뒤 머지한다.
  제품/의존성 수정이 필요한 경우에는 이전 후보를 승격 대상에서 제외하고 새 source로 준비를 다시 한다.

stable tag push 또는 명시적 workflow_dispatch를 사용한다. 공개 RC/prerelease는 사용자의 명시 요청
없이 만들지 않는다. prerelease tag push는 거부하고, 요청된 경우에만 dispatch의
`allow_prerelease: true`를 사용한다. 그 별도 build를 stable candidate라고 취급하지 않는다.

[과거 release evidence](release-evidence.md), [최종 수용 추적](https://github.com/jihoon22-lee/devbox-archive/blob/main/docs/v0.8-acceptance.md).
