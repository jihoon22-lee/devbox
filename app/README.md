# Devbox 화면

저장소 루트에서 `pnpm dev`를 실행한다. Rust 빌드만 공유 자원 제한·잠금 아래 실행하며,
그 뒤 dev 데몬에 Hello로 연결하거나 소유 포그라운드 데몬을 시작한다.
화면은 `http://localhost:1450`, gateway는 1451이다. 기존 서비스는 변경하지 않는다.
종료 시 이번 실행이 시작한 자식 프로세스만 정리한다.

`pnpm --filter app e2e`는 같은 supervisor 아래 Chromium 시나리오를 실행한다.
먼저 `pnpm --filter app exec playwright install chromium`으로 브라우저를 준비한다.
1460·1461 포트를 사용하며 테스트마다 임시 HOME/XDG와 고유 인스턴스를 만들고 정리한다.
점유 포트의 프로세스를 종료하거나 재사용하지 않는다. 실패 로그·trace는 `test-results/`에 남는다.

전체 검사는 PR의 모든 과제를 마친 뒤 `pnpm check`로 한 번 실행한다.
독립 검사의 실패를 모으며 내부 E2E는 잠금을 다시 획득하지 않는다.
다른 worktree의 검증이 진행 중이면 exit 75로 종료한다.
자원 사용 근거는 Git common-dir의 `devbox-verification/latest.json`에 남는다.
