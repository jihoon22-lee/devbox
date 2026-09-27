# 0015 사용자별 백그라운드 agent

상태: 채택 · B9 구현 반영(Windows 수용 결과는 ledger #580)

기록일: 2026-09-25 · 확정일: 2026-09-26

## 맥락

기존 런타임 작업·서비스·웹훅 리스너·활동 수집은 제품 UI 프로세스에 있었다. 창 종료나 UI 장애가 작업 수명에 영향을 주고 제품별 트레이 아이콘도 최대 세 개였다. 네 제품에 흩어진 백그라운드 작업의 연결·권한 처리를 하나의 프로세스에서 관리한다(리뷰 §8 C안).

Workspace 터미널은 WSL 배포판의 zellij·tmux 세션에 연결한다. 세션은 이미 UI 수명과 분리되어 있고 pane의 권한은 창별 peer/session guard에 묶여 있다. 이 경계까지 옮기는 비용을 백그라운드 작업 분리와 섞지 않는다.

## 결정

### 프로세스와 배포

동일 사용자 세션·Suite 설치마다 `devbox-agent.exe` 하나를 둔다. 서로 다른 설치는 ADR 0007의 namespace와 설치 접미사로 분리한다. 소스는 `apps/devbox-agent`이고 **창 없는 Tauri 앱**으로 만든다. 기존 engine의 `tauri::AppHandle`·plugin·managed state를 그대로 이용하되 WebView를 만들지 않으므로 agent를 위한 WebView2 프로세스는 없다.

Control Center 패키지의 `resources/suite/devbox-agent.exe`로 배포한다. Suite bootstrap처럼 내부 구성요소로 취급해 공개 자산 7개 계약을 유지한다. 공개 제품 카드나 다섯 번째 사용자 제품은 추가하지 않는다. ADR 0016에 따라 별도 코드 서명·암호화·인증 토큰·pipe DACL 강화는 이번 범위에 추가하지 않는다.

설치본의 native agent 요청은 현재 generation agent에 연결하며 없으면 한 번 시작한다. 시작 경쟁은 설치 범위의 단일 인스턴스와 클라이언트 연결 잠금으로 합친다. portable은 기존 제품 프로세스 소유이며 설치 연결 실패를 portable로 전환하지 않는다. agent가 제품 열기·활동 일시중지/재개·전체 종료 메뉴가 있는 트레이 아이콘 하나를 소유한다.

로그인 자동 시작은 기본 꺼짐이다. Control Center 환경과 Knowledge 활동 설정은 같은 `agent.settings`를 사용한다. HKCU Run의 `Devbox Agent`는 현재 generation의 내부 exe와 `--autostart`만 등록한다. 기존 설치 소유 Knowledge Run 항목·Workspace 시작 바로가기는 켜짐 의도를 보존해 이전하고 다른 설치 항목은 덮어쓰지 않는다. agent 시작 및 검증된 forward-only 설치/업데이트 commit에서 켜진 경로만 갱신하고 uninstall에서 소유 등록을 제거한다. 제품 창이나 새 수집 동의를 자동으로 만들지 않는다.

### 소유권 이전 순서

| 단계 | agent로 옮기는 작업 |
|---|---|
| P2-02 | 런타임 작업·서비스·스케줄 |
| P2-03 | 웹훅 리스너 |
| P2-04 | 활동 수집·검색 색인, 트레이 통합·자동 시작 |

터미널·파일 편집·LSP·Git 검토·UI 상태는 제품 프로세스에 남는다. 창을 닫아도 agent 작업은 유지되지만 제품별 route·project context·권한은 계속 별도로 검사한다. 실행 주체 통합이 제품 권한 통합이나 자동 신뢰를 뜻하지 않는다.

### IPC와 입장 검사

named pipe는 `\\.\pipe\devbox-agent-<설치 접미사>`다. 접미사는 ADR 0007의 기존 설치 key에서 얻고 임의 JSON 필드를 namespace로 사용하지 않는다. 양쪽은 `suite-runtime`의 OS pipe witness·process handle·실행 이미지·설치 generation 검사와 같은 경계를 적용한다. 제품은 동일 활성 generation의 agent인지 확인하고, agent는 동일 generation의 네 제품만 받는다. `CapturedScope::capture_agent_image`로 내부 resource 이미지를 별도로 보유·재검증하며 공개 제품 역할을 부여하지 않는다. pipe 이름이나 `Hello.product`만으로 인증하지 않는다.

`crates/agent-protocol`의 프레임은 4바이트 little-endian 길이와 JSON 본문이다. 본문 상한은 1 MiB, 출력 batch는 최대 64 KiB다. 큰 파일 저장은 agent로 옮기지 않는다. 길이 0·상한 초과·잘못된 JSON·알 수 없는 envelope 필드는 연결을 종료할 수 있는 고정 오류로 처리한다. 부분 수신과 여러 프레임 동시 수신을 지원하며 미완성 프레임 버퍼는 한 프레임 크기로 제한한다.

큰 조회 응답은 기존 `Stream` envelope의 `replyChunk` payload(offset·totalBytes·data)로 나눈다. batch는 64 KiB 이내이며 클라이언트가 검증·보관한 정확한 byte cursor의 `Ack` 뒤에만 다음 batch를 보낸다. `StreamEnd(reply_complete)`와 전체 길이·JSON 검증이 끝나야 결과를 반환한다. 응답 하나는 직렬화 기준 64 MiB, 연결당 보관 응답은 128 MiB로 제한하며 ack 기한은 batch마다 고정 10초다. 취소·기한 초과 후 이미 전송 중이던 정확한 ack 하나는 제한된 종료 기록으로 소비하되 새 응답에 연결하지 않는다. 요청은 전송 전에 전체 envelope의 1 MiB 한도를 검사한다. 큰 로그 목록의 순수 필터·내보내기는 UI에서 처리한다.

`Hello`/`Welcome` 뒤 `Call.id`로 여러 요청을 다중화한다. `Call.request`는 기존 `ComponentRequest<C>`의 `{header, method, args}` 그대로다. agent는 component별 typed 역직렬화와 기존 `admit`의 session·route·deadline·replay·lane 검사를 수행한다. OS에서 확인한 제품·설치·generation에 session을 결합한다. codec의 `check_hello`는 버전/형식 검사일 뿐 peer 인증을 대체하지 않는다.

연결의 활성 요청 ID 중복은 `duplicate_request`로 거부하고 완료·취소 때 제거한다. 이는 기존 requestId replay 검사를 대체하지 않는다. 실제 연결 소유자가 기존 admission 한도 안에서 활성 요청 수와 수신 기한을 제한한다. 로그 tail·작업 출력 스트림은 stream ID로 다중화하고 P1-16의 한 batch 전송→실제 소비 완료→정확한 cursor ack 흐름과 고정 ack 기한을 따른다. UI 연결 종료 시 해당 구독을 해제한다. 단순 연결 단절로 이미 수락한 변경 요청을 자동 재전송하지 않는다.

### 업데이트와 writer lease

Control Center의 기존 업데이트/복원 확인 절차를 유지한다. 업데이터는 새 generation 활성화 전에 같은 설치 agent의 admission을 닫고 `Shutdown`을 보내 작업·서비스·리스너·수집기 쓰기를 정리한다. 제품이 소유한 PTY·LSP 등은 기존 제품 종료 절차로 정리한다. agent로 옮기지 않은 PTY를 agent가 소유한다고 가정하지 않는다.

agent도 제품과 같은 `suite-writers.lock`의 shared writer lease를 **전체 수명 및 하위 writer 종료까지** 보유한다. `WriterGuard::acquire_component`와 `component_namespace`가 내부 resource 위치에서 검증된 Control Center owner를 찾는다. 활성화 전에는 시작하지 않으며, 검증되지 않은 경로에서 빈 guard로 진행하지 않는다.

`Shutdown` 응답 자체를 교체 허가로 사용하지 않는다. 기존 bootstrap 업데이터가 모든 writer 종료를 확인하고 exclusive writer gate를 얻은 뒤 quiesced data checkpoint·검증·기존 activation 단계에 따라 교체한다. shutdown/gate 확보 실패 시 교체하지 않으며 기존 generation과 복구 상태를 보존한다. `suite-update.block`/`suite-data-restore.block` 동안 새 agent 시작도 차단한다. 새 활성 generation의 제품이 새 agent를 시작하며 이전 generation agent와 연결하지 않는다.

프로토콜이 다르면 Welcome 전에 `Rejected { reason: "protocol_mismatch" }`로 거절한다. UI는 “agent를 다시 시작합니다” 상태로 같은 설치·검증된 agent의 종료 및 재시작을 요청한다. 기존 writer/update gate를 우회하거나 이름만 같은 프로세스를 종료하지 않는다. 재시작 후 새 handshake가 성공해야 요청을 보낸다.

### 장애 복구

연결 시도는 200ms·500ms·1초·2초·4초 간격의 제한된 backoff를 사용하며 실패하면 연결 상태와 수동 재연결 버튼을 보여 준다. agent 비정상 종료 후 다음 UI 요청에서 검증된 현재 generation agent를 다시 시작하며 상시 감시 서비스는 두지 않는다. 요청의 완료 여부가 불명확하면 기존 operation/receipt로 상태를 확인한다. UI의 임의 재시도로 같은 변경을 두 번 수행하지 않는다.

## 결과

창 종료와 독립적인 작업 유지, 트레이 하나, 백그라운드 권한 검사 지점 하나를 제공한다. 같은 설치의 제품 연결마다 사용자의 추가 연결 승인을 요구하지 않는다. 프로세스 하나와 업데이트 종료 단계, 재연결·장애 진단 및 디버깅 대상이 늘어난다.

B9는 codec·클라이언트·검증된 pipe server와 runtime/webhook/collector owner를 연결한다. Knowledge 저장소가 없으면 수집기를 만들지 않고 요청 때 다시 확인한다. 활동 동의가 없으면 검색 색인만 시작하고 활동 조회·설정은 일시중지 상태로 준비한다. 노트·vault·미저장 확인·native 파일 열기와 제품 간 handoff 발행은 제품 host에 남는다.

명시적 전체 종료는 인증된 연결에 종료를 알리고 설정 변경·실행·리스너·수집/색인 쓰기를 합류시킨다. 이미 열린 제품의 주기적 조회는 agent를 다시 띄우지 않는다. 사용자의 수동 재연결·새 제품 실행은 다시 시작할 수 있고, 다른 제품이 시작한 검증된 agent에는 기존 제품도 연결할 수 있다. 비정상 종료와 의도적 종료를 구분하며 프로토콜 교체도 native process 종료와 pipe 소실을 확인한 뒤 진행한다.

Workspace 원본에 연결된 작업은 예약 실행을 포함해 실행 직전에 원본·승인·작업 투영을 다시 확인한다. Development Sessions는 agent가 준비 참조·lease·실행 receipt를 보유하고 UI는 재연결 시 기존 receipt로 복구한다. agent 재시작으로 owner lease가 사라지면 Degraded로 표시하고 작업을 자동 재실행하지 않는다. Webhook bind/켜짐은 저장하지만 일반 응답 규칙·요청 이력은 기존처럼 메모리 상태이고 agent 재시작 시 비워진다.

## 근거

- [기존 bus와 framing](../../crates/suite-runtime/src/platform/component_bus.rs), [OS peer 이미지 검사](../../crates/suite-runtime/src/platform/peer_identity.rs)
- [설치 identity·writer lease](../../crates/product-shell-tauri/src/installation.rs), [bootstrap의 exclusive writer gate](../../apps/devbox-control-center/src-tauri/src/bootstrap.rs)
- [quiesced data checkpoint](../../apps/devbox-control-center/src-tauri/src/core/data_checkpoint.rs), [activation 계약](../../crates/product-contract/src/activation.rs)
- [설치 namespace ADR 0007](0007-data-namespaces.md), [보안 범위 ADR 0016](0016-personal-security-scope.md)
- [P1-19 계획](../superpowers/plans/2026-09-23-review-remediation/p1-19-agent-adr-and-protocol.md), 같은 묶음 P1-11–14의 typed 요청 및 P1-16 stream 계약, 리뷰 §8
- 진행 원장: [ledger #580](https://github.com/jihoon22-lee/devbox/issues/580)
