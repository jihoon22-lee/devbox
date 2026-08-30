# Devbox Launcher

Devbox Launcher 0.1.1은 `Ctrl+Alt+Space`로 여는 일시적 검색 창이다. 설정에서
`Ctrl+Alt+L` 또는 `Ctrl+Alt+J`로 바꿀 수 있으며, 변경된 단축키는 저장 후 즉시 다시
등록된다. 기본 키를 다른 프로그램이 점유했거나 현재 플랫폼에서 등록할 수 없으면 창을
숨기지 않고 고정 상태와 대체 키를 안내한다.

Launcher는 build-time `apps/catalog.json`의 앱과, 존재할 때만 다음 5개 versioned integration
snapshot source를 검색한다. producer의 source DB를 직접 열지 않고, 공용 bounded reader로
named sidecar 또는 기존 summary snapshot만 읽는다. `jobs-services.json` sidecar와 그 fallback은
#474를 닫은 #479를 통해 v0.5.1 stable에 포함됐으며, 공개 v0.5.0 binary에는 없다.

- Workbench `workbench/v1/profiles.json` — `profiles/v1` named view
- Repo Manager `repo-manager/v1/repositories.json` — `repositories/v1` named view
- Run Manager `run-manager/v1/jobs-services.json` — `jobs-services/v1` named view;
  named sidecar가 없을 때만 `summary.json` flat fallback (#479/#474)
- Everything+ `everything-plus/v1/summary.json` — `saved-queries/v1` view
- WSL Desktop `wsl-desktop/v1/profiles.json` — `profiles/v1` named view

`crates/integration`의 bounded reader가 versioned path를 읽고 entry의 target, payload version,
path/query/id를 검증한다. 각 catalog/snapshot entry에는 정확한 entry 직렬화 값에서 계산한
SHA-256 `revision`이 붙으며, preview와 launch는 그 exact revision을 다시 전달한다. entry가
rename/change되거나 제거되면 ID·revision 재검증에서 거부된다. stale snapshot은 사용자가 명시적으로
계속하기로 확인한 경우에만 현재 source를 다시 검증해 전달한다. source 상태는 `missing`,
`corrupt`, `permission`, `linked`를 구분해 다른 source 검색을 계속하며, 대상 앱이 설치되지
않았으면 `--install-app <id>` AppLink로 Devbox Manager의 설치 화면만 연다.

Workbench·WSL Desktop profile와 Repo Manager repository named producer는 primary CRUD/scan/
worktree 결과가 성공한 뒤 snapshot publication을 best-effort로 시도한다. Launcher는 이
publication 결과를 source DB 대신 named snapshot contract로만 소비한다. 즐겨찾기와 최근 실행은
bounded opaque result ID만 앱 전용 `launcher-preferences.json`에 저장하며 raw path, query,
payload, secret은 저장하지 않는다.

Workbench의 `Path`/`Profile`, WSL Desktop의 `Path`/`Profile`, Run Manager의 `Task`, Devbox
Manager의 `Install`은 모두 수신 앱이 현재 저장 상태나 embedded catalog를 다시 확인한다. Run
Manager는 task를 자동 실행하지 않고 확인 대화상자를 먼저 표시하며 Cancel이 기본 focus다.
지원하지 않는 새 target을 받은 기존 앱은 명시적으로 no-op/error 처리한다.

기존 Life Log→Knowledge `knowledge-draft/v1` capability/action은 구조화 handoff 계약으로
그대로 유지한다. Launcher는 이를 clipboard text action으로 변환하거나 노출하지 않는다.
catalog revision 15의 static `transform-text` action은 사용자가 확인한 현재 selection만
Developer Toolbox로 `toolbox-text/v1` one-time masked handoff로 전달한다. selection은 launch
직전에 다시 검증하며, 이 W07 경로는 clipboard fallback을 사용하지 않는다. credential 형태의
줄은 공용 경계에서 먼저 마스킹하고 argv·설정·이력에 원문을 남기지 않는다. 별도
`Clipboard 미리보기`는 명시적으로 고른 때에만 현재 selection, 그 다음 clipboard를 읽어 modal에
표시하며 handoff나 저장으로 연결하지 않는다.

범용 file/web/Windows settings 검색, arbitrary shell, clipboard history, PowerToys plugin host는
제공하지 않는다. `src-tauri/src/core/launcher.rs`가 mutation 없는 bounded index를 소유하고,
`src-tauri/src/commands.rs`가 매 요청의 재검증과 launch 경계를 소유한다. 실제 Windows
`RegisterHotKey`, focus-loss hide, cold/hot AppLink와 packaged installer 동작은 v0.5.0 W3/W4
Windows checkpoint에서 검증한다. W07 Windows 실기/packaged acceptance 완료를 이 문서는
주장하지 않는다.
