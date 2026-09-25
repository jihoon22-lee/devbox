# 0014 component별 타입 IPC

상태: 제안

기록일: 2026-09-25

## 맥락

문자열 메서드와 JSON 인자를 여러 계층에서 반복하면 native와 TypeScript 계약이 어긋날 수 있다. 각 기능의 입장 검사와 오류 투영을 유지하면서 타입 경계를 명확히 해야 한다.

## 결정

component별 Tauri command와 공통 admission extractor를 두고 ts-rs 12.0.1로 타입을 생성한다. Knowledge에서 먼저 적용한 뒤 API Studio·Control Center·Workspace로 넓힌다.

## 결과

계약 변경을 컴파일과 생성물 검사로 확인할 수 있다. P1-11–P1-14 구현과 검증이 끝나기 전에는 이 설계를 구현 완료로 취급하지 않는다.

## 근거

- [docs/superpowers/plans/2026-09-23-review-remediation/p1-11-typed-ipc-foundation.md](../../docs/superpowers/plans/2026-09-23-review-remediation/p1-11-typed-ipc-foundation.md)
- [docs/superpowers/plans/2026-09-23-review-remediation/p1-12-typed-ipc-knowledge.md](../../docs/superpowers/plans/2026-09-23-review-remediation/p1-12-typed-ipc-knowledge.md)

리뷰 후속 결정·진행: [ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).
