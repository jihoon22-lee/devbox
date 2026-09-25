# 0004 native 소유 권한

상태: 채택

기록일: 2026-09-25

## 맥락

여러 기능을 한 화면에 합쳐도 각 기능의 파일·프로세스·네트워크 권한은 같지 않다. 화면 전환을 권한 승인으로 사용하면 안 된다.

## 결정

카탈로그의 component authority와 각 제품 native allowlist로 권한을 정한다. route 선택과 렌더러 상태는 권한을 넓히지 않는다.

## 결과

제품 간 context 전달에도 소유자 확인을 유지한다. 새 메서드는 UI뿐 아니라 native 허용 목록과 검증 경로를 함께 추가해야 한다.

## 근거

- [apps/products.json](../../apps/products.json)
- [apps/devbox-workspace/src-tauri/src/component.rs](../../apps/devbox-workspace/src-tauri/src/component.rs)
- [apps/devbox-knowledge/src-tauri/src/component.rs](../../apps/devbox-knowledge/src-tauri/src/component.rs)

리뷰 후속 결정·진행: [ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).
