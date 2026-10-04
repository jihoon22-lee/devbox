# Devbox Workspace

v0.9.0 소스의 네 사용자 제품 중 하나다. Overview·Source·Files·Dependencies·Sessions·Tasks & Services·Runtime·Logs·Problems·Terminal를 통합한다.
현재 구현·검증은 [재정비 계획](../../docs/superpowers/plans/2026-10-03-product-readiness/00-roadmap.md)과 통합 PR, 공개 상태는 [릴리스 근거](../../docs/release-evidence.md)와 GitHub Release에서 확인한다. #580은 철회본의 닫힌 기록이다.

## 실행과 개발

- `pnpm --filter devbox-workspace dev`: 명시적으로 표시된 browser fixture.
- Windows의 `pnpm --filter devbox-workspace tauri dev`: 실제 native 제품.
- Browser `?route=overview`와 debug `--route=overview`는 개발용 route 선택이다.
  release 권한을 부여하는 옵션이 아니며 native host가 caller·session·route를 다시 검사한다.
- 제품 identity는 `com.devbox.v08.workspace`, 데이터는 installation별 namespace다.
  처음 실행하면 프로젝트 저장소를 자동으로 준비한다. 오류가 나면 다시 시도할 수 있다.
- 배포는 Suite installer 또는 제품 ZIP 전체를 사용한다. [설치·복구 안내](../../docs/windows-guide.md).

## 유지하는 계약

- Windows/WSL project·worktree·target identity, repository manifest/local overlay와 trust review를 유지한다.
- Files는 standalone file과 프로젝트 파일 모두 지원하며 dirty/recovery/revision을 보존한다.
  Windows managed LSP와 WSL component는 각 target의 실행·환경·경로 권한을 다시 검사한다.
- Session과 Runtime은 자신이 시작한 task/service/terminal만 제어하고 외부 resource를 보존한다.
  Port→Log→Problem→File 이동은 검토된 context/opaque reference를 전달한다.
  터미널 로그 요청의 일시적 조회 실패 안내는 후속 조회가 정상 완료되면 해제한다.
- Terminal 보조 창은 hide/reload와 종료를 구분하며 tabs/panes/exact layout/cwd·tmux/Zellij를 유지한다.
  종료 중에는 마지막 레이아웃을 고정하고, 이전 창에서 늦게 도착한 저장 요청은 복원된 새 창에 적용하지 않는다.
- 프로젝트 등록·선택·관리는 Overview에서 한다. 다른 화면은 현재 context와 해당 작업을 표시하며,
  프로젝트가 없으면 Overview의 선택 화면으로 이동할 수 있다. 저장소 준비 실패·재시도는 어느 화면에서나
  표시한다. Source에서 생성한 작업 폴더의 등록 검토도 Overview로 이동하며 native 검증·명시 선택을 유지한다.
- Overview의 원본 snapshot·profile/template/session/LSP/window-state import는 명시적 검토와
  source 재검증을 거쳐 적용한다. 원본 Git/worktree 파일을 복사하거나 자동 실행하지 않는다.

## 구현과 근거

기능 코드는 이 제품 host와 `packages/` feature UI, 이름을 가진 `crates/` engine에 있다.
기존 15개 앱의 standalone shell·Tauri bootstrap은 제거했다. 현재 소유권은
[projects](../../docs/projects.md), 기능/데이터 및 R01–R26/S01–S08 대응은
[acceptance trace](https://github.com/jihoon22-lee/devbox-archive/blob/main/docs/v0.8-acceptance.md)를 참조한다.

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

설치본의 작업·서비스·예약·Development Sessions 실행은 agent가 소유한다. 창을 닫아도 실행은 유지되며 재열기 시 기존 receipt와 lease로 복구한다. agent가 종료되어 lease가 사라진 세션은 Degraded로 표시하고 자동 재실행하지 않는다. 터미널·편집기·LSP는 Workspace에 남는다. 로그인 자동 시작은 Control Center 환경에서 설정한다.

### 초안·실행 복구 수용

Files는 dirty 본문(빈 본문 포함)과 encoding/BOM/line ending을 native 복구 저장소에 기록한다.
‘복구 내용을 기록했습니다’ 이후의 마지막 확인된 snapshot이 crash 복구 경계다. 복구는
검토한 내용을 dirty 편집 버퍼로 열며 일반 저장 또는 명시 폐기까지 복구 기록을 유지한다.
옛 기록은 원본 파일의 저장 형식을 다시 읽고, 원본을 읽을 수 없으면 기록을 보존한 채
파일 접근과 인코딩을 먼저 확인한다. main X는 종료 검토를 열고 저장·폐기·취소를 구분한다.
Source 초안은 context 전환과 정상 종료를 보호한다. Source crash 복원은 제공하지 않는다.

Windows 수용은 임시 소유 namespace에서 `WORK-01/02/03` UI 모듈로 정상 종료 취소,
확인된 복구 snapshot의 crash/reopen, Source 초안의 Agent 전환 차단, 새 Agent worktree의
등록·선택·정리를 확인한다. 격리 WSL fixture와 native 창/process 관찰 adapter가 없으면
`NOT_RUN`으로 남기며 WSL frontend/unit PASS를 실제 Windows PASS로 대체하지 않는다.

Runtime 요청 복구에서 대상 실행 상태를 먼저 열고 요청 기록을 정리한다. 기록 정리는
프로세스 중단이 아니다. Terminal은 native 열림 성공 뒤 목록 실패를 구분하고 상태 조회와
동일 요청 확인을 분리한다. 세션 시작 결과가 불확실하면 ‘세션 상태 확인’으로 기존 세션을
조회한다. Dependencies의 lock 변경은 다시 분석→전송 내용 검토→명시 승인 순으로 처리한다.
기본 분석은 로컬 lockfile을 읽으며 패키지를 설치하거나 제거하지 않는다.
