# 0002 세대별 Suite 설치

상태: 대체됨(0017)

기록일: 2026-09-25

## 맥락

실행 파일 교체 중 기존 프로세스가 데이터를 쓰면 복구 경계가 불명확해진다. 설치 선언과 실제 열린 파일의 검증을 구분해야 한다.

## 결정

한 설치 루트의 generations/<g>/products/<p>/에 제품을 두고 devbox-installation.json과 devbox-activation.json으로 import·health·committed·recover 단계를 관리한다. writer lease로 교체 중 새 writer를 막는다.

## 결과

이전 generation과 사용자 데이터를 보존하면서 업데이트할 수 있다. 상태 기록과 열린 writer의 수명을 함께 검증해야 한다.

## 근거

- [crates/product-shell-tauri/src/installation.rs](https://github.com/jihoon22-lee/devbox/blob/72ff50c7d992fccb54ca51d796cfa7f5b29c7caa/crates/product-shell-tauri/src/installation.rs)
- [crates/product-contract/src/activation.rs](https://github.com/jihoon22-lee/devbox/blob/72ff50c7d992fccb54ca51d796cfa7f5b29c7caa/crates/product-contract/src/activation.rs)

현재 결정: [ADR 0017](0017-rebuild-single-app-wsl-daemon.md) · [v1 설계](../superpowers/plans/2026-10-08-devbox-v1/01-design.md). 과거 결정: [닫힌 ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).
