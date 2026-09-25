# 0012 고정 스키마 운영 로그

상태: 채택

기록일: 2026-09-25

## 맥락

문제 재현에 호출 종류와 결과는 필요하지만 인자·본문·경로를 로그에 남길 필요는 없다. 자유 문장 로그는 비밀값이 섞일 가능성을 만든다.

## 결정

제품별 날짜 JSONL에 코드·component·method·시간·결과만 기록한다. 14일 보관하고 하루 4MiB와 한도 표식을 적용한다. 외부 전송은 없으며 지원 번들은 검토한 요약을 내보낸다.

## 결과

느린 성공과 실패·취소·거부·panic을 진단할 수 있다. 검증 전 이름과 자유 문장은 digest가 되므로 원문 메시지 복원은 불가능하고 로그 실패가 명령을 실패시키지는 않는다.

## 근거

- [crates/product-contract/src/operation_log.rs](../../crates/product-contract/src/operation_log.rs)
- [crates/product-shell-tauri/src/operation_log.rs](../../crates/product-shell-tauri/src/operation_log.rs)

리뷰 후속 결정·진행: [ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).
