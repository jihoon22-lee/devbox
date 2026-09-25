# 0007 설치별 제품 데이터 namespace

상태: 채택

기록일: 2026-09-25

## 맥락

같은 PC의 서로 다른 설치가 같은 데이터를 자동으로 공유하면 업데이트와 제거의 소유권이 섞인다. 제품 구분과 설치 구분을 모두 보존해야 한다.

## 결정

제품 데이터는 %LOCALAPPDATA%/<identifier>.i<설치 접미사>에 둔다. 같은 Suite 설치는 접미사를 공유하고 제품 identifier로 나눈다. v0.8.1 설치 키의 native components() 및 직렬화 계약을 유지한다.

## 결과

설치 간 데이터 혼동을 줄이고 generation 교체 뒤에도 같은 데이터를 찾는다. ID 표현 변경만으로 새 namespace를 만들지 않도록 회귀 검사가 필요하다.

## 근거

- [crates/product-shell-tauri/src/lib.rs](../../crates/product-shell-tauri/src/lib.rs)
- [crates/product-shell-tauri/src/installation.rs](../../crates/product-shell-tauri/src/installation.rs)
- [crates/suite-runtime/src/platform/component_scope.rs](../../crates/suite-runtime/src/platform/component_scope.rs)

리뷰 후속 결정·진행: [ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).
