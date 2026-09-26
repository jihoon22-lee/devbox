# 0014 component별 타입 IPC

상태: 채택

기록일: 2026-09-25

## 맥락

문자열 메서드와 JSON 인자를 여러 계층에서 반복하면 native와 TypeScript 계약이 어긋날 수 있다. 각 기능의 입장 검사와 오류 투영을 유지하면서 타입 경계를 명확히 해야 한다.

## 결정

component별 Tauri command와 공통 명시 admission 함수를 두고 ts-rs 12.0.1로 타입을 생성한다. Knowledge에서 먼저 적용한 뒤 API Studio·Control Center·Workspace로 넓힌다.

CommandArg를 별도로 구현하지 않는다. Tauri의 기본 역직렬화에서 잘못된 메서드·인자가 먼저 거부되면 기존 Problem 응답과 거절 운영 로그를 만들 수 없다. 따라서 닫힌 `IncomingRequest` 봉투를 받고 `admit_request<C>` 내부에서 component enum으로 해석한 뒤 세션·권한·route·동시 실행 한도를 검사한다. 유효한 요청의 header/method/args 형식은 유지한다.

오류 로그에는 native가 선언한 안정된 코드만 기록한다. 분류할 수 없는 원문은 로그나 renderer에 보내지 않고 `unavailable`로 투영한다. DTO에 `TS`를 파생하고 명시적 exporter에서 결과 타입까지 생성한다. 생성 폴더는 exporter가 소유하며, CI는 포맷한 재생성 결과와 tracked/untracked 파일을 함께 검사한다.

## 결과

네 제품 모두 component별 타입 command로 소스 전환을 마쳤다. Workspace의 main 창은 14개 command를 사용하며 기존 `execute` 명령은 제거했다. 보조 터미널 창은 별도의 peer·session 권한을 검사하는 타입 경계를 유지한다. 엔진은 미등록 Tauri command와 `__component_*` shim 대신 닫힌 enum API를 제공한다.

Workspace의 실행 차로 표와 deadline 예산은 Rust가 소유한다. 요청 수와 worker 수는 기존 값을 유지하고, 파일 시스템·context permit을 worker보다 먼저 얻는 순서와 중지·취소의 별도 여유를 보존한다. Workspace에 새로운 전역 quota를 겹쳐 적용하지 않으며, 세션·재전송·route 검사와 운영 로그는 공통 admission을 거친다. 큰 본문 크기는 JSON 재직렬화 없이 검사하고, 기존 native Files/LSP owner에 넘기는 임시 DTO는 작업 대기 전에 해제한다.

`workspace.setup`만 Workspace의 activation import 단계에서 허용한다. 등록 snapshot 등 일반 command에는 같은 예외를 주지 않는다. 현재 프로젝트 정의·Registry는 Workspace owner가, 의존성 분석은 repositories engine이 실행한다. WSL helper의 내부 Source protocol adapter도 같은 닫힌 call enum과 native SourceAccess를 사용한다.

입력·결과 타입과 프런트 deadline 표를 함께 생성하고, 기존 route 허용 표 338개와 비교해 권한 확대를 검사한다. 묶음 검증·CI·Windows 실기 수용 상태는 ledger와 PR을 따른다.

## 근거

- [docs/superpowers/plans/2026-09-23-review-remediation/p1-11-typed-ipc-foundation.md](../../docs/superpowers/plans/2026-09-23-review-remediation/p1-11-typed-ipc-foundation.md)
- [docs/superpowers/plans/2026-09-23-review-remediation/p1-12-typed-ipc-knowledge.md](../../docs/superpowers/plans/2026-09-23-review-remediation/p1-12-typed-ipc-knowledge.md)

리뷰 후속 결정·진행: [ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).
