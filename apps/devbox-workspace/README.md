# Devbox Workspace

v0.8의 네 사용자 제품 중 하나다. Overview·Source·Files·Dependencies·Sessions·Tasks & Services·Runtime·Logs·Problems·Terminal를 통합한다.
B01~B08의 owner 수용은 완료됐고, 최종 B09 후보·공개 결과는 [#541](https://github.com/jihoon22-lee/devbox/issues/541)에 기록한다.

## 실행과 개발

- `pnpm --filter devbox-workspace dev`: 명시적으로 표시된 browser fixture.
- Windows의 `pnpm --filter devbox-workspace tauri dev`: 실제 native 제품.
- Browser `?route=overview`와 debug `--route=overview`는 개발용 route 선택이다.
  release 권한을 부여하는 옵션이 아니며 native host가 caller·session·route를 다시 검사한다.
- 제품 identity는 `com.devbox.v08.workspace`, 데이터는 installation별 namespace다.
  시작만으로 legacy source를 초기화하거나 원본 경로에 새 데이터를 쓰지 않는다.
- 배포는 Suite installer 또는 제품 ZIP 전체를 사용한다. [설치·이전·복구 안내](../../docs/windows-guide.md).

## 유지하는 계약

- Windows/WSL project·worktree·target identity, repository manifest/local overlay와 trust review를 유지한다.
- Files는 standalone file과 프로젝트 파일 모두 지원하며 dirty/recovery/revision을 보존한다.
  Windows managed LSP와 WSL component는 각 target의 실행·환경·경로 권한을 다시 검사한다.
- Session과 Runtime은 자신이 시작한 task/service/terminal만 제어하고 외부 resource를 보존한다.
  Port→Log→Problem→File 이동은 검토된 context/opaque reference를 전달한다.
- Terminal 보조 창은 hide/reload와 종료를 구분하며 tabs/panes/exact layout/cwd·tmux/Zellij를 유지한다.
  종료 중에는 마지막 레이아웃을 고정하고, 이전 창에서 늦게 도착한 저장 요청은 복원된 새 창에 적용하지 않는다.
- Overview의 원본 snapshot·profile/template/session/LSP/window-state import는 명시적 검토와
  source 재검증을 거쳐 적용한다. 원본 Git/worktree 파일을 복사하거나 자동 실행하지 않는다.

## 구현과 근거

기능 코드는 이 제품 host와 `packages/` feature UI, 이름을 가진 `crates/` engine에 있다.
기존 15개 앱의 standalone shell·Tauri bootstrap은 제거했다. 현재 소유권은
[projects](../../docs/projects.md), 기능/데이터 및 R01–R26/S01–S08 대응은
[acceptance trace](../../docs/v0.8-acceptance.md)를 참조한다.

[이전 개발 단계의 상세 README](https://github.com/jihoon22-lee/devbox-archive/blob/main/docs/history/v0.8-development/workspace.md)는 당시 구현
순서·결정의 역사적 기록이다. 그 문서의 hidden/pending 상태를 현재 배포 상태로 해석하지 않는다.
제품 실행/installer 근거와 deterministic fixture·browser·physical device 검사는 구분한다.

Owned Runtime 작업의 관찰·종료는 보존한 distro/executable identity를 매번 확인하며,
프로젝트 경로 이동이나 filesystem helper 종료로 차단되지 않는다. 새 실행은 기존
project/source/target 승인을 계속 요구한다.

### Review corrections (2026-09-21)

Source commit confirmation reads a native Git index/HEAD witness. The approval includes that
revision; execution rejects external add/reset, changed staged blobs (including the same path),
or a changed HEAD until the user reviews again. Existing repository identity, Git trust and
cancellation checks still apply to both Windows and the WSL helper. The witness is checked
immediately before the Git command; it is an optimistic check, not a lock on arbitrary external
Git writers or trusted hooks after that check.

Logs accepts a source-owned, bounded/redacted Webhook projection from API Studio through the
approved Suite peer and explicit incoming review. Claims bind artifact, revision and operation;
retries of that operation are idempotent, other claims/replays are refused. Failed delivery
preserves existing Logs and the API source. These transient captures are not saved as file paths.

WSL stop preserves the supervisor through TERM, validates live marked group membership again
before KILL, and passes the marker through private stdin. Unreaped zombies do not block live
resource cleanup. Noisy/failed probes cannot establish success or prevent a freshly authorized
KILL. Normal leader probes are constant-time; a retired leader requires a bounded-frequency
process scan to find remaining marked descendants. Stop delivery and observation share bounded
phase budgets (six seconds total for the default two-second grace). Actual Windows/WSL startup
latency and process behavior require the Windows acceptance fixture; local bash tests alone are
not Windows evidence.
