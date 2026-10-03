# 0009 같은 설치 제품의 자동 연결

상태: 채택

기록일: 2026-09-25

## 맥락

제품 간 기능을 쓸 때마다 연결 승인을 반복하면 일상 흐름이 끊긴다. 다른 설치나 generation의 peer를 같은 제품으로 신뢰할 수는 없다.

## 결정

같은 설치의 제품은 named pipe bus로 기본 자동 연결한다. 사용자는 설정에서 끄며 선호는 suite-connection-v2.json에 제품·설치 단위로 저장한다.

## 결과

업데이트 뒤에도 연결 끄기 선호를 유지한다. peer·설치·generation 검증은 남기고 잘못된 설정을 자동 연결 기본값으로 덮지 않는다.

자동 연결 준비 중의 제품 간 호출은 원래 요청 기한 안에서만 연결 결과를 기다린다.
꺼짐·검증 실패·수동 검토 결정은 즉시 반영하고, 연결 준비 대기는 업무 호출을 재실행하지 않는다.
설정 상태는 준비 중/연결됨/꺼짐/실패를 구분한다.

## 근거

- [crates/suite-runtime/src/preference.rs](../../crates/suite-runtime/src/preference.rs)
- [crates/suite-runtime/src/lib.rs](../../crates/suite-runtime/src/lib.rs)

현재 구현·검증: [재정비 계획](../superpowers/plans/2026-10-03-product-readiness/00-roadmap.md). 과거 결정: [닫힌 ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).
