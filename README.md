# devbox

Windows 11 + WSL에서 여러 프로젝트를 AI 에이전트와 함께 열고·돌리고·지켜보고·기록하는 개인 개발 허브.

- 상태: v1.0.0 재구축 S0b Task 1 골격. 앱과 데몬 실행 기능은 후속 과제에서 구현한다.
- 이전 소스: [v0.9.0 태그](https://github.com/jihoon22-lee/devbox/tree/v0.9.0). 공개 상태는 GitHub Release에서 확인한다.
- 설계: [01-design](docs/superpowers/plans/2026-10-08-devbox-v1/01-design.md)
- 진행 원장: [PROGRESS](docs/superpowers/plans/2026-10-08-devbox-v1/PROGRESS.md)
- 공통 규약: [CONVENTIONS](CONVENTIONS.md)

## 개발

현재 사용 가능한 검사: `python3 scripts/tests/test_scaffold.py`, `bash scripts/banned-words.sh`.
Rust 검사는 `source ~/.cargo/env` 뒤 공통 자원 supervisor로 `cargo test -p devbox-protocol --lib`를 실행한다.
`pnpm install`은 Biome만 설치한다. `pnpm check`·`pnpm dev`·`pnpm gen`과 Windows 개발 스크립트는 후속 과제에서 구현한다.
실제 Windows 앱 실행·배포 빌드는 Windows에서 한다.
