# Codex 작업 환경

저장소 작업 지침은 [AGENTS](../AGENTS.md)와
[CONVENTIONS §11](../CONVENTIONS.md#11-codex-지침스킬작업-기록)이 원장이다.
현재 작업 범위는 [제품 재정비 계획](superpowers/plans/2026-10-03-product-readiness/00-roadmap.md)을 따른다.

- 개인 모델·context·계정 설정을 저장소 설정으로 강제하지 않는다. Windows/WSL 호스트와
  기존 개발 경로를 보존하며, 문제와 무관한 daemon·서비스·설정 변경을 수행하지 않는다.
- 이 작업의 하위 에이전트는 사용자가 허용한 GPT-6 Astra·GPT-6.1 Sol 범위에서 사용한다.
  최종 병렬 리뷰는 GPT-6 Astra로 진행한다. 실제 동시 실행 한도와 메모리·프로세스 여유를 지킨다.
- 무거운 로컬 검증은 [검증 운영](verification.md)의 공통 잠금·자원 제한을 사용한다.
  에이전트 수를 늘려 같은 전체 빌드·테스트를 동시에 실행하지 않는다.
- 개발·migration·릴리스에 별도 스킬을 요구하지 않는다. [릴리스 정책](release-policy.md)을
  직접 따르며 제거한 release 스킬을 다시 만들지 않는다.
- 작업 결정·영향·검증·미실행 항목은 통합 PR 본문과 현 계획 추적표에 남긴다.
  닫힌 #580과 과거 계획을 실행 원장으로 재사용하지 않는다.
- 개인 설정의 변경·복구가 명시적으로 요청되면 해당 호스트의 현재 버전과 지원 설정을 확인한다.
  인증·전체 개인 설정을 저장소나 PR에 복사하지 않는다. 플러그인 cache를 직접 수정하지 않는다.

2026-09-07의 개인 모델/context 설정과 당시 확인 결과는
[고정된 과거 기록](https://github.com/jihoon22-lee/devbox/blob/ab2ee78a54d3aef7a0e2d546349574bc12a4c2ac/docs/codex-setup.md)에 보존한다.
당시 수치·계정 지원 조건을 현재 권장값으로 재사용하지 않는다.
