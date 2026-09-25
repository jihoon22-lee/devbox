# 0003 native 세션 입장 검사

상태: 채택

기록일: 2026-09-25

## 맥락

렌더러가 제공한 요청 ID와 경로는 권한의 근거가 아니다. 오래된 요청과 다른 창의 재사용을 차단해야 한다.

## 결정

handshake·route·최대 30초 deadline·requestId replay 검사를 native에서 수행한다. provenance도 native가 생성한다.

## 결과

UI와 무관하게 호출의 출처와 수명을 확인한다. 만료 전 요청 ID를 임의로 버리지 않으므로 용량 초과 요청은 거부될 수 있다.

## 근거

- [crates/product-contract/src/lib.rs](../../crates/product-contract/src/lib.rs)
- [crates/product-shell-tauri/src/lib.rs](../../crates/product-shell-tauri/src/lib.rs)

리뷰 후속 결정·진행: [ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).
