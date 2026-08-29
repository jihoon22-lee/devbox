# Devbox Launcher

Devbox Launcher 0.1.1은 `Ctrl+Alt+Space`로 여는 일시적 검색 창이다. 설정에서
`Ctrl+Alt+L` 또는 `Ctrl+Alt+J`로 바꿀 수 있으며, 변경된 단축키는 저장 후 즉시 다시
등록된다. 기본 키를 다른 프로그램이 점유했거나 현재 플랫폼에서 등록할 수 없으면 창을
숨기지 않고 고정 상태와 대체 키를 안내한다.

Launcher는 build-time `apps/catalog.json`의 앱과, 존재할 때만 다음 versioned integration
snapshot path를 검색한다. 이 목록은 consumer-side 계약이며, bootstrap 자체가 모든 producer를
구현하거나 catalog에 등록한다는 의미가 아니다. `jobs-services.json` sidecar와 그 fallback은
#474를 닫은 #479를 통해 v0.5.1 stable에 포함됐으며, 공개 v0.5.0 binary에는 없다.

- Workbench `profiles/v1` (후속 producer)
- Repo Manager `repositories/v1` (후속 producer)
- Run Manager `jobs-services/v1` (`jobs-services.json` sidecar; `summary.json` flat fallback; #479/#474)
- Everything+ `saved-queries/v1`
- WSL Desktop `profiles/v1` (후속 profile producer)

`crates/integration`의 bounded reader가 versioned path를 읽고 entry의 target, payload version,
path/query/id를 검증한다. path가 없으면 `missing`, 오래되거나 손상됐거나 읽을 권한이 없으면
각각 독립 상태로 격리해 다른 source 검색을 계속한다. 사용자가 결과를 실행하는 순간 같은
검증을 다시 수행하며 대상 앱이 설치되지 않았으면 `--install-app <id>` AppLink로 Devbox
Manager의 설치 화면만 연다.

Workbench의 `Path`/`Profile`, WSL Desktop의 `Path`/`Profile`, Run Manager의 `Task`, Devbox
Manager의 `Install`은 모두 수신 앱이 현재 저장 상태나 embedded catalog를 다시 확인한다. Run
Manager는 task를 자동 실행하지 않고 확인 대화상자를 먼저 표시하며 Cancel이 기본 focus다.
지원하지 않는 새 target을 받은 기존 앱은 명시적으로 no-op/error 처리한다.

기존 Life Log→Knowledge `knowledge-draft/v1` capability/action은 구조화 handoff 계약으로
그대로 유지한다. Launcher는 이를 clipboard text action으로 변환하거나 노출하지 않는다.
Developer Toolbox의 `toolbox-text/v1` action은 실제 claim/ack receiver가 준비될 때까지
노출하지 않는다. 대신 사용자가 `Clipboard 미리보기`를 명시적으로 고른 때에만 현재 selection,
그 다음 clipboard를 읽어 modal에 표시하며 전달·저장하지 않는다.

범용 file/web/Windows settings 검색, arbitrary shell, clipboard history, PowerToys plugin host는
제공하지 않는다. `src-tauri/src/core/launcher.rs`가 mutation 없는 bounded index를 소유하고,
`src-tauri/src/commands.rs`가 매 요청의 재검증과 launch 경계를 소유한다. 실제 Windows
`RegisterHotKey`, focus-loss hide, cold/hot AppLink와 packaged installer 동작은 v0.5.0 W3/W4
Windows checkpoint에서 검증한다.
