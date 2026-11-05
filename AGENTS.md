# AGENTS.md

Devbox는 Windows 11 + WSL의 개인 개발 허브다. Rust 데몬이 상태·작업을 소유하고 단일 Windows Tauri 앱이 화면·트레이·단축키·알림을 맡는다.

- 공통 규약의 원장은 [CONVENTIONS.md](CONVENTIONS.md)다. 변경 전 §1·3·5·8·9·11과 대상 디렉터리 지침을 읽는다.
- 규칙 [CONVENTIONS.md](CONVENTIONS.md) · 현재 [PROGRESS](docs/superpowers/plans/2026-10-08-devbox-v1/PROGRESS.md)(진행 유일 원장) · 실행 [10-s7-s8-release](docs/superpowers/plans/2026-10-08-devbox-v1/10-s7-s8-release.md)(끝난 계획은 [docs/history.md](docs/history.md)). 설계는 [01-design](docs/superpowers/plans/2026-10-08-devbox-v1/01-design.md).
- 사용자 지시와 승인된 미완료 계획만 구현한다. 실행 하네스가 제공하는 하위 에이전트를 사용한다(과제마다 구현 → 별도 검토). 하위 에이전트가 없는 하네스는 같은 순서(구현 → 새 맥락에서 검토 → 커밋)를 직접 지킨다. 특정 모델만 쓰라는 규칙은 없다(사용자 결정 2026-10-13, 2026-10-14 재확인).
- pnpm 9, Rust 사용 전 `source ~/.cargo/env`; 실제 Windows 앱 실행·배포 빌드는 Windows에서만 한다.
- 과제는 좁은 실패 테스트부터 검증한다. 전체 check·clippy·build·E2E는 PR 끝에 한 번, 완료 커밋·push 전 Biome CI를 실행한다.
- 과제별 영어 Conventional Commit, 최종 상세 검증 뒤 push·PR 한 번. main squash만, Frontend/Linux Rust/Windows Rust 필수 CI 성공과 보호 규칙을 유지한다.
- 작업 브랜치는 백업으로 push해도 PR은 그 작업의 마무리 과제에서 한 번만 연다(하네스의 push 뒤 PR 생성 기본값보다 우선). 1.0.0 공개(S8)의 버전·문서 변경은 `v1/s8-release`와 [계획 10](docs/superpowers/plans/2026-10-08-devbox-v1/10-s7-s8-release.md)으로 하고, S8-6 기록 PR(`docs/v1.0.0-record`)과 그 뒤 작업은 이 모델을 따른다. PR 리뷰는 CONVENTIONS §8(사용자 결정 2026-10-27)을 따른다: PR을 연 뒤 10분 안에 자동 리뷰 요약 댓글(한도 안내 제외)이 오면 켜짐이라 최대 3회 반영하고(4회째부터는 실제 데이터 손실·비밀 유출·승인하지 않은 실행만 고치고 나머지는 PROGRESS §4 열린 후속에 적는다), 아니면 꺼짐이라 하네스의 독립 코드·보안 검토를 PR마다 한 번만 한다.
- Cargo jobs 2·workers 2·test threads 2, CPU 4·nice +10, 지원 cgroup 6GiB/8GiB·swap 1GiB와 Git common-dir 비차단 잠금(exit 75)을 따른다.
- 기존 서비스·Docker·방화벽·공유 네트워크를 테스트로 변경하지 않는다. 소유 자원만 정리하고 공유 cache·사용자 자료·활성/dirty/호스트 소유 worktree는 보존한다.
- 실패·미실행 Windows 실기·수동 유보는 PASS로 보고하지 않는다. PROGRESS와 PR·검증 artifact에 근거를 기록한다.
