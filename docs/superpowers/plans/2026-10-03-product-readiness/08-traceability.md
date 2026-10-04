# 감사·추가 개선·검증 추적표

2026-10-03 실행 중. R01–R14의 주요 수정과 좁은 로컬 회귀를 아래 작업별 근거에 연결했다. Windows packaged 사용자 여정 runner와 전체 gate는 R15에서 연결했으며, 후보 실행 전 상태는 **NOT_RUN**이다. 구현·로컬 PASS를 출시 수용 PASS로 해석하지 않는다. 최종 PR·candidate·release 결과는 같은 통합 PR 본문과 Actions artifact에 추가한다.

코드 경로는 현재 `/home/jihoon/projects/devbox` 기준이다. 다른 checkout에서는 저장소 상대 위치를 그대로 사용한다. 상세 변경 파일과 assertion은 소유 작업 문서에 있다.

### 후보 검증 후속 보정

통합 PR #616 이후 출시 차단 수정은 최소 보정 PR에 묶는다. #617은 Windows 파일명 충돌,
#618은 commit/passive-effect 사이 작업 시작과 Transform 명시 저장의 수명 경계를 수정했다.
후보 `37126573659`는 네 제품과 7개 자산 조립, product-shells·Knowledge·cross-product·WSL2를
통과했지만 API 응답 선택과 installer 준비에서 실패하여 승격 대상이 아니다.

- API: Windows 진단 `37132833899`에서 `.response`와 `.response-panel` 높이 0,
  본문 높이 28px(패딩만 남음) 및 SSE 영역에 가려지는 화면을 확인했다. 응답 영역이 축소되지 않도록
  수정하고 버튼 줄바꿈을 적용했다. 같은 개발용 Chromium 요청의 수정 전 0px → 수정 후 응답 320px 이상,
  본문 160px 이상을 확인했으며 720×480에서도 본문 선택·포인터 접근 및 헤더 탭을 확인했다.
  이는 browser 레이아웃 근거이고 수정된 Windows 설치본 수용 완료를 뜻하지 않는다.
- Installer: 같은 Actions step 안에서 새 `GITHUB_ENV` 값을 읽어 WSL fixture 소유권 검사에
  실패했다. 생산자와 소비자 step을 분리했다. 이 실패 이전에는 40개 설치 UI 여정이 실행되지 않았다.
- 설치 UI 진단 `37134283547`·`37134661732`에서는 Welcome 창의 활성 `다음 >` 컨트롤(id 1)을
  관찰했지만 UI Automation이 Button 클래스를 Pane(50033)·Invoke 미지원으로 읽었다. 숫자 식별자
  비교만으로는 해결되지 않았다. .NET Framework 기본 proxy 초기화가 PowerShell 동적 호출에서
  `NullReferenceException`을 일으킴을 별도 재현했고, typed C# 진입점으로 기본 provider를 초기화한다.
  Windows의 별도 raw Win32 Button fixture에서 기존 Pane/Invoke 미지원 재현과 수정 후 Button(50000)
  인식·실제 Invoke·소유 marker·정상 종료를 확인했다. 등록 전 실패가 owner receipt 오류에 가려지지
  않도록 원래 오류·단계·소유 창 구조를 보존한다. 설치 완료 근거는 후속 실제 여정에서 확인하며
  초기화 및 fixture 성공만으로 설치 PASS를 기록하지 않는다.
- 후속 진단 `37170395096`은 실제 NSIS Welcome·경로 선택·설치·Finish와 Control Center 연결까지
  진행했다. 당시 일반 CDP 오류로 중단됐으며 `37170824962`에서 첫 화면 캡처는 성공하고 스크롤 뒤
  Workspace 열기의 좌표 hit-test가 실패함을 확인했다. 이후 활성화·40개 여정은 완료되지 않았다.
  GUI 프로세스 호출 뒤 미설정 `LASTEXITCODE`를 읽던 정리 코드도 발견해 정확한 자식 프로세스의
  종료를 기다리도록 보정했고 `37170824962`의 실제 정리는 통과했다.
  실제 Chromium에서도 root scroll 뒤 동일한 hit-test 실패를 재현했다. 검증 driver는 화면 기준
  pointer 좌표와 페이지 기준 DOM hit-test 좌표를 구분하며, 실제 pointer 클릭과 덮인 요소 거절을
  확인했다. 설치 단계 목록 번호가 다음 항목에 붙어 보이는 간격도 기존 토큰을 유지해 수정했다.
- 진단 `37171221250`에서는 실제 Workspace 열기 클릭 뒤 제품 오류가 표시됐다. native가 보관한
  `\\?\D:\…` 형식의 canonical root를 외부 입력 검사에 다시 넘겨 정상 설치를 거절하는 결함을
  확인했다. CapturedScope에서 얻은 로컬 디스크 경로만 일반 형식으로 변환하고 동일 filesystem
  identity를 확인한다. 외부 입력의 device·UNC·verbatim 경로 거절은 유지한다. 순수 Rust 경로 회귀와
  Windows 파일 identity 확인은 통과했으며 실제 수정 바이너리의 설치 실행은 새 후보에서 검증한다.
- 새 설치의 Inventory·복구 이력보다 준비 안내와 다음 동작을 먼저 배치했다. 실제 컴포넌트·CSS와
  합성 native 응답을 사용한 브라우저에서 1024×720·720×480 모두 제품 열기와 준비 후 다음 단계를
  첫 화면에서 확인했다. 명시 확인 전 실행 차단과 복원·재설치·업데이트 조건은 유지한다.
- API 진단 `37170520484`는 요청·응답 선택·Transform 전달·재시작을 통과했다. 진단용 CSS가 fixture의
  페이지 새로고침에서 제거되는 문제를 수정하고 문서별 적용 단계·계산값을 기록했다. 이전 payload에
  현 CSS를 적용한 진단이며 새 배포 bytes 자체의 통과나 최종 후보 수용으로 취급하지 않는다.
- 보관 후보를 재사용한 진단은 runner와 payload source를 구분하고 별도 diagnostic artifact에만
  기록한다. 최종 exact-main 후보의 설치/UI 수용과 승격 근거를 대체하지 않는다.
- #619의 PR CI `37172279567`과 exact-main CI `37172648247`은 모두 통과했다. 새 후보
  `37173572971`(`c91d0326`)은 네 제품·7개 자산, 네 native scope 및 WSL2/Docker를 통과했고,
  실제 설치 뒤 `open_setup_product` 성공으로 경로 수정도 확인했다. 그러나 Control Center를
  정상 종료·재개하는 검증의 창 선택이 실패해 설치 여정과 최종 seal은 실패했다. 출시 가능한 후보가 아니다.
  보관 진단 `37177932549`는 같은 PID 아래 `Tauri Window` 외에 `Tao Thread Event Target`와
  설치 identifier의 `-sic` 제어 창도 native/UIA에서 표시 중으로 보고되는 것을 확인했다.
  단순 visible 필터로 해결되지 않으므로 정확한 제품·보조 창 식별을 보정한다. 실제 두 앱 창이나
  modal의 모호성은 계속 거절하고, 보관 진단의 결과를 새 후보의 승격 근거로 재사용하지 않는다.
- 보관 진단 `37178443869`에서 정확한 보조 창 제외 후 Control Center 종료·재개와 네 제품 실행은
  통과했다. 다음 Workspace 복구 화면에서 요구한 기본 client 크기와 실제 치수가 달라 중단됐다.
  일회성 hosted Windows에만 지원 display mode를 임시 적용하고, 크기·화면·소유 창 측정은
  assertion 전에 보존하도록 보완했다. 기본 1180×780·최소 720×480 수용 기준은 유지한다.
  남은 Workspace 종료 driver 세 곳도 접근성 dialog 이름으로 종료 검토를 식별하도록 맞췄다.
  진단 `37179213181`은 준비 직후 1920×1080이던 화면이 별도 앱 단계에서 1024×768로 돌아간
  기록을 남겼다. 임시 display 소유 pwsh를 각 실제 UI 여정과 같은 run block에 유지하도록 수정했다.
  로컬 영향 검사와 해당 workflow 회귀는 통과했으며, 최종 설치 수용을 대신하지 않는다.
  진단 `37179511129`에서는 네 제품 recover/import 각각 기본·최소 크기(16개 화면)가 통과했다.
  INSTALL-02도 실제 종료·재개를 통과했다. health 전환 뒤 renderer 준비 전 버튼 조회가 실패해
  전체 설치는 아직 실패이며, 접근성 준비 관측과 Workspace 보조 터미널 창 종료 경로를 보정한다.
  진단 `37179887712`는 health까지 24개 크기 관측을 통과하고 활성화 확정 화면까지 진행했다.
  확정 후 탐색 메뉴·본문의 동일한 복구 버튼 때문에 멈춘 driver는 `제품 화면` navigation 안으로
  대상을 한정했다. 설치·전환·재설치의 같은 탐색 동작에도 이 범위를 적용한다.
  진단 `37180226349`는 활성화 확정·복구 화면·Control Center 성능을 통과한 뒤 keyboard Launcher에서
  멈췄다. 실제 Chromium에서 driver Enter가 keypress 없이 기본 버튼 실행을 누락하는 현상을 재현하고,
  keyDown의 native text·physical code를 보정했다. Enter/Space 기본 실행, shortcut 문자 미삽입,
  한글 조합 Enter 보호를 실제 CDP 입력으로 확인했다. 제품 상태를 입력 대신 변경하지 않는다.
  진단 `37180653079`는 Launcher 키보드·모달·한글 조합을 통과하고 실제 배율 변경에서 멈췄다.
  네 main 창이 Tauri 기본값인 zoom hotkeys 비활성을 사용해 설정을 명시적으로 켰으며,
  config 회귀는 RED→GREEN을 확인했다. 새 제품 bytes의 Windows 확대 수용은 아직 미실행이다.
- 후속 runner는 hosted Windows의 소유 main 창·foreground·WebView focus를 확인한 뒤 native
  Ctrl+Add/Ctrl+0 입력으로 배율을 바꾸고 renderer DPR·viewport와 원상 복구를 관찰한다.
  PowerShell/C# 구문·입력 구조·소유 계층·비 hosted 거절은 로컬에서 확인했지만 실제 확대 PASS를
  대신하지 않는다. 실패한 최초 오류와 스크린샷은 cleanup 전에 보존한다.
- 진단 `37181151303`은 활성화가 실제 committed임을 소유 receipt·manifest·key로 확인한 뒤
  독립 업무를 관찰했다. 설치 확대 실패는 그대로 유지하며 이 예외는 진단 workflow에만 적용한다.
  INSTALL-02·AGENT-01·HTTP-01–03은 통과했고, API MCP·Knowledge 검색은 lazy route 준비 전
  입력한 runner 문제를 확인해 접근성 target 대기를 추가했다. v0.8.1 준비는 그 고정 source의
  역사적 `execute` 명령을 사용하도록 수정했다. 철회본 설치 완료와 reviewed update commit은
  여전히 실패 원인 확인 중이며, 추가 소유 native 오류·단계 근거를 기록한다. R16은 미완료다.
- 진단 `37182426188`은 native 확대 입력의 소유 창·foreground·WebView focus 검사를 통과한 뒤
  기존 payload의 배율 비활성에서 실패했다. Workspace 최초 오류는 Tasks lazy route의 `+ 새 작업`
  target 준비 전 입력으로 확인해 같은 방식으로 보정했다. 실제 첫 화면에서 발견한 흰색 기본 입력란과
  등록 확인 checkbox 너비는 공용 다크 컨트롤 기본값·Workspace 선택자 수정으로 해결했다.
  브라우저 CSS fixture의 720/1180px 너비·고대비 색상 확인은 통과했으며 packaged 수용과 구분한다.


- 진단 `37183914862`는 32개 기록 중 6 PASS·26 FAIL을 남겼다. Workspace 저장 직후 카드 렌더 대기,
  OAuth grant 목록 갱신 대기, webhook/gRPC/Activity 첫 route 준비 대기를 보정했다. Knowledge는
  native 저장 경로의 슬래시 표현과 실제 접근성 이름을 맞추고 긴 편집기의 화면 내 영역을 클릭한다.
  API 삭제 확인은 실제 Chromium에서 동기 confirm이 pointer 응답을 막는 현상을 재현해 명시적
  취소/승인을 클릭 전에 연결했다. 원격 API 실패와 동일 원인인지는 다음 기록으로 확인한다.
  고정 v0.8.1 fixture에는 Workspace·Knowledge의 명시적 저장소 시작 입력을 추가하며 기존 native
  readiness·health 조건을 유지한다. 철회본 등록 오류는 Details, reviewed helper의 조기 종료는
  소유 프로세스의 고정 형식 stderr 코드로 보존한다. 원인 미확정 제품 코드는 추측 수정하지 않았다.
  세대 전환 실패가 독립 검사를 가리지 않도록 integration/layout을 delivery보다 먼저 실행하며
  Agent의 update/portable 선행 근거는 유지한다. 묶음 catalog 검사는 2.893초·RSS 약 121MiB·swap 0으로
  통과했다. 기존 payload는 새 CSS·zoom 설정이 없으며 이번 기록도 승격 근거가 아니다.

- reviewed helper의 시작 검증 실패가 부모 창 종료 뒤 stderr로만 사라지던 UX를 보완했다.
  검증 실패에는 고정 오류 코드·데이터 보존·재개 안내를 한 번 표시하며 기존 Retry/Cancel 오류와
  일반 CLI는 유지한다. 실제 production 경계 함수의 좁은 회귀 2개를 확인했고 Windows 연결 검증은
  최종 Windows CI에서 수행한다. 진단 `37186226140`은 installer 실행 전 WebView2 정책 조회에서
  중단돼 제품 결과가 없으며, 이후 runner는 종료·OS 오류·signal 코드도 보존한다.

- 진단 `37186406654`는 기존 `c91d0326` payload로 UI-01의 네 제품·다섯 상태·두 크기 40개
  화면 관찰과 PERF-01을 통과했다. 이는 필수 사용자 여정 40개 전체 통과를 뜻하지 않는다.
  INSTALL-02·AGENT-01·HTTP-01–03·SEARCH-01/02도 통과했지만 전체 실행은 실패다.
  철회본 위 재설치는 `bootstrap_update_pending`, 검토 후 업데이트 확정은
  `bootstrap_root_unsafe`로 실패했다. 전자는 health 단계의 기존 dispatcher 보존 분기보다
  먼저 적용되는 등록 차단, 후자는 설치 root를 현재 작업 폴더로 상속하는 helper 실행에서
  원인을 확인했다. 배타적 writer lease·계획/설치 identity·경로 안전 검사는 유지한다.
  runner는 Workspace Files 준비와 최초 실패 보존, API 환경 재시작 준비·webhook 종료 관찰,
  MCP 합성 응답의 필수 캐시 메타데이터, Knowledge 키보드 이동을 보정한다.
  v0.8.1의 명시적 저장소 준비 직후 typed unavailable은 정확한 역사 source·요청 provenance가
  일치할 때만 기존 제한 시간 내 읽기 전용 재관측하며 native readiness 조건은 유지한다.
  이 진단의 UI·성능 성공도 새 제품 bytes의 수용 근거로 이전하지 않는다.
  보정 후 등록 정책 2개와 실제 자식 프로세스 작업 폴더 회귀를 확인했다. 후자는 기존 상속 방식
  실패·명시적 helper 디렉터리 성공을 재현했다. 실제 Chromium에서는 CodeMirror의 Tab 들여쓰기와
  동기 prompt의 key 응답 대기를 재현하고 Escape→Tab 및 명시적 prompt 결정의 동시 처리로
  해결했다. Knowledge 입력 실패는 첫 화면을 보존한 뒤 검증한 합성 문서만 UI로 복구한다.
  최종 묶음 catalog/workflow 검사 3.981초·RSS 약 133MiB·swap 0, Biome CI 통과.
  추가 Windows 전용 소스의 실제 컴파일 및 새 제품 bytes 수용은 최종 CI·후보에서 확인한다.

- #620의 PR CI `37188904663`과 exact-main CI `37189562754`은 각각 6개 job을 통과했다.
  후보 `37190461507`(`dfadd9e5`)은 네 제품 빌드·7개 자산 조립·네 native scope·독립 VM의
  WSL2/Docker 및 INSTALL-01/02를 통과했지만 설치 후 사용자 여정과 최종 seal은 실패했다.
  UI-01의 40개 화면 관측과 Knowledge 입력·성능의 성공을 전체 여정 성공으로 합산하지 않는다.
  이 후보는 승격하지 않는다. v0.9.0 태그와 Release는 아직 생성하지 않았다.
- 같은 후보에서 업데이트가 health 단계까지 진행했으나 실제 보존본 receipt가 새 journal의
  복구 목록에서 빠졌다. 검증된 update plan의 receipt를 health 공개 전에 기록·저장하고 이전 이력도 유지한다.
  재개 시 중복 기록 및 충돌을 실제 Journal 회귀로 확인했다. Windows 설치 수용은 아직 남았다.
  다른 generation의 보존본은 복원 권한을 넓히지 않고 목록에서 제한을 설명한다. 목록 갱신마다
  데이터 전체를 읽지 않으며, 크기 제한·digest·소유권을 확인한 manifest 정보만 사용한다.
  전체 데이터 검증은 기존 복원 준비 경계에 남긴다.
  실제 Store의 새 작업 revision 0 조건을 유지해 이전 이력을 초기 상태로 원자 저장한다.
  Store begin/write·닫기·재개와 중복 기록 방지 회귀도 통과했다. 이전 이력의 로컬 전달만
  확인한 테스트로 영속 저장 성공을 대신하지 않는다.
  Knowledge 오류·알림이 수평 flex의 직접 자식이 되어 편집 영역을 밀던 제품 레이아웃도 수정했다.
- runner의 viewport 중앙 좌표가 부모 scrollport 밖의 다른 pane을 가리키는 오류를 Chromium의
  실제 입력으로 재현했다. 관측한 조상 clipping 영역 안에서만 좌표를 고르고 기존 소유권
  hit-test를 유지한다. 실제 입력 성공과 overlay 거절을 확인했다. Workspace 자동 등록·lazy 화면,
  Knowledge 요청 문서 로딩을 읽기 전용 관측으로 기다린다. 작은 합성 문서의 비교는 AX가 덧붙이는
  줄바꿈 대신 렌더링된 실제 줄을 사용하며 trim으로 데이터 차이를 감추지 않는다.
  고정된 v0.8.1 import fixture는 당시 소스가 요구하는 활성화 전 migration owner 기록을 사용한다.
  이 로컬 회귀 및 보관 bytes 진단은 새 제품 bytes의 최종 수용을 대체하지 않는다.
  Knowledge 영향 검사에서 349개가 통과했고, 추가 오류 배치 회귀는 수정 전 실패를 확인했다.
  수정 후 직접 영향받는 두 파일 43개·최종 frontend 빌드·catalog/workflow·Biome은 통과했다
  (47.4초, 합산 RSS 약 1.41GiB, swap 0). 편집기가 없는 상태는 빈 문서로 인정하지 않는다.
  Control Center의 최종 frontend 빌드·75개 테스트, Rust check/clippy/fmt·44개 테스트와
  production TypeScript exporter를 통과했다. 실제 등록된 update 이력·Store 재개 회귀도
  이 Rust 실행에 포함됐다. 마지막 Rust 보충 검사는 30.9초·합산 RSS 약 1.02GiB·swap 0이었다.
  검사 중 지적된 테스트 비교 구문만 고친 뒤 Clippy와 미실행 범위를 이어서 확인했다.

- 보관 bytes 진단 `37197180461`(runner `a0a1fd90`, payload `dfadd9e5`)은 설치·활성화,
  40개 화면 관측, HTTP·Webhook·문서별 Undo·검색을 통과했지만 전체 사용자 여정은 실패했다.
  서로 다른 source의 진단이며 승격할 수 없다. 같은 draft PR #621에서 확인된 원인을 묶어 수정한다.
  Files의 680px 최소 너비는 확대된 655px viewport를 넘었다. 실제 Chromium 레이아웃 회귀로
  넘침을 재현·수정했다. Windows 현대식 저장창의 파일명 컨트롤을 지원하고 실제 소유 임시
  저장창의 입력·파일 생성으로 실패/성공을 확인했다. WSL 선택 목록과 종료 확인창의 준비,
  Handoff 접근성 이름, MCP grant의 native cache를 고려한 실제 취소·재시작 준비도 수정했다.
- ENV 취소 실패는 제품의 전역 확인 처리 결함이었다. 설치된 Tauri dialog 2.7.2가
  `window.confirm`을 Promise로 바꾸므로 동기 분기는 선택 전에 작업을 수행했다.
  네 제품의 전역 확인 호출을 공통 비동기 인앱 확인창으로 교체하고 명시 승인만 실행한다.
  취소·Esc·host 해제·표시 오류는 거절하며 확인 중 바뀐 대상과 상태를 재검사한다.
  비동기 확인 대기·취소 때 native 부작용이 없다는 회귀와 실제 Chromium 입력·포커스 복귀를
  확인했다. 이 브라우저 검사는 수정 바이너리의 Windows 설치 수용을 대신하지 않는다.
- INSTALL-03의 복구 목록 조회 실패는 기존 artifact에서 상세 원인을 확인할 수 없었다.
  입력·journal·잠금의 고정된 오류 코드만 전달해 다음 실제 실행에서 구분하고, 파일 경로나
  원시 오류는 노출하지 않는다. 권한·복원 generation 검사는 완화하지 않았다.
  제거 프로그램 비정상 종료는 즉시 실패로 기록하고 성공 receipt를 180초 기다리지 않는다.
  health 기록 중에는 같은 native journal 잠금을 사용하는 목록을 동시에 조회하지 않고,
  한 번 실행한 기록 버튼이 다시 준비된 뒤 읽기 전용 freshHealth를 확인한다.
- 최종 영향 검증은 네 frontend 빌드·타입 검사를 통과했다. Knowledge 초기 JS는
  확인창 추가 뒤 280,076 bytes로 상한을 넘었으므로 저장소 준비 UI를 복구 가능한 지연 로딩으로
  옮겼다. 해당 앱만 다시 빌드해 276,521 bytes 및 실제 route·복구 테스트를 확인했다.
  마지막 미실행 범위의 catalog/workflow·bundle·컴포넌트 크기·Biome·Rust fmt와 실제 Chromium
  확인창/Files 검사는 통과했다(39.1초, RSS 약 1.50GiB, swap 0). 통과한 다른 앱은 다시 빌드하지 않았다.
  Workspace 확인 처리 171개·Control Center 25개 및 새 검토 helper 3개, Knowledge 74개,
  API 확인 처리·동시 기록 보존·민감정보 복사의 직접 영향 검사와 공통 확인창 4개가 통과했다.
  고정 native 오류 코드·production exporter·Control Center 타입 검사도 통과했다.
  설치된 새 Windows 바이너리의 전체 수용과 최종 CI는 로컬 통과와 구분해 남겨 둔다.
- 같은 설치 진단의 Tasks 화면에서도 900px 최소 너비가 확대된 655px viewport를 넘었다.
  최소 너비를 제거하고 탐색·작업 조작·편집 및 기록 검색을 좁은 화면에 맞춰 배치한다.
  실제 Chromium의 CSS 회귀에서 기존 넘침을 재현했으며 설치본의 최종 수용과 구분한다.
- RUNTIME-01 응답 유실 검증은 native callback을 멈춘 뒤 일반 Runtime.evaluate로 읽으면서
  멈춘 renderer의 실행을 기다렸다. 실제 Chromium에서 대기를 재현하고 캡처한 call frame의
  동기 읽기로 변경했다. 원래 operation ID·한 번의 실행과 중지 확인·재시작 전 authority 근거는
  유지하며 실패 시 debugger를 해제해 후속 독립 여정에 일시정지를 남기지 않는다.
- PR CI `37200601015`는 새 테스트 fixture의 누락된 타입 필드와 notices의 lock digest를,
  `37200916164`는 고정 v0.8.1 명령의 역사 예외 누락과 동시 브라우저의 짧은 준비 제한을 발견했다.
  남은 실행을 취소하고 직접 영향 범위부터 보정했다. 역사 예외는 두 파일의 정확한 두 줄로
  제한하고 다른 구버전 호출 거절 회귀를 추가했다. 브라우저는 순차 실행하고 시작 준비만
  상한 20초로 관측한다. 기존 CI 필수 gate와 제품 수용 조건은 유지한다.
  Tasks의 43개 제어·원래 실행 식별자를 보존하는 runner 9개 회귀 및 변경된 Workspace의
  frontend 빌드·bundle 상한·Biome이 통과했다. 빌드의 합산 RSS는 약 1.65GiB, swap은 0이다.

최종 보정 PR·새 후보·공개 결과는 같은 PR 본문과 Actions artifact·Release notes에 기록한다.

## 1. 기존 감사 23건의 누락 없는 배정

| ID | 우선 | 재현 조건·문제 | 핵심 위치 | 작업 | 검증 ID·종료 조건 |
|---|---|---|---|---|---|
| S1 | P1 | import 배너가 추가되면 네 앱 본문 200px로 이동 | `/home/jihoon/projects/devbox/packages/product-shell/src/index.tsx:93`, `styles.css:32` | R06 | UI-01: 모든 delivery 상태의 본문/primary 가시성 |
| W0 | P1 | setupOnly store selected 뒤 금지 registry snapshot→실패/재생성 반복 | `/home/jihoon/projects/devbox/apps/devbox-workspace/src/RegistryGate.tsx:109` | R07 | INSTALL-01/02: selected에 snapshot/start_empty 재호출 없음 |
| W1 | P1 | Files edit→X/restart, recovery writer 미연결로 초안 소실 | `/home/jihoon/projects/devbox/apps/devbox-workspace/src-tauri/src/component.rs:349`, `/home/jihoon/projects/devbox/packages/workspace-features/src/files/App.tsx:1438` | R02 | WORK-01: 실제 편집→기록→종료/재열기, 실패 시 창 보존 |
| W2 | P1 | Source dirty→Agents 다른 worktree 검토, guard 우회 | `/home/jihoon/projects/devbox/apps/devbox-workspace/src/Workspace.tsx:288` | R02 | WORK-02: native select 전 차단, 취소 후 초안/context 보존 |
| W3 | P2 | 새 agent worktree 등록 후 parent registry stale | `/home/jihoon/projects/devbox/apps/devbox-workspace/src/agents/agentFlow.ts:87` | R03 | WORK-03: base-only 초기 상태에서 즉시 Source/Dependencies 일치 |
| AS-01 | P1 | JSON/raw→None 후 이전 body native 송신 | `/home/jihoon/projects/devbox/crates/http-client-engine/src/commands/request.rs:1252` | R04 | HTTP-01: 실제 echo body=0, cURL/browser/native 일치 |
| AS-02 | P1 | query/form 예약 문자 비인코딩→의미 변조 | `/home/jihoon/projects/devbox/crates/http-client-engine/src/commands/request.rs:3121` | R04 | HTTP-02: decode pairs/중복/순서/한글 보존 |
| AS-03 | P1 | Webhook 외부 POST 후 history 화면 미갱신 | `/home/jihoon/projects/devbox/packages/api-studio-features/src/webhooks/App.tsx:313` | R09 | WEB-01/02: idle arrival/route 복귀/실패 후 읽기 회복 |
| AS-04 | P1 | var2 하나 import→+변수→기존 값 삭제 | `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/components/RequestSidebar.tsx:521` | R08 | ENV-01: sparse/secret 충돌 시 기존 값 보존 |
| AS-05 | P2 | 환경 변수 이름 입력/수정·개별 삭제 불가 | 같은 RequestSidebar.tsx:456 | R08 | ENV-01: baseUrl/token을 UI로 만들고 요청까지 사용 |
| AS-06 | P2 | 새 환경 생성 뒤 첫 기존 환경 선택 | `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/hooks/useEnvironmentPersistence.ts:149` | R08 | ENV-01: 생성 id 선택/실패 시 이전 선택 유지 |
| AS-07 | P2 | auth 변경 뒤 비활성 field 변수 때문에 send 차단 | `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/lib/runner.ts:191` | R04 | HTTP-01/03: 활성 field만 검사·resolve |
| AX-01 | P1 | MCP 연결 A에서 B/None 선택, 호출은 A 자격 | `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/ProtocolLab.tsx:989`, native `mcp.rs:560` | R05 | AUTH-01/02: active grant와 UI 일치, mutation 1회 |
| AX-02 | P2 | pipeline 편집이 신규 id 생성, 20개 포화 후 저장 회복 불가 | `/home/jihoon/projects/devbox/packages/api-studio-features/src/transforms/workflows/SmartWorkflowPanel.tsx:259` | R11 | TRANSFORM-01: full library 편집/삭제/새 저장/재열기 |
| AX-03 | P2 | gRPC 2개 message 뒤 error→응답0으로 소실 | `/home/jihoon/projects/devbox/crates/http-client-engine/src/commands/grpc.rs:1003` | R10 | GRPC-01: 부분2개+terminal error와 history 일치 |
| K1 | P1 | 노트 A→B→Undo→B에 A 내용 autosave | `/home/jihoon/projects/devbox/packages/knowledge-features/src/notes/components/MarkdownEditor.tsx:220` | R01 | DOC-01: 실제 B disk에 A sentinel 없음 |
| K2 | P2 | 반복 observe 뒤 idle 종료, 마지막 입력 후298초 집계 | `/home/jihoon/projects/devbox/crates/activity-engine/src/core/sessionizer.rs:55` | R13 | ACTIVITY-01: idle-tail 구간 제외, pause/clock 경계 보존 |
| K3 | P2 | regex 기호/한글 제거한 FTS가 정상 후보 제외 | `/home/jihoon/projects/devbox/packages/knowledge-features/src/search/App.tsx:453` | R12 | SEARCH-01: candidate→worker 통합 정확성·한도 partial |
| K4 | P2 | Settings의 history 재생성 항상 disabled/현재 digest 오용 | `/home/jihoon/projects/devbox/packages/knowledge-features/src/activity/App.tsx:590` | R13 | ACTIVITY-02: 과거 entry 기간/timezone으로 재생성 |
| K5 | P2 | 원본 삭제 뒤 journal 열기 실패, UI 복원/본문 확인 불가 | `/home/jihoon/projects/devbox/packages/knowledge-features/src/notes/components/RecoveryControls.tsx:92` | R01 | DOC-02: 원본 없이 preview/copy, 명시 CAS 복구 |
| K6 | P2 | installed QuitGuard가 실제로 유지되는 수집을 중지한다고 안내 | `/home/jihoon/projects/devbox/apps/devbox-knowledge/src/QuitGuard.tsx:97` | R13 | ACTIVITY-03/AGENT-04: owner별 문구와 실제 process 일치 |
| K7 | P2 | 최소720×480 기본 노트 편집폭33px | `/home/jihoon/projects/devbox/packages/knowledge-features/src/notes/App.css:35/354`, Knowledge tauri.conf.json:20 | R06 | UI-01/02: 최소 창 편집폭≥320px, toolbar 도달 |
| C1 | P1 | products 화면 recordSuiteHealth가 route admission에서 거절 | `/home/jihoon/projects/devbox/apps/devbox-control-center/src/Health.tsx:76`, native `ipc/delivery.rs:131` | R07 | INSTALL-01: 실제 화면 route=header, 다음 단계 완료 |

원 감사 근거 구분: S1/K1/K7 브라우저 UI 재현; W0 실제 권한을 반영한 RED 2개; AS-01/02 순수 Rust 분기 probe; AS-04/06/07 TypeScript 순수 함수 probe; K2 실제 sessionizer 실행; 나머지는 생산 코드 연결 확인. 이번 계획 작성에서 이들 시험을 다시 돌리지 않았다.

## 2. 계획 단계에서 추가 확인한 코드 결함

| ID | 분류/근거 | 문제 | 작업 | 종료 조건 |
|---|---|---|---|---|
| N01 | P2 · 코드 확인 | `IncomingCommands.tsx`/`SuiteConnection.tsx`의 refresh 성공이 이전 조회 issue를 해제하지 않아 복구 후에도 실패 표시가 남음. write issue와 섞지 않고 해제 필요 | R06 | 읽기 실패→최신 성공 후 해당 경고 소멸, late failure 무시; unrelated write 실패는 유지 |
| N02 | P2 · 코드 확인 | Requests App의 warning이 HTTP 전용 조건 안에 있어 Protocols에서 환경 save 실패 이유가 숨겨짐 | R08 | ENV-02: Protocols/History/Requests 모두 오류 표시와 write lock 설명 |
| N03 | P2 · 코드 확인 | Activity shift는 day/week/month만 변경; timeline 화살표는 날짜를 바꾸지 않음 | R13 | ACTIVITY-02: timeline 날짜±1일·native 기간 이동 |
| N04 | P2 · 코드 확인 | Activity idle threshold의 UI void 호출/native set_setting 오류 무시→거짓 성공 | R13 | ACTIVITY-03: read-only DB 오류 전달, 이전 ack값 보존, 재시작 readback |

N01 근거: `/home/jihoon/projects/devbox/packages/product-shell/src/IncomingCommands.tsx` refresh.then은 setReviews만, SuiteConnection.tsx refresh.then은 setStatus만 호출한다. 각 catch는 setIssue를 호출한다. 성공적 refresh가 error를 해제하는 경로가 없다.

N02 근거: `/home/jihoon/projects/devbox/packages/api-studio-features/src/requests/App.tsx:1532`의 `workspace !== 'protocol'` 조건 내부에 persistenceWarning. sidebar의 환경 UI와 persistenceReady=false 잠금은 protocol에서도 사용된다.

N03/N04 근거: `/home/jihoon/projects/devbox/packages/knowledge-features/src/activity/App.tsx:757`, `components/ActivitySettings.tsx:208`, `/home/jihoon/projects/devbox/crates/activity-engine/src/commands/tracking.rs:313`, `core/db.rs:249`. DB에는 이미 fallible `try_set_setting()`이 있으므로 이를 활용한다.

이 4건은 추가 코드 검토의 확정 경로이며 실제 Windows 재현을 이번에 실행한 것은 아니다. 기존 감사23건에 몰래 합쳐 재현 수를 부풀리지 않는다.

## 3. 고장으로 단정하지 않는 추가 개선

| ID | 왜 필요한가/현재 근거 | 구체 범위 | 작업·검증 |
|---|---|---|---|
| I01 | 설치 UI가 여러 제품과 helper 단계 조합을 요구 | journal 원장의 연속 안내·즉시 상태 갱신·중단 재개·다음 행동 | R07 INSTALL-01/02/03 |
| I02 | 일반 기능 불가 상태에서도 Agent/incoming 조회와 재시도 UI 노출 | availability 유도, 예상 대기/실제 실패 구분 | R06 UI-01, R07 |
| I03 | 같은 제품 안의 고정 panel과 toolbar가 작은 창에 경쟁 | 모든 route 대표 최소창·접근성·IME/초점·오류/빈상태 계약 | R06 UI-01/02 |
| I04 | Knowledge ‘버리고 종료’의 저널 보존 의도가 불분명 | keep-close와 명시적 영구 폐기 분리·drain·첫 offline 로컬 복구 | R01 DOC-02/03 |
| I05 | API 이전 응답을 전송/취소 중 유지하고 일부 행 이름이 모호 | 이전 응답 표시·요청 상태·행 레이블/포커스 | R04 HTTP-03/UI-02 |
| I06 | 환경 CRUD의 보존/실패/이름 충돌·secret 제약이 화면에 부족 | key CRUD·collision·ack·모든 route 오류, secret 재노출 없음 | R08 ENV-01/02 |
| I07 | 주기 갱신을 넣으면 rule draft/선택/stale 응답과 충돌 가능 | single-flight·hidden pause·부분 실패·마지막 정상 결과 유지 | R09 WEB-01/02 |
| I08 | workflow 저장완료/dirty/포화 상태 구분 필요 | ID 보존·save-as 분리·삭제/실패복구·입출력 비영속 유지 | R11 TRANSFORM-01 |
| I09 | 저장 검색 의미가 현재 source/regex 상태에 의존하고 index 상태 갱신이 제한적 | 현 저장 포맷 의미 명시·원자 load·partial/offline/expiry 상태 | R12 SEARCH-01/02 |
| I10 | privacy/동의/일시정지/창 종료/자동시작의 의미가 분산 | actual owner/status 표시·설정 ack·범위 설명 일치 | R13 ACTIVITY-03, R15 AGENT-03/04 |
| I11 | Tasks/Terminal 실행 응답 유실 때 실패/결과 불확실/재실행 혼동 | receipt 조회·read-only retry·대상 이동·provider 재검토 | R14 RUNTIME-01/02/DEPS-01 |
| I12 | 이전 installer 시험은 silent+직접 API/helper, foreground 수집은 사용자 확인 대기 | owned UI 입력·disk/wire/OS 관찰·source/digest matrix·fail closed | R00/R15/R16 전체 |

Source 초안의 crash 후 영구 복원, 모든 검색 모드 저장 포맷 확대, gRPC incremental streaming UI, 새로운 provider/프로토콜/앱은 이번 범위 밖이다. 필수 사용 흐름을 완성하는 위 항목은 단순 권장 사항으로 남기지 않고 소유 작업의 수용 조건에 포함한다.

## 4. 구현 이후 기록 형식

아래 한 행을 실제 결과로 작성하고 증거가 없으면 NOT_RUN으로 둔다. 이번 문서에는 가짜 PR 번호나 PASS를 채우지 않는다.

```text
ID / owner work item / single PR URL / tested source SHA / L1-L4 또는 L5 scenario / run artifact / PASS|FAIL|NOT_RUN / 남은 제한
```

R00 matrix 검사로 기존23·추가4·개선12의 소유 작업 누락과 scenario 없는 항목을 거부한다. R15/R16은 해당 source의 필수 scenario가 모두 채워졌는지 확인한다. 결함 수정 commit만 존재하고 실제 사용자 여정이 미실행이면 완료로 닫지 않는다.


## 5. 구현과 로컬 확인 근거

아래 commit은 작업별 변경 이력이며 최종 배포 source는 통합 PR의 main 머지 SHA다. 모든 작업의 Windows UI 수용은 후보 실행 전 NOT_RUN이다. 같은 테스트의 통과를 여러 번 합산하지 않는다.

| 작업 | 구현 commit | 원인·직접 영향 범위의 로컬 확인 | 남은 수용 |
|---|---|---|---|
| R00 | `302361bb` | 실제 입력 driver, matrix·identity·미실행 거절 validator 회귀 | Windows 입력/전체 gate 연결 |
| R01 | `600a9b06` | 문서 generation·Undo, journal/autosave/recovery·Quit·Startup, native journal 및 exporter | DOC-01–03 |
| R02 | `50131024`, `dfc56ecf` | Files 42, Workspace 초안/종료/전이 guard, native recovery metadata·files host·close admission | WORK-01/02 |
| R03 | `d1e87f6c` | registry projection·Agent 완료 후 갱신, typed app 검사 | WORK-03 |
| R04 | `2fcddb4d` | HTTP native 66 및 활성 필드·browser/cURL/codegen 직접 영향 회귀 | HTTP-01–03 |
| R05 | `6a8a29e5` | 실제 native 연결 grant snapshot, ProtocolLab 인증 잠금·불일치 거절 | AUTH-01/02 |
| R06 | `efa8c4a4`, `a3a1895d` | shell·Agent·Incoming·SuiteConnection; Chromium Notes 720×480 편집폭 468px/1180×780 704px, API 720×480 가로 잘림 없음 | 네 제품·전체 delivery 상태/keyboard/IME UI-01/02 |
| R07 | `50131024`, `c23f8218`, `a3a1895d`, `327e05af` | 재설치·업데이트·복원의 health 진입 회귀 RED→GREEN; setupOnly RED 2건 포함 RegistryGate; Control Center setup/Health/Restore 7, native delivery 검증 2, exporter·tsc | 실제 installer부터 INSTALL-01–03 |
| R08 | `3e0c60b1` | sparse/rename/delete·선택·저장 실패 UI 19 | ENV-01/02 |
| R09 | `d2221313` | Webhooks single-flight·active/hidden·읽기 회복 및 Studio 재진입 | WEB-01/02 |
| R10 | `5ff0dd7f` | 두 stream message 뒤 INTERNAL trailer 실제 tonic 회귀와 UI/history | GRPC-01 |
| R11 | `dbed4bdb` | full library 수정·삭제·용량 복원·지연 저장 UI/store | TRANSFORM-01 |
| R12 | `c978c3f2` | Search UI 30/API 3, content metadata 2/Notes metadata 1, native exporter | SEARCH-01/02 |
| R13 | `4345e80a`, `fd49b549` | idle sessionizer·실패 ack·DST 과거 기록, 날짜·privacy·owner UI, native tracking·exporter·tsc | ACTIVITY-01–03 |
| R14 | `5b314ca7`, `a6a41fe5` | Runtime recovery/Terminal/Dependencies/Files 영향 및 tsc; 응답 유실 뒤 실제 상태 조회로 실행 여부 조정, 부작용 중복 실행 차단 | RUNTIME-01/02, LSP-01, DEPS-01 |
| R15 | `efa4c861`, `8dc2b443`, `3ce46aac`, `8a51160d`, `33f031a3`, `f3be5eb1`, `df2fd78d` | 실제 입력·앱별 여정·handoff·설치 단계 관찰 runner 연결, exact-source/digest/전체 ID/스크린샷 gate; 로컬 runner 계약 검사 | 전체 packaged UI 여정은 후보 실행 전 NOT_RUN |
| R16 | `b2443b99`, `ee687389`, `eac4b572` | 현재 문서·닫힌 원장·철회 이력 정합성, 실제 철회본 7개 파일 digest, 업데이트·설치 실패 runner 준비 | PR CI→main CI→candidate→tag/release |

추가 구현 검토에서 설치 준비 중의 제품 연결과 Agent 재연결을 구분했다. 제품 버스는 준비 상태 확인에 필요하므로 복구 단계에도 명시적으로 연결할 수 있고, 일반 작업·Agent 재연결·수신 작업은 계속 차단한다. API의 좁은 창에서 숨겨지던 코드/OpenAPI/캡처 조작도 브라우저 측정으로 확인해 줄바꿈을 적용했다.

R15 여정 연결에서 추가 발견한 결함도 수정했다. Workspace 빠른 열기의 한글 조합 Enter 보호(`f58a7199`), API 버튼의 실제 접근성 이름(`733d5b0b`), 대상 ES 버전에서 지원하지 않는 Array.at 사용(`3070818a`), Windows 입력 tick 순환(`fd49b549`)을 직접 영향 범위로 확인했다. 설치 수용은 동일 설치 key와 후보 digest를 추적하며, v0.8.1에서 전환하는 별도 소유 fixture만 부모 설치 key를 명시하는 예외를 둔다. 예전 공개본의 성공을 새 후보에 승계하지 않는다.

기존 후보 artifact `11265063384`(run `37095144741`)의 철회본은 새 후보가 아니라 동일 버전 교체 검사의 고정 입력이다. manifest SHA-256 `27a3dd7b2dab585fa6a930d3a1e64bd4f1f3166a141c926080f005bc16114b4b` 및 7개 파일의 폐쇄 목록·크기·digest를 확인한다. 입력이 없거나 바뀌면 새 빌드로 대신하지 않는다. 별도 v0.8.1 전환과 실제 철회본→수정본 경로의 데이터 보존은 각각 구분해 기록한다.

최종 로컬 통합에서 기존 크기 상한을 넘은 컴포넌트는 앱 내부 helper로 분리했다(`726cfdf8`, `9762e3d0`, `cdd34186`). Knowledge의 초기 route 처리는 복구 가능한 지연 로딩으로 옮겨 초기 JS 278,731 bytes(상한 280,000)를 확인했다. 타입·테스트 계약 누락과 훅 의존성, Clippy 지적을 보완했고, 최초 전체 검사의 실패/미실행 범위만 이어서 확인했다. 상세 결과와 최종 source는 단일 PR 본문에 기록한다.


## 6. 설치 후보 수용 보정

통합 #616 이후의 보정 #621은 `ed66f7e1`로 머지됐고, 해당 main CI
[37203253236](https://github.com/jihoon22-lee/devbox/actions/runs/37203253236)는 통과했다.
후보 [37204376052](https://github.com/jihoon22-lee/devbox/actions/runs/37204376052)는
네 native 범위·격리 WSL2/Docker·7개 자산 조립·최초 설치/활성화·UI-01·PERF-01을 통과했으나,
설치 후 사용자 여정 실패로 최종 봉인이 거부됐다. 이 후보는 승격할 수 없다.

후속 보정은 R15 검증 경계에 한 묶음으로 적용한다. 비동기 route·연결·문서 복구 완료를
읽기 전용으로 관찰한 뒤 UI 입력을 한 번 수행하고, 디버거 정지 중에는 해당 call frame에서
원래 요청을 읽는다. CodeMirror 접근성 표시의 개행은 문서 바이트가 아니므로 원본·저널과
정확한 편집기 줄 내용을 비교한다. 업데이트 health 실패에는 실제 실패 시점의 제한된
제품 상태와 스크린샷을 남긴다. 기존 소유권·원본 보존·중복 실행 금지·40개 필수 여정은 유지한다.

Workspace 재시작 뒤에는 native session의 프로젝트 선택이 초기화된다.
RUNTIME-01은 같은 소유 프로젝트를 명시적으로 다시 선택해 실행 맥락을 복원한다.
저장소 준비와 프로젝트 선택은 별도 상태이며, 선택 초기화만으로 기존 runtime 시간 초과의
직접 원인이 확정되지는 않는다. 배포본의 결과는 재사용 진단에서 별도로 확인한다.
별도로 개발 모드 StrictMode effect 재실행이 완료 receipt를 소비한 뒤 결과를 버리는
제품 결함도 재현해 진행 중 조회 결과를 공유하도록 보정한다. 이 개발 모드 회귀를
배포 후보 실패의 직접 원인으로 간주하지 않으며, 기존 후보에는 이 제품 수정이 포함되지 않는다.

추가 진단은 위 후보의 검증된 설치 파일을 재사용하며 `diagnosticOnly=true`,
`promotionEvidence=false`로 기록한다. 진단 성공은 다른 source의 출시 수용으로 승계하지 않는다.
최종 보정 PR의 로컬 영향 검사·필수 CI 및 새 exact-main 후보 전체 수용 후에만
v0.9.0 태그와 릴리스를 진행한다. 실제 최종 결과는 해당 PR 본문과 Actions artifact에 기록한다.


재사용 진단 [37211916702](https://github.com/jihoon22-lee/devbox/actions/runs/37211916702)은
DOC-03·ACTIVITY-01·ENV-01 및 HTTP/Webhook/Search 일부 여정을 통과했으나 전체는 실패했다.
추가 확인된 제품 결함은 기존 private vault 부재를 신규 vault 생성으로 숨기던 Knowledge
시작 처리, 입력란을 가리는 닫기 없는 Undo 알림, Runtime 완료 조회 중 화면을 숨기거나
후속 목록 조회가 실패할 때 이미 소비한 완료 안내를 잃는 경계다. 각 실패를 로컬 회귀로
재현해 수정했으며 Runtime 배포본 시간 초과와의 직접 인과는 새 후보에서 확인한다.
Undo 닫기·입력은 Chromium 720×480·320×480에서 실제 조작으로 확인했다.

검증 코드도 함께 보정한다. Workspace 편집기 관찰에 누락된 CDP 인자를 전달하고,
Activity의 실제 미리보기 대화상자에서 명시적으로 취소한 뒤 다음 작업으로 이동한다.
API 저장 충돌은 실제 지역화된 오류를 확인하고 OAuth·native 저장 대화상자의 준비를
관찰한다. 이관 WAL 표식은 허용된 기존 settings 테이블에 기록하여 실제 스키마 검사를
유지한다. 이전 generation 보존본의 복원 차단을 확인한 뒤 현재 generation 보존본을
실제 UI로 생성하여 복원·rollback을 검증한다. 정상 종료 중 WebView 연결이 먼저 끊겨도
소유한 native 프로세스의 실제 종료를 끝까지 확인한다. Workspace를 재시작해도 설치 Agent의 첫 실행은 남으므로, 재조정 뒤 실제 UI에서 중지하고
종료를 확인한 후 두 번째 실행을 시작한다. overlap=skip 정책은 유지한다. Agent의 연결 불가·
소유자 부재는 앞선 업데이트 실패와 함께 기록하며 성공으로 간주하지 않는다.

이 수정은 기존 payload `ed66f7e1`에 없으므로 진단 재사용만으로 제품 수정을 수용할 수 없다.
상세 로컬 결과와 새 Windows 후보 결과를 #622 본문에 갱신하며 실패한 후보는 계속 승격 금지다.

#622는 `80a23c0f`로 머지했다. 후보 준비 중 R06 화면 증거를 다시 확인하여 Tasks·Agent 등
작업 화면 위에 전체 프로젝트 등록 폼이 반복되는 배치 결함을 추가 보정한다. 준비를 마친
프로젝트 관리는 개요에서만 표시하되 컴포넌트와 입력 초안·편집 보호는 유지한다. Source에서
생성한 worktree의 등록 검토는 개요로 연결하고, 프로젝트 미선택 상태에는 개요 이동 버튼을
제공한다. 준비 실패·재시도는 계속 표시한다. 이 변경 전 main CI는 중단했고, 수정한 source의
필수 CI와 새 후보를 사용한다. 실제 로컬 화면·Windows 수용 결과는 해당 보정 PR에 기록한다.

#623은 `046e0d27`로 머지했고 exact-main CI `37217839287`은 통과했다.
후보 `37218692587`은 7개 자산 조립·native API/Knowledge/product-shells·격리 WSL2/Docker와
최초 설치/활성화를 통과했으나 Knowledge 검색 파일을 Workspace 편집기로 전달하는
cross-product 수용에서 실패했다. 이 후보는 승격하지 않는다. 재사용 진단은 기존 후보
bytes와 runner source를 구분하고 `diagnosticOnly=true`, `promotionEvidence=false`를
기록한다. 진단 `37225141266`에서는 동일 bytes로 cross-product 17개 검사를 통과했지만
최초 실패 원인은 미확정이다. 추가 IPC 관찰자는 Tauri의 읽기 전용 invoke 속성에 연결되지
않았으므로 실제 상세 응답을 수집하지 못했다. 효과 없는 관찰 코드는 제거하며 이 실행을
제품 수정이나 새 source의 출시 수용 근거로 사용하지 않는다.

같은 실행에서 확인된 별도 제품 결함은 portable Knowledge의 초기 저장소 준비 전에 종료
보호가 Activity 상태를 조회하여 미등록 Tauri state에 접근하는 panic이다. 미준비 local
collector는 owner를 보존한 미확인 상태를 반환하고 실제 준비 뒤나 installed Agent 조회는
유지하도록 수정했다. 초기 조회·준비 완료·remote owner 회귀를 로컬에서 확인했다.

후보 빌드 지연도 실제 cache 로그로 확인했다. 두 shard가 같은 exact cache를 복원하여
추가 dependency 산출물을 저장하지 못하므로 shard별 cache namespace를 사용한다. 새
namespace가 아직 없을 때만 기존 cache를 읽기 전용으로 복원하며 공유 cache는 삭제하지
않는다. 이 변경의 성능 효과는 다음 후보 실행에서 측정한다. 설치 후 전체 여정 및 새로운
제품 source의 최종 수용 결과는 보정 PR 본문과 Actions artifact에 기록한다.

설치 후 실행이 계속되는 동안 검증 fixture의 대기 경계도 점검했다. Workspace의 읽지 않는
stderr pipe는 즉시 비우고 동기 창 닫기에 10초 제한을 둔다. update·legacy helper는 180초
실행 제한 뒤 소유한 child에 종료를 요청하며, 2초 정리 유예에도 종료 이벤트가 없으면
명시적으로 실패하여 다음 검사와 증거 기록을 막지 않는다. 원래 오류·정상 종료 코드는
보존한다. 종료 요청 실패·이벤트 부재 회귀와 기존 legacy 검사는 통과했지만 이것이 현재
Windows 실행 지연의 원인이라는 증거는 아직 없으므로 별도로 기록한다.


후보 `37218692587`의 설치 여정은 17 PASS·11 FAIL·12 미완료로 종료됐다. NSIS 업데이트가
`bootstrap_update_pending`으로 실패한 뒤 검증 Node가 자식 프로세스 참조를 유지하여
42분 동안 다음 단계로 진행하지 못했고 작업 제한 시간에 도달했다. 실패한 소유 설치 창은
가능할 때 정상 취소하며, 취소가 끝나지 않아도 프로세스와 설치 증거를 보존한 채 참조를
해제하여 원래 실패와 다음 독립 검사를 기록한다. 임의 프로세스를 강제 종료하지 않는다.

Control Center는 최초 dispatcher를 업데이트 후에도 유지한다. 이후 정상 Health 업데이트가
최초 revision으로 돌아오면 이를 잘못 거부하던 정책을 수정했다. pending 상태·candidate
revision·기존 등록·shortcut/uninstaller·dispatcher 신뢰 검사는 유지하며 Health 중에는
같은 revision이어도 dispatcher를 다시 등록하거나 버전을 미리 광고하지 않는다.
Workspace의 터미널 로그 조회는 일시 실패 후 성공해도 오래된 오류 안내가 남던 결함을
별도 회귀로 수정했다. 두 제품 수정의 Windows 수용은 새 후보에서 확인해야 한다.

설치 검증 자체의 실패도 구분했다. WORK-01 재시작 뒤 같은 소유 프로젝트와 Files 화면을
복원하고, Source 승인 이후 aggregate busy가 해제된 시점을 관찰한다. RUNTIME-01은
디버거 연결 해제로 정지한 응답을 소비하기 전에 소유 native 프로세스 종료를 완료하며,
재시작 직후 해당 요청이 pending인지 확인한다. API native 파일 선택 대화상자 준비를
관찰하고 정리 실패로 최초 오류를 덮어쓰지 않는다. Knowledge는 이관 후 현재 native
vault root를 읽고 초안 재생성 전에 UI의 기록 새로 고침을 명시적으로 수행한다.

보정한 검증 코드는 기존 후보의 설치·활성화와 앱별 다섯 실행만 선택하는 재사용 진단으로
먼저 확인한다. 이 모드는 업데이트·이관 검사를 생략했다고 receipt에 기록하고 승격 근거로
사용하지 않는다. 기본 후보의 전체 여정은 유지하고 각 단계의 시작·종료·exit code를 남긴다.
최종 로컬 영향 검사와 재사용/신규 후보의 실제 결과는 보정 PR 및 Actions에 기록한다.
