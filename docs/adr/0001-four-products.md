# 0001 네 제품 구성

상태: 채택

기록일: 2026-09-25

## 맥락

기능별 앱을 따로 배포하면 설치와 창, 제품 연결을 반복해야 한다. 현재 카탈로그는 네 사용자 제품과 내부 component를 구분한다.

## 결정

Workspace·API Studio·Knowledge·Control Center 네 제품으로 유지한다. 내부 기능을 별도 사용자 제품으로 다시 쪼개지 않는다.

## 결과

설치와 탐색 비용을 줄인다. 기능 소유권과 권한은 각 제품의 native component 경계에서 계속 분리해야 한다.

## 근거

- [apps/products.json](../../apps/products.json)
- [docs/architecture/v0.8-foundation.md](../../docs/architecture/v0.8-foundation.md)

리뷰 후속 결정·진행: [ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).
