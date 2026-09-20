# 릴리스 실행 정책

v0.8 공개 계약은 네 제품 portable ZIP + Suite setup + manifest + notices의 **7개 파일**이다.
Workspace WSL helper와 Control Center Suite helper는 소유 제품 ZIP에 포함하며 별도 사용자 제품이 아니다.
과거 15-app/32-asset parser와 pinned config는 v0.7 baseline/migration fixture에만 사용한다.

1. B09 source merge와 required CI/full audit가 끝나면 정확한 current main SHA와 예정 stable tag로
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
   네 portable 기본 native 실행을 확인한다. 결과는 #541/#542와 Actions artifact에 기록한다.
   결과 문서만을 위한 source 변경 PR로 이미 검증한 candidate SHA를 바꾸지 않는다.

stable tag push 또는 명시적 workflow_dispatch를 사용한다. 공개 RC/prerelease는 사용자의 명시 요청
없이 만들지 않는다. prerelease tag push는 거부하고, 요청된 경우에만 dispatch의
`allow_prerelease: true`를 사용한다. 그 별도 build를 stable candidate라고 취급하지 않는다.

[과거 release evidence](release-evidence.md), [최종 수용 추적](v0.8-acceptance.md).
