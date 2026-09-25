# 0016 개인용 보안 범위

상태: 채택

기록일: 2026-09-25

## 맥락

사용자는 개인용 도구에 실제 문제가 없을 정도의 보안을 원한다. 모든 추가 보호 장치를 이번 작업의 필수 조건으로 삼지 않는다.

## 결정

업데이트 서명·Authenticode·pipe DACL 강화·CSP 추가·multipart grant·복구 저널 암호화·네이티브 확인 대화상자는 이번 범위에서 제외한다. 개인정보 fail-closed·링크 거부·DPAPI·로그 비노출·peer 검증·OAuth 봉인은 유지한다.

## 결과

핵심 기능과 실제 데이터 경계에 집중한다. 제외한 장치가 필요해지면 새 ADR로 이 결정을 대체한다.

## 근거

- [docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md](../../docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md)

리뷰 후속 결정·진행: [ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).
