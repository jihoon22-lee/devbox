# Devbox Control Center

v0.8의 네 사용자 제품 중 하나다. Products·Commands·Tools·Migration·Updates·Recovery를 통합한다.
B01~B08의 owner 수용은 완료됐고, 최종 B09 후보·공개 결과는 [#541](https://github.com/jihoon22-lee/devbox/issues/541)에 기록한다.

## 실행과 개발

- `pnpm --filter devbox-control-center dev`: 명시적으로 표시된 browser fixture.
- Windows의 `pnpm --filter devbox-control-center tauri dev`: 실제 native 제품.
- Browser `?route=products`와 debug `--route=products`는 개발용 route 선택이다.
  release 권한을 부여하는 옵션이 아니며 native host가 caller·session·route를 다시 검사한다.
- 제품 identity는 `com.devbox.v08.controlcenter`, 데이터는 installation별 namespace다.
  시작만으로 legacy source를 초기화하거나 원본 경로에 새 데이터를 쓰지 않는다.
- 배포는 Suite installer 또는 제품 ZIP 전체를 사용한다. [설치·이전·복구 안내](../../docs/windows-guide.md).

## 유지하는 계약

- 네 제품과 필수 components의 package identity·실행 상태·설치 상태를 구분해 표시한다.
- Commands는 bounded federated source와 한 개의 shortcut owner를 사용한다. 충돌·missing product·
  stale source를 진단하며 typed context를 통해 승인된 Suite member만 실행한다.
- Tools는 Environment·Data Inspector·doctor·support bundle·Related Tools/package-only setup을 제공한다.
  경로를 읽을 수 있다는 이유로 삭제·설치·실행 권한을 부여하지 않는다.
- Migration은 full/partial/mixed 원본을 검토하고 per-source apply/explicit skip 및 source 변경 재검토를 한다.
- Suite installer/bootstrap은 generation·journal·owned file identity로 install/update/undo/commit/
  restore/resume/uninstall을 수행한다. postcommit 새 데이터와 제거 후 사용자 데이터를 보존한다.
- legacy cleanup은 pinned installed-file reference와 exact ownership을 요구한다. locked/changed/unknown
  경로는 보존하고 pending 상태를 기록한다. 원본 uninstaller로 ownership 검사를 우회하지 않는다.

## 구현과 근거

기능 코드는 이 제품 host와 `packages/` feature UI, 이름을 가진 `crates/` engine에 있다.
기존 15개 앱의 standalone shell·Tauri bootstrap은 제거했다. 현재 소유권은
[projects](../../docs/projects.md), 기능/데이터 및 R01–R26/S01–S08 대응은
[acceptance trace](../../docs/v0.8-acceptance.md)를 참조한다.

[이전 개발 단계의 상세 README](../../docs/history/v0.8-development/control-center.md)는 당시 구현
순서·결정의 역사적 기록이다. 그 문서의 hidden/pending 상태를 현재 배포 상태로 해석하지 않는다.
제품 실행/installer 근거와 deterministic fixture·browser·physical device 검사는 구분한다.
