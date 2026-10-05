# 이전 계획·감사·측정 기록

현재 구현·출시 작업은 [2026-10-03 계획](superpowers/plans/2026-10-03-product-readiness/00-roadmap.md),
운영 규약은 [CONVENTIONS](../CONVENTIONS.md), 설계 결정은 [ADR](adr/README.md)이 원장이다.
완료되거나 대체된 실행 지시와 당시의 미완료 표시는 아래 고정 commit에서 보존한다.
과거 PASS·공개 완료·사용자 실기 요청을 현재 v0.9.0 수정본의 상태로 해석하지 않는다.

## 통합한 문서

| 기록 | 당시 범위와 현재 관계 | 원문 |
|---|---|---|
| 2026-08 제품 기회·실행 계획 | v0.4 실행 완료 및 옛 독립 앱 구성. 현 네 제품 목록을 대체하지 않음 | [제품 기회 원문](https://github.com/jihoon22-lee/devbox/blob/ab2ee78a54d3aef7a0e2d546349574bc12a4c2ac/docs/product-opportunities.md) |
| 2026-08-19 감사 | 이슈 #176, v0.5 이전 제품의 코드·Windows 미실행 구분 | [감사 원문](https://github.com/jihoon22-lee/devbox/blob/ab2ee78a54d3aef7a0e2d546349574bc12a4c2ac/docs/superpowers/reports/2026-08-19-issue-176-code-audit.md) |
| 2026-09-02 WSL Desktop 검토 | 옛 WSL Desktop의 터미널 UX 및 #521–#534 후속 이력 | [검토 원문](https://github.com/jihoon22-lee/devbox/blob/ab2ee78a54d3aef7a0e2d546349574bc12a4c2ac/docs/superpowers/reports/2026-09-02-wsl-desktop-usability-review.md) |
| v0.8 설계·측정 | 네 제품 전환 당시 구조, namespace·migration·초기 성능 근거 | [Foundation](https://github.com/jihoon22-lee/devbox/blob/ab2ee78a54d3aef7a0e2d546349574bc12a4c2ac/docs/architecture/v0.8-foundation.md), [Workspace](https://github.com/jihoon22-lee/devbox/blob/ab2ee78a54d3aef7a0e2d546349574bc12a4c2ac/docs/architecture/v0.8-workspace.md), [Knowledge](https://github.com/jihoon22-lee/devbox/blob/ab2ee78a54d3aef7a0e2d546349574bc12a4c2ac/docs/architecture/v0.8-knowledge.md) |
| v0.8 원시 측정 | 해당 source·환경의 기록이며 새 후보 PASS가 아님 | [baseline JSON](https://github.com/jihoon22-lee/devbox/blob/ab2ee78a54d3aef7a0e2d546349574bc12a4c2ac/docs/architecture/v0.8-baseline-measurements.json), [Knowledge WSL2 JSON](https://github.com/jihoon22-lee/devbox/blob/ab2ee78a54d3aef7a0e2d546349574bc12a4c2ac/docs/evidence/v0.8-knowledge-wsl2.json) |
| 2026-09-23 실행 계획 | B1–B12와 철회된 첫 v0.9.0까지의 계획·감사. 현재 R00–R16으로 대체 | [원장과 전체 계획](https://github.com/jihoon22-lee/devbox/tree/ab2ee78a54d3aef7a0e2d546349574bc12a4c2ac/docs/superpowers/plans/2026-09-23-review-remediation) |
| 2026-10-03 이후 후보 진단 상세 | 통합 PR #616 이후의 시행별 로그. 현 추적표에는 작업 매핑과 최신 상태만 유지 | [당시 전체 추적표](https://github.com/jihoon22-lee/devbox/blob/ab2ee78a54d3aef7a0e2d546349574bc12a4c2ac/docs/superpowers/plans/2026-10-03-product-readiness/08-traceability.md), [#616](https://github.com/jihoon22-lee/devbox/pull/616) |

이전 실행 계획 42개와 기타 보존 문서·측정 파일을 포함한 50개 파일의
원문은 삭제 전 공개 main commit `ab2ee78a54d3aef7a0e2d546349574bc12a4c2ac`에 남아 있다. Git 이력과 PR 근거를
보존하므로 과거 결정·코드 예제·검증 명령이 필요할 때 정확한 시점의 내용을 확인할 수 있다.

## 기존 후속 후보의 보존

아래는 기존 계획의 미포함 후보이며 이번 재출시에 추가로 구현하기로 승인된 작업이 아니다.
이미 구현된 부분과 구현하지 않은 기능을 혼동하지 않도록 유지한다.

- WSL 설정·배포판 백업·VHDX 관리 UI, 통합 비밀 사용처/`.env` 관리.
- Knowledge 전역 빠른 기록·백링크 그래프·프로젝트 연결·ADR 템플릿.
- API Studio NTLM/Negotiate·웹훅 터널/LAN 공유·데이터 파일 반복 실행·파일 컬렉션 양방향 동기화.
- 터미널·Launcher의 agent 이관, 영어 UI·라이트/고대비 테마, 남은 integration 계층 정리.
- 서명·권한 강화 등 명시적으로 제외한 보안 범위는 [ADR 0016](adr/0016-personal-security-scope.md)을 따른다.

현재 사용 안내·공개 버전·설치 제한은 [Windows 안내](windows-guide.md)와
[릴리스 근거](release-evidence.md)를 따른다. 닫힌 #580을 다시 실행 원장으로 사용하지 않는다.
