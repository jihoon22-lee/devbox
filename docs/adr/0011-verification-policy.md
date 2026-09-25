# 0011 과제 회귀와 묶음 최종 검증

상태: 채택

기록일: 2026-09-25

## 맥락

전체 workspace 검증을 과제마다 반복하면 같은 빌드와 테스트에 시간을 쓴다. 과제의 회귀 증거와 최종 통합 증거를 구분한다.

## 결정

로드맵 D4는 과제별 실패 테스트와 좁은 회귀, 묶음 끝 verify:affected·Clippy·Windows 검증을 정한다. 실패 후에는 수정들을 모아 실패·영향 범위만 재검증한다. 이번 실행의 2026-09-25 사용자 직접 지시는 테스트를 개발 중 작성하되 묶음 구현 완료 후 모아서 실행하도록 우선 적용했고 PR에 기록한다.

## 결과

최종 게이트를 유지하면서 중복 검증을 줄인다. 실행하지 않은 환경은 PASS로 쓰지 않으며 선행 묶음의 main CI가 끝나야 후속 구현을 시작한다.

## 근거

- [docs/verification.md](../../docs/verification.md)
- [CONVENTIONS.md](../../CONVENTIONS.md)
- [docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md](../../docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md)

리뷰 후속 결정·진행: [ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).
