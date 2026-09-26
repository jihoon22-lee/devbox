# 0015 사용자별 백그라운드 agent

상태: 채택(구현은 Phase 2)

기록일: 2026-09-25 · 확정일: 2026-09-26

## 맥락

현재 런타임 작업·서비스·웹훅 리스너·활동 수집은 제품 UI 프로세스에 있다. 창 종료나 UI 장애가 작업 수명에 영향을 주며 제품별 트레이 아이콘도 최대 세 개다. 네 제품에 흩어진 백그라운드 작업의 연결·권한 처리를 하나의 프로세스에서 관리한다(리뷰 §8 C안).

Workspace 터미널은 WSL 배포판의 zellij·tmux 세션에 연결한다. 세션은 이미 UI 수명과 분리되어 있고 pane의 권한은 창별 peer/session guard에 묶여 있다. 이 경계까지 옮기는 비용을 백그라운드 작업 분리와 섞지 않는다.

## 결정

### 프로세스와 배포

동일 사용자 세션·Suite 설치마다 `devbox-agent.exe` 하나를 둔다. 서로 다른 설치는 ADR 0007의 namespace와 설치 접미사로 분리한다. 소스는 `apps/devbox-agent`이고 **창 없는 Tauri 앱**으로 만든다. 기존 engine의 `tauri::AppHandle`·plugin·managed state를 그대로 이용하되 WebView를 만들지 않으므로 agent를 위한 WebView2 프로세스는 없다.

Control Center 패키지의 `resources/suite/devbox-agent.exe`로 배포한다. Suite bootstrap처럼 내부 구성요소로 취급해 공개 자산 7개 계약을 유지한다. 공개 제품 카드나 다섯 번째 사용자 제품은 추가하지 않는다. ADR 0016에 따라 별도 코드 서명·암호화·인증 토큰·pipe DACL 강화는 이번 범위에 추가하지 않는다.

어느 제품이든 시작할 때 같은 설치의 agent가 없으면 시작한다. 시작 경쟁은 설치 범위의 단일 소유권으로 합친다. 로그인 자동 시작은 기본 꺼짐인 설정이며, Phase 2에서 Workspace의 기존 `--background` 경로를 대체한다. agent가 제품 열기·종료 메뉴가 있는 트레이 아이콘 하나를 소유한다.

### 소유권 이전 순서

| 단계 | agent로 옮기는 작업 |
|---|---|
| P2-02 | 런타임 작업·서비스·스케줄 |
| P2-03 | 웹훅 리스너 |
| P2-04 | 활동 수집·검색 색인, 트레이 통합·자동 시작 |

터미널·파일 편집·LSP·Git 검토·UI 상태는 제품 프로세스에 남는다. 창을 닫아도 agent 작업은 유지되지만 제품별 route·project context·권한은 계속 별도로 검사한다. 실행 주체 통합이 제품 권한 통합이나 자동 신뢰를 뜻하지 않는다.

### IPC와 입장 검사

named pipe는 `\\.\pipe\devbox-agent-<설치 접미사>`다. 접미사는 ADR 0007의 기존 설치 key에서 얻고 임의 JSON 필드를 namespace로 사용하지 않는다. 양쪽은 `suite-runtime`의 OS pipe witness·process handle·실행 이미지·설치 generation 검사와 같은 경계를 적용한다. 제품은 동일 활성 generation의 agent인지 확인하고, agent는 동일 generation의 네 제품만 받는다. Phase 2에서 현재 제품 이미지 목록에 agent의 검증된 내부 resource 경로를 명시적으로 연결해야 한다. pipe 이름이나 `Hello.product`만으로 인증하지 않는다.

`crates/agent-protocol`의 프레임은 4바이트 little-endian 길이와 JSON 본문이다. 본문 상한은 1 MiB, 출력 batch는 최대 64 KiB다. 큰 파일 저장은 agent로 옮기지 않는다. 길이 0·상한 초과·잘못된 JSON·알 수 없는 envelope 필드는 연결을 종료할 수 있는 고정 오류로 처리한다. 부분 수신과 여러 프레임 동시 수신을 지원하며 미완성 프레임 버퍼는 한 프레임 크기로 제한한다.

`Hello`/`Welcome` 뒤 `Call.id`로 여러 요청을 다중화한다. `Call.request`는 기존 `ComponentRequest<C>`의 `{header, method, args}` 그대로다. agent는 component별 typed 역직렬화와 기존 `admit`의 session·route·deadline·replay·lane 검사를 수행한다. OS에서 확인한 제품·설치·generation에 session을 결합한다. codec의 `check_hello`는 버전/형식 검사일 뿐 peer 인증을 대체하지 않는다.

연결의 활성 요청 ID 중복은 `duplicate_request`로 거부하고 완료·취소 때 제거한다. 이는 기존 requestId replay 검사를 대체하지 않는다. 실제 연결 소유자가 기존 admission 한도 안에서 활성 요청 수와 수신 기한을 제한한다. 로그 tail·작업 출력 스트림은 stream ID로 다중화하고 P1-16의 한 batch 전송→실제 소비 완료→정확한 cursor ack 흐름과 고정 ack 기한을 따른다. UI 연결 종료 시 해당 구독을 해제한다. 단순 연결 단절로 이미 수락한 변경 요청을 자동 재전송하지 않는다.

### 업데이트와 writer lease

Control Center의 기존 업데이트/복원 확인 절차를 유지한다. 업데이터는 새 generation 활성화 전에 같은 설치 agent의 admission을 닫고 `Shutdown`을 보내 작업·서비스·리스너·수집기 쓰기를 정리한다. 제품이 소유한 PTY·LSP 등은 기존 제품 종료 절차로 정리한다. agent로 옮기지 않은 PTY를 agent가 소유한다고 가정하지 않는다.

agent도 제품과 같은 `suite-writers.lock`의 shared writer lease를 **전체 수명 및 하위 writer 종료까지** 보유한다. 현재 `WriterGuard::acquire`는 generation의 `products/<product>` 경로를 전제로 하므로 내부 resource 위치의 agent에는 Phase 2에서 검증된 설치 scope를 받는 명시적 연결이 필요하다. 검증되지 않은 실행 경로에서 빈 guard로 진행하면 안 된다.

`Shutdown` 응답 자체를 교체 허가로 사용하지 않는다. 기존 bootstrap 업데이터가 모든 writer 종료를 확인하고 exclusive writer gate를 얻은 뒤 quiesced data checkpoint·검증·기존 activation 단계에 따라 교체한다. shutdown/gate 확보 실패 시 교체하지 않으며 기존 generation과 복구 상태를 보존한다. `suite-update.block`/`suite-data-restore.block` 동안 새 agent 시작도 차단한다. 새 활성 generation의 제품이 새 agent를 시작하며 이전 generation agent와 연결하지 않는다.

프로토콜이 다르면 Welcome 전에 `Rejected { reason: "protocol_mismatch" }`로 거절한다. UI는 “agent를 다시 시작합니다” 상태로 같은 설치·검증된 agent의 종료 및 재시작을 요청한다. 기존 writer/update gate를 우회하거나 이름만 같은 프로세스를 종료하지 않는다. 재시작 후 새 handshake가 성공해야 요청을 보낸다.

### 장애 복구

pipe 단절 시 UI는 1초·2초·4초 간격으로 다시 연결한다. 세 번 실패하면 “백그라운드 서비스를 다시 시작” 버튼을 보인다. agent 비정상 종료 후 다음 UI 요청에서 검증된 현재 generation agent를 다시 시작하며 상시 감시 서비스는 두지 않는다. 요청의 완료 여부가 불명확하면 기존 operation/receipt로 상태를 확인한다. UI의 임의 재시도로 같은 변경을 두 번 수행하지 않는다.

## 결과

창 종료와 독립적인 작업 유지, 트레이 하나, 백그라운드 권한 검사 지점 하나를 제공한다. 같은 설치의 제품 연결마다 사용자의 추가 연결 승인을 요구하지 않는다. 프로세스 하나와 업데이트 종료 단계, 재연결·장애 진단 및 디버깅 대상이 늘어난다.

Phase 1의 산출물은 이 ADR과 순수 codec·메시지·handshake 검사뿐이다. **이 변경으로 제품 실행 동작은 바뀌지 않으며 agent가 실행되지는 않는다.**

## 근거

- [기존 bus와 framing](../../crates/suite-runtime/src/platform/component_bus.rs), [OS peer 이미지 검사](../../crates/suite-runtime/src/platform/peer_identity.rs)
- [설치 identity·writer lease](../../crates/product-shell-tauri/src/installation.rs), [bootstrap의 exclusive writer gate](../../apps/devbox-control-center/src-tauri/src/bootstrap.rs)
- [quiesced data checkpoint](../../apps/devbox-control-center/src-tauri/src/core/data_checkpoint.rs), [activation 계약](../../crates/product-contract/src/activation.rs)
- [설치 namespace ADR 0007](0007-data-namespaces.md), [보안 범위 ADR 0016](0016-personal-security-scope.md)
- [P1-19 계획](../superpowers/plans/2026-09-23-review-remediation/p1-19-agent-adr-and-protocol.md), 같은 묶음 P1-11–14의 typed 요청 및 P1-16 stream 계약, 리뷰 §8
- 진행 원장: [ledger #580](https://github.com/jihoon22-lee/devbox/issues/580)
