# Architecture

Devbox v0.8은 네 독립 Windows 제품과 명시적인 Suite 연결로 구성된다. 한 제품의 route가
보인다는 이유로 다른 제품의 데이터·process·secret 권한을 얻지 않는다.

- **제품 host:** `apps/devbox-*/src-tauri`가 command authority·제품별 저장소·resource 수명을 소유한다.
- **공유 UI:** product-shell은 context/navigation/Operations/Review를 제공하고 domain UI는
  각 feature package에 둔다. 기존 앱 frontend shell은 제거했다.
- **native engine:** 이미 소비 중인 14개 engine을 `crates/`로 옮겼다. 기존 domain test와 migration
  reader를 보존하며 standalone startup·icons·capability·Tauri bundle 설정은 제거했다.
- **Suite routing:** 검증한 installation/generation/member digest와 typed context를 사용한다.
  legacy executable을 찾아 대신 실행하는 경로는 제공하지 않는다. 과거 ID·argv/parser는
  read-only 진단·이전과 회귀 fixture에만 남는다.
- **데이터 이전:** 원본을 보존한 WAL consistent snapshot, destination namespace, idempotency와
  conflict 검토를 사용한다. 사용자 Git/vault 원본은 그대로 참조한다. cache와 live process는
  사용자 설정의 이전과 구분한다. secret은 raw payload로 전달하지 않고 재연결한다.
- **배포:** 두 Windows shard가 네 제품을 한 번 빌드한다. Windows assembly가 WSL/Suite helper를
  포함한 closed manifest·ZIP을 검사하고 Suite setup 하나를 만든다. 동일 bytes로 native,
  WSL2/Docker, migration/update/undo/commit/uninstall과 성능을 검증한다.
- **게시:** exact-main 성공 후보만 annotated stable tag로 승격한다. 게시 전후 다운로드 hash와
  공개 후 기본 native 실행을 확인하며 후보 없음·만료 시 rebuild로 대체하지 않는다.

세부 계약은 [foundation](architecture/v0.8-foundation.md), [제품 목록](projects.md),
[수용 추적](v0.8-acceptance.md), [릴리스 정책](release-policy.md)을 참조한다.
[v0.7 아키텍처 기록](history/v0.7/architecture.md)은 당시 사실을 그대로 보존한다.
