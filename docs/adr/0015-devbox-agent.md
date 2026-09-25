# 0015 사용자별 백그라운드 agent

상태: 제안

기록일: 2026-09-25

## 맥락

런타임·웹훅·수집기를 UI 프로세스에만 두면 창 수명과 작업 수명이 결합된다. 공용 소유자가 필요한 작업과 제품에 남길 작업을 구분한다.

## 결정

사용자별 headless devbox-agent로 런타임·웹훅·수집기를 옮기고 UI는 클라이언트로 둔다. 터미널은 Workspace에 남긴다. 상세 RPC·수명 설계는 P1-19에서 확정한다.

## 결과

창과 독립적인 작업 관리 및 트레이 하나를 제공할 수 있다. agent 실패·재연결·소유권 복구 계약이 추가되며 아직 구현 완료된 기능은 아니다.

## 근거

- [docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md](../../docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md)
- [docs/superpowers/plans/2026-09-23-review-remediation/p1-19-agent-adr-and-protocol.md](../../docs/superpowers/plans/2026-09-23-review-remediation/p1-19-agent-adr-and-protocol.md)

리뷰 후속 결정·진행: [ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).
