# AGENTS.md

Devbox는 Windows 11 + WSL의 개인 개발 허브다. Rust 데몬이 상태·작업을 소유하고 단일 Windows Tauri 앱이 화면·트레이·단축키·알림을 맡는다.

- 공통 규약의 원장은 [CONVENTIONS.md](CONVENTIONS.md)다. 변경 전 §1·3·5·8·9·11과 대상 디렉터리 지침을 읽는다.
- 설계: [01-design](docs/superpowers/plans/2026-10-08-devbox-v1/01-design.md), 실행: 같은 폴더의 00-roadmap.md, 진행 유일 원장: PROGRESS.md.
- 사용자 지시와 승인된 미완료 계획만 구현한다. 과제마다 구현 후 별도 검토하며 하위 에이전트는 GPT-6.1 Sol/GPT-6 Astra만 사용한다.
- pnpm 9, Rust 사용 전 `source ~/.cargo/env`; 실제 Windows 앱 실행·배포 빌드는 Windows에서만 한다.
- 과제는 좁은 실패 테스트부터 검증한다. 전체 check·clippy·build·E2E는 PR 끝에 한 번, 완료 커밋·push 전 Biome CI를 실행한다.
- 과제별 영어 Conventional Commit, 최종 상세 검증 뒤 push·PR 한 번. main squash만, Frontend/Linux Rust/Windows Rust 필수 CI 성공과 보호 규칙을 유지한다.
- Cargo jobs 2·workers 2·test threads 2, CPU 4·nice +10, 지원 cgroup 6GiB/8GiB·swap 1GiB와 Git common-dir 비차단 잠금(exit 75)을 따른다.
- 기존 서비스·Docker·방화벽·공유 네트워크를 테스트로 변경하지 않는다. 소유 자원만 정리하고 공유 cache·사용자 자료·활성/dirty/호스트 소유 worktree는 보존한다.
- 실패·미실행 Windows 실기·수동 유보는 PASS로 보고하지 않는다. PROGRESS와 PR·검증 artifact에 근거를 기록한다.
