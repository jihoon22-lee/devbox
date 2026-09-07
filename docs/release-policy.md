# 릴리스 실행 정책

CONVENTIONS §4에서 분리한 릴리스 작업용 규칙이다. 릴리스 준비·검증·게시를 다룰 때 읽는다.

아래 3×5 shard·15개 앱·32개 public asset 계약은 **현재 v0.7 topology**다.
v0.8의 네 제품과 목표 asset 수는 아직 구현된 배포 계약이 아니다. WP08에서 catalog/manifest와
검증기를 함께 확정하고 WP09에서 검증하기 전에는 숫자만 바꾸거나 기존 gate를 우회하지 않는다.
exact-main 후보 검증 → 동일 source의 stable 승격 → fresh-download 검증은 유지한다.
사용자의 명시 요청 없이 public RC/prerelease를 만들지 않는다.

### 릴리스 트리거와 prerelease 보호

- 안정판을 만들기 전에 exact current `main` commit과 예정 `vX.Y.Z`를 입력해
  `Windows package candidate`를 실행한다. 카탈로그 순서로 균등 분할한 3개 Windows shard가
  각각 5개 앱만 빌드하고, Linux assembly가 누락·중복 없이 15개 앱·32개 파일인지 다시 검증한다.
  조립된 단일 후보로 packaged runtime·installer acceptance까지 통과시킨다. packaged runtime은
  assembly가 검증한 config 원본을 후보 evidence에서 그대로 사용한다. installer acceptance는 앱
  version이 바뀐 항목만 baseline→candidate update와 rollback을 수행하고, version이 같은 항목은
  baseline과 candidate의 독립 fresh install/uninstall로 검증한다. Windows checkout은 생성된
  notices를 LF로 유지하고, 앱별 NSIS build 전에는 저장소 내부의 고정 bundle staging만 비워
  이전 release resource가 새 installer에 재사용되지 않게 한다. 이후 같은 commit에
  annotated tag를 만들면 release workflow는 그 후보의 commit·tag·repository·workflow run·32개
  asset digest를 다시 확인한 뒤 바이너리를 재빌드하지 않고 승격한다. 일치하는 성공 후보가 없거나
  만료된 경우 공개 전에 fail-closed한다. 안정판에서는 prerelease build job이 의도적으로 skip되므로,
  최종 draft download verifier는 암묵적 `success()`에 의존하지 않고 `always()` 아래 preflight와
  draft-stage job의 명시적 success를 요구한다.
- `.github/workflows/release.yml`의 안정판 경로는 정확한 `vX.Y.Z` annotated tag push 또는
  명시적인 `workflow_dispatch`로 유지한다. `workflow_dispatch`의 `version`은 기본값 없이
  매번 전체 tag를 입력한다.
- `vX.Y.Z-...` prerelease/RC tag push는 허용하지 않는다. 중앙
  `.github/scripts/validate-release-input.py`가 build 전 preflight에서 fail-closed로 거부하므로
  Windows build가 시작되지 않고 GitHub Release도 생성되지 않는다.
- prerelease는 향후 필요할 때만 `workflow_dispatch`에서 전체 버전을 정확히 입력하고,
  의도적으로 이름 붙인 boolean 입력 `allow_prerelease: true`를 함께 지정해 실행한다. 이
  입력의 기본값은 `false`이며, gate 없는 수동 version 입력은 prerelease를 열지 않는다.
  stable-only candidate와 별개인 이 명시적 경로는 기존 Windows package build를 유지한다.
- 위 정책의 입력·상태 출력은 해당 Python 스크립트와 단위 테스트를 단일 원본으로 삼는다.

## 과거 릴리스 근거

[Release evidence index](./release-evidence.md)의 고정 기록을 참조한다.
