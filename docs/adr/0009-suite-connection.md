# 0009 같은 설치 제품의 자동 연결

상태: 채택

기록일: 2026-09-25

## 맥락

제품 간 기능을 쓸 때마다 연결 승인을 반복하면 일상 흐름이 끊긴다. 다른 설치나 generation의 peer를 같은 제품으로 신뢰할 수는 없다.

## 결정

같은 설치의 제품은 named pipe bus로 기본 자동 연결한다. 사용자는 설정에서 끄며 선호는 suite-connection-v2.json에 제품·설치 단위로 저장한다.

## 결과

업데이트 뒤에도 연결 끄기 선호를 유지한다. peer·설치·generation 검증은 남기고 잘못된 설정을 자동 연결 기본값으로 덮지 않는다.

## 근거

- [crates/suite-runtime/src/preference.rs](../../crates/suite-runtime/src/preference.rs)
- [crates/suite-runtime/src/lib.rs](../../crates/suite-runtime/src/lib.rs)

리뷰 후속 결정·진행: [ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).
