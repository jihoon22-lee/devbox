# 0011 과제 회귀와 묶음 최종 검증

상태: 채택

기록일: 2026-09-25

## 맥락

전체 workspace 검증을 과제마다 반복하면 같은 빌드와 테스트에 시간을 쓴다. 과제의 회귀 증거와 최종 통합 증거를 구분한다.

## 결정

과제별로 실패를 재현하는 좁은 회귀를 실행하고, 구현을 통합한 뒤 verify:affected·Clippy·필요한 Windows 검증을 한 번 모은다. 실패 후에는 수정들을 모아 실패·영향 범위만 재검증한다. 현재 실행 순서와 성공 CI 근거 재사용은 CONVENTIONS §5·8과 검증 운영 문서가 원장이다.

## 결과

최종 게이트를 유지하면서 중복 검증을 줄인다. 실행하지 않은 환경은 PASS로 쓰지 않는다. 선행 변경을 통합 개발 브랜치에 반영하면 후속 구현을 진행하고, 작업마다 main 머지나 CI 대기를 만들지 않는다.

## 근거

- [docs/verification.md](../../docs/verification.md)
- [CONVENTIONS.md](../../CONVENTIONS.md)
- [docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md](https://github.com/jihoon22-lee/devbox/blob/ab2ee78a54d3aef7a0e2d546349574bc12a4c2ac/docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md)

현재 구현·검증: [재정비 계획](../superpowers/plans/2026-10-03-product-readiness/00-roadmap.md). 과거 결정: [닫힌 ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).
