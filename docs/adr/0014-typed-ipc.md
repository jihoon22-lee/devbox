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

계약 변경을 컴파일과 생성물 검사로 확인할 수 있다. P1-11–P1-14 구현과 검증이 끝나기 전에는 이 설계를 구현 완료로 취급하지 않는다.

## 근거

- [docs/superpowers/plans/2026-09-23-review-remediation/p1-11-typed-ipc-foundation.md](../../docs/superpowers/plans/2026-09-23-review-remediation/p1-11-typed-ipc-foundation.md)
- [docs/superpowers/plans/2026-09-23-review-remediation/p1-12-typed-ipc-knowledge.md](../../docs/superpowers/plans/2026-09-23-review-remediation/p1-12-typed-ipc-knowledge.md)

리뷰 후속 결정·진행: [ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).
