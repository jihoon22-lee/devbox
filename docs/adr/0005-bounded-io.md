# 0005 외부 입력과 작업의 상한

상태: 채택

기록일: 2026-09-25

## 맥락

파일·HTTP·IPC 입력은 신뢰할 수 없고 무제한 할당이나 대기는 앱 전체에 영향을 준다. 크기 제한과 실행 시간 제한은 각각 필요하다.

## 결정

외부 입력의 크기·개수·시간을 먼저 제한하고 초과하면 명시적 오류로 끝낸다. 부분 결과를 완전한 성공으로 표시하지 않는다.

## 결과

과도한 입력과 장기 작업의 영향을 제한한다. 상한에 걸린 사용자는 입력을 줄이거나 다시 시도해야 하며 OS 호출 자체의 강제 중단까지 보장하지 않는다.

## 근거

- [crates/webhook-core/src/core/http.rs](../../crates/webhook-core/src/core/http.rs)
- [apps/devbox-knowledge/src-tauri/src/component.rs](../../apps/devbox-knowledge/src-tauri/src/component.rs)

리뷰 후속 결정·진행: [ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).
