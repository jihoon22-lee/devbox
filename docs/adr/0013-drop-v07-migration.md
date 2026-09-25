# 0013 v0.7 가져오기 종료

상태: 채택

기록일: 2026-09-25

## 맥락

사용자는 두 PC에서 v0.7 데이터를 가져오지 않고 새로 구축하기로 결정했다. 현재 기능과 과거 가져오기 코드가 섞여 유지 비용이 커졌다.

## 결정

다음 공개 버전 v0.9.0에서 v0.7 발견·가져오기·정리 진입점을 제거한다. 현재 v0.8.1 데이터의 JSON 형식과 템플릿·프로필·LSP·세션은 유지한다.

## 결과

처음 실행 흐름을 단순화한다. 남아 있는 폐기된 참조는 조용히 삭제하지 않고 명시적 오류로 보존하며 과거 근거는 비공개 보관소에 둔다.

## 근거

- [docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md](../../docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md)
- [docs/superpowers/plans/2026-09-23-review-remediation/p1-04-remove-v07-workspace.md](../../docs/superpowers/plans/2026-09-23-review-remediation/p1-04-remove-v07-workspace.md)

리뷰 후속 결정·진행: [ledger #580](https://github.com/jihoon22-lee/devbox/issues/580).
