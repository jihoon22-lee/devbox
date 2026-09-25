# 0008 사용자 범위 비밀 봉인

상태: 채택

기록일: 2026-09-25

## 맥락

API와 Runtime은 비밀값을 저장하고 실행 시에만 사용해야 한다. 비밀 참조와 실제 평문을 같은 객체로 취급하지 않는다.

## 결정

비밀은 Windows DPAPI CurrentUser로 봉인하고 평문 내보내기를 제공하지 않는다. 공유 Sealer 계약과 버전 envelope를 유지하며 OS 구현 통합은 P1-09에서 수행한다.

## 결과

소유 사용자 범위에서 값을 복원한다. 다른 사용자나 PC로의 자동 평문 이전을 보장하지 않으며 개인용 보안 범위는 ADR 0016을 따른다.

## 근거

- [crates/secrets/src/lib.rs](../../crates/secrets/src/lib.rs)
- [docs/superpowers/plans/2026-09-23-review-remediation/p1-09-secrets-dpapi.md](../../docs/superpowers/plans/2026-09-23-review-remediation/p1-09-secrets-dpapi.md)

리뷰 후속 결정·진행: [ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).
