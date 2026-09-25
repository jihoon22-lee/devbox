# Devbox Control Center

v0.8의 네 사용자 제품 중 하나다. Products·Commands·Tools·Updates·Recovery를 통합한다.
B01~B08의 owner 수용은 완료됐고, 최종 B09 후보·공개 결과는 [#541](https://github.com/jihoon22-lee/devbox/issues/541)에 기록한다.

## 실행과 개발

- `pnpm --filter devbox-control-center dev`: 명시적으로 표시된 browser fixture.
- Windows의 `pnpm --filter devbox-control-center tauri dev`: 실제 native 제품.
- Browser `?route=products`와 debug `--route=products`는 개발용 route 선택이다.
  release 권한을 부여하는 옵션이 아니며 native host가 caller·session·route를 다시 검사한다.
- 제품 identity는 `com.devbox.v08.controlcenter`, 데이터는 installation별 namespace다.
  사용자 데이터와 설치 소유 파일의 경계를 구분한다.
- 배포는 Suite installer 또는 제품 ZIP 전체를 사용한다. [설치·복구 안내](../../docs/windows-guide.md).

## 유지하는 계약

- 네 제품과 필수 components의 package identity·실행 상태·설치 상태를 구분해 표시한다.
- Commands는 bounded federated source와 한 개의 shortcut owner를 사용한다. 충돌·missing product·
  stale source를 진단하며 typed context를 통해 승인된 Suite member만 실행한다.
  기억한 Suite 연결은 제품의 창·shell 준비 완료 후 재개하여 cold start 요청이 준비되지 않은
  Review 창에 전달되지 않도록 한다.
- Tools는 Environment·doctor·support bundle·Related Tools/package-only setup을 제공한다.
  경로를 읽을 수 있다는 이유로 삭제·설치·실행 권한을 부여하지 않는다.
- 새 설치는 데이터 및 복구 화면에서 네 제품의 저장소 상태를 기록하고 활성화를 확정한다.
- Suite installer/bootstrap은 generation·journal·owned file identity로 install/update/undo/commit/
  restore/resume/uninstall을 수행한다. postcommit 새 데이터와 제거 후 사용자 데이터를 보존한다.

## 구현과 근거

기능 코드는 이 제품 host와 `packages/` feature UI, 이름을 가진 `crates/` engine에 있다.
기존 15개 앱의 standalone shell·Tauri bootstrap은 제거했다. 현재 소유권은
[projects](../../docs/projects.md), 기능/데이터 및 R01–R26/S01–S08 대응은
[acceptance trace](https://github.com/jihoon22-lee/devbox-archive/blob/main/docs/v0.8-acceptance.md)를 참조한다.

[이전 개발 단계의 상세 README](https://github.com/jihoon22-lee/devbox-archive/blob/main/docs/history/v0.8-development/control-center.md)는 당시 구현
순서·결정의 역사적 기록이다. 그 문서의 hidden/pending 상태를 현재 배포 상태로 해석하지 않는다.
제품 실행/installer 근거와 deterministic fixture·browser·physical device 검사는 구분한다.

지원 번들의 `operations`에는 같은 설치의 네 제품 운영 로그 요약이 들어간다.
로그는 각 제품 데이터 폴더의 `logs/`에 UTC 날짜별로 기록하고 14일 보관한다.
실패·취소·거부·panic과 250ms 이상 걸린 성공만 기록하며 원문 인자·본문·경로는 포함하지 않는다.
portable 제품처럼 설치 접미사가 다르면 다른 제품 로그는 `missing`으로 표시된다.
