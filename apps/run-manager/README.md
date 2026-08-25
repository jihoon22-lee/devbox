# run-manager — Run Manager (예약 실행·서비스 관리)

예약 실행(크론 잡)과 상시 실행(서비스)을 한 곳에서 관리하는 앱.
산출물: `Run Manager.exe` (`apps/run-manager`).

## 주요 기능

- **작업(cron job)** — 이름, 명령, 작업 디렉터리, 환경변수, 실행 대상(Windows/WSL 배포판), cron 빌더 + 다음 실행 시각 미리보기
- **실행 정책** — 중복 실행 정책(skip/queue/kill-previous), occurrence 원자적 claim
- **서비스(service)** — start/stop/restart·자동 시작·재시작 정책(never/on-failure/always)·백오프·헬스체크(프로세스 생존/로컬 TCP)
- **관찰성** — 실행 이력, stdout/stderr 회전 로그 tail, 실패 Windows toast 알림
- **행 컨텍스트 메뉴** — 작업·서비스·실행 이력의 우클릭/Shift+F10/Menu key 메뉴, 대상 행 우선 선택, 닫힌 뒤 focus 복구
  - 작업: 지금 실행, 활성화/비활성화, 편집, 로그 열기, 확인 후 삭제
  - 서비스: 실제 초기 인스턴스 상태에 따른 시작/정지/재시작, 편집, 정지 상태에서만 확인 후 삭제
  - 실행 이력: 로그 보기, 해당 작업 재실행, 현재 stdout/stderr 스트림 로그 저장
- **안전한 lifecycle action** — 작업 활성 실행과 서비스 정지는 모든 UI 진입점에서 확인하고, 활성 작업 삭제와 정지 중이거나 snapshot을 확인할 수 없는 서비스 lifecycle 변경은 fail-closed로 비활성화
- **재시도 제어** — `retry_waiting` 서비스의 명시적 정지는 예약된 backoff를 취소하고, 재시작은 대기 시간을 건너뛰어 새 generation을 시작
- **제한된 로그 저장** — backend가 run ID로 해석한 app-owned 회전 로그만 decimal cursor로 읽고, 현재 스트림을 최대 50MiB까지 저장. 파일명에는 bounded opaque run ID만 사용하며 명령·경로·환경변수를 넣지 않음
- **실행 어댑터** — Windows(Job Object)·WSL(session/group), DPAPI 환경변수 보호

## 기술

- 공용 크레이트 `crates/wsl`·`crates/secrets`·`crates/integration`, 프로세스/서비스 실행은 자체 구현(`src-tauri/src/platform/`)
- 트레이 상주 + 백그라운드 tokio 루프 + SQLite
- `packages/diff-view`·`packages/context-menu`

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: `docs/superpowers/specs/2026-08-12-run-manager-design.md`
