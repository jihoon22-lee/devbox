# devbox

Windows 11 + WSL에서 여러 프로젝트를 AI 에이전트와 함께 열고·돌리고·지켜보고·기록하는 개인 개발 허브.

- 상태: v1.0.0 재구축 S0b. 프로젝트 관리·WSL 데몬·Windows 앱 연결을 구현했으며, 최종 검증과 사용자 S0 확인은 [진행 원장](docs/superpowers/plans/2026-10-08-devbox-v1/PROGRESS.md)을 따른다.
- 이전 소스: [v0.9.0 태그](https://github.com/jihoon22-lee/devbox/tree/v0.9.0). 공개 상태는 GitHub Release에서 확인한다.
- 설계: [01-design](docs/superpowers/plans/2026-10-08-devbox-v1/01-design.md)
- 진행 원장: [PROGRESS](docs/superpowers/plans/2026-10-08-devbox-v1/PROGRESS.md)
- 공통 규약: [CONVENTIONS](CONVENTIONS.md)

## 개발

WSL에서 Node 24·pnpm 9·Rust 1.98.1을 사용한다.

```bash
source ~/.cargo/env
pnpm install --frozen-lockfile
pnpm dev
```

화면은 `http://localhost:1450`에서 연다. `pnpm dev`는 기존 dev 데몬에 연결하거나 이번 실행 소유 데몬을 시작한다.
`pnpm gen`은 RPC 타입을 생성한다. 전체 검사는 모든 변경을 마친 뒤 `pnpm check`로 한 번 실행하며,
E2E 브라우저는 `pnpm --filter app exec playwright install chromium`으로 준비한다.

실제 앱 실행·설치 파일 빌드는 Windows에서 한다. PowerShell의 `scripts/win-dev.ps1`·`scripts/win-build.ps1`은
WSL 바이너리를 빌드하고 소유한 Windows 작업 폴더에서 Tauri를 실행한다. 로컬 Windows 도구가 없으면
PR의 Windows CI 설치 파일을 `bash scripts/dogfood.sh`로 받는다. 정확한 커밋·실행·파일 해시를 확인하고
Windows Downloads 아래 새 폴더에 저장하며, 설치는 자동 실행하지 않는다.

Claude Code MCP 등록 예시: `claude mcp add devbox -- ~/.local/share/devbox/bin/devbox mcp`.
설정 폴더(`~/.config/devbox`, accept는 `~/.config/devbox-accept`)의 빈 파일 `.config.toml.lock`은 `config.toml` 동시 쓰기 잠금이므로 지우지 말고, 그 폴더를 dotfiles로 Git 관리하면 ignore에 넣는다.

필수 도구·상세 명령·검증 범위는 [앱 개발 안내](app/README.md)를 따른다.
