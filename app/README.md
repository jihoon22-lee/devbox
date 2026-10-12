# Devbox 화면

저장소 루트에서 `pnpm dev`를 실행한다. Rust 빌드만 공유 자원 제한·잠금 아래 실행하며,
그 뒤 dev 데몬에 Hello로 연결하거나 소유 포그라운드 데몬을 시작한다.
화면은 `http://localhost:1450`, gateway는 1451이다. 기존 서비스는 변경하지 않는다.
종료 시 이번 실행이 시작한 자식 프로세스만 정리한다.

`pnpm --filter app e2e`는 같은 supervisor 아래 Chromium 시나리오를 실행한다.
먼저 `pnpm --filter app exec playwright install chromium`으로 브라우저를 준비한다.
1460·1461 포트를 사용하며 테스트마다 임시 HOME/XDG와 고유 인스턴스를 만들고 정리한다.
점유 포트의 프로세스를 종료하거나 재사용하지 않는다. 실패 로그·trace와 성공한 프로젝트 여정의 MainFrame·전환기·제거 확인 화면은 `test-results/`에 남는다.
CI의 `e2e-diagnostics` 산출물에서도 확인한다.

전체 검사는 PR의 모든 과제를 마친 뒤 `pnpm check`로 한 번 실행한다.
독립 검사의 실패를 모으며 내부 E2E는 잠금을 다시 획득하지 않는다.
다른 worktree의 검증이 진행 중이면 exit 75로 종료한다.
자원 사용 근거는 Git common-dir의 `devbox-verification/latest.json`에 남는다.

Windows에서 직접 확인할 설치 파일은 WSL에서 다음 명령으로 받는다. 원격에 push된 브랜치를 지정한다.

```bash
bash scripts/dogfood.sh v1/s0b-skeleton
```

GitHub CLI 인증이 필요하다. 정확한 원격 SHA의 `windows.yml` PR 실행을 먼저 찾고, 실행이 없으면
기본 브랜치에 워크플로가 있는지 확인한 뒤 수동 실행한다. 실패한 실행은 다운로드하지 않는다.
실제 Windows Downloads 위치에 SHA·run ID·고유 접미사가 있는 새 폴더를 만들며 기존 자료는 보존한다.
설치 파일의 SHA256과 `receipt.json`의 원본 SHA·run ID·저장소·워크플로를 검사하고 경로를 출력한다.
Windows 쪽 Rust·MSVC·pnpm 없이 이 NSIS 설치 파일을 사용할 수 있다. public RC나 Release는 만들지 않는다.
최종 변경의 필수 CI 성공 근거를 확인한 뒤 PR을 squash 머지한다. 실사용은 사용자가 원할 때 하며 개발·머지의 대기 조건이 아니다.

Windows 개발 도구가 이미 준비된 환경에서는 WSL 저장소의 스크립트를 PowerShell에서 호출한다.
Node 24·pnpm 9·Rust 1.98.1·MSVC 및 Windows SDK가 필요하며 스크립트가 전역 도구를 설치하지 않는다.
WSL에는 musl target과 linker가 준비돼 있어야 한다. 다음 경로는 실제 WSL 배포판·저장소 위치에 맞춘다.

```powershell
& "\\wsl.localhost\Ubuntu\home\jihoon\projects\devbox-wt\v1-s0b-skeleton\scripts\win-dev.ps1" -Distro Ubuntu -WinRoot C:\dev\devbox-s0b
& "\\wsl.localhost\Ubuntu\home\jihoon\projects\devbox-wt\v1-s0b-skeleton\scripts\win-build.ps1" -Distro Ubuntu -WinRoot C:\dev\devbox-s0b
```

`-WinRoot`을 생략하면 LocalAppData 아래에 고유한 빌드 폴더를 만든다. 지정한 폴더가 이미 있다면
동일한 원본·목적지에 결합된 `.devbox-win-owner.json`이 있어야 한다. 소유 표시가 없는 기존 폴더,
드라이브·프로필 루트·UNC 목적지·경로 중첩·junction은 거부한다. `.git`·`node_modules`·`target`·`dist`는
동기화에서 제외하고 Windows 의존성은 frozen lockfile로 설치한다.
WSL 빌드는 공유 supervisor 제한·잠금을 따르고 Cargo metadata에서 실제 musl 바이너리 위치를 읽는다.
Windows Cargo 산출물은 항상 소유 트리의 `app/src-tauri/target`에 둔다.
빌드는 실제 리소스가 실행 파일 옆에 있는지 확인하고 `Devbox.exe --self-check` 종료 코드를 기다린 뒤
`app/src-tauri/target/release/bundle/nsis`의 설치 파일 경로를 출력한다.

전체 WSL 검사는 workspace와 별도로 앱 자체 Cargo.lock의 fmt·순수 core 테스트도 포함한다.
Windows 네이티브 fmt·clippy·테스트·NSIS 빌드·self-check는 `Rust (Windows)` CI가 실행한다.
WSL 검사 성공은 Windows 네이티브 실행 성공을 뜻하지 않는다.
