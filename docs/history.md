# 끝난 계획과 이전 기록

v1 재구축의 끝난 계획 문서와 줄이기 전 진행 원장 전문을 가리킨다. 현재 규칙은 [CONVENTIONS](../CONVENTIONS.md), 설계는 [01-design](superpowers/plans/2026-10-08-devbox-v1/01-design.md), 진행 상태는 [PROGRESS](superpowers/plans/2026-10-08-devbox-v1/PROGRESS.md), 지금 계획은 [10-s7-s8-release](superpowers/plans/2026-10-08-devbox-v1/10-s7-s8-release.md)다.

- 아래 계획 문서는 S8-2(2026-11-03)가 트리에서 지웠다(사용자 결정 4). 내용은 S7 머지 커밋 `842fa82e`(#651)와 같다.
- 원문은 고정 링크로 보거나 저장소에서 `git show <커밋>:<경로>`로 읽는다.
- 기록의 PASS·FAIL·NOT_RUN은 그 기록이 적은 source SHA의 결과다. 지금 상태로 읽지 않는다.
- 지금 계획 10은 1.0.0 공개 기록 뒤(S8-6) 여기로 옮긴다.

## v1 계획 문서

경로는 모두 `docs/superpowers/plans/2026-10-08-devbox-v1/` 아래다.

| 문서 | 내용 | 원문 |
|---|---|---|
| `00-roadmap.md` | 로드맵: 하위 프로젝트 순서, 검증과 실사용(§2.2), 실행 규칙(§3, 2026-10-27 CONVENTIONS로 옮김), v0.9.0 회피 목록(§4), 2026-10-08 결정(§5). §7 추적표는 01-design §9.4로 옮겼다 | [고정 링크](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/00-roadmap.md) · `git show 842fa82e:docs/superpowers/plans/2026-10-08-devbox-v1/00-roadmap.md` |
| `02-s0a-spike.md` | S0a 구조 확인 실험 계획 | [고정 링크](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/02-s0a-spike.md) · `git show 842fa82e:docs/superpowers/plans/2026-10-08-devbox-v1/02-s0a-spike.md` |
| `03-s0b-skeleton.md` | S0b 걷는 뼈대 1/2: 저장소·프로토콜·데몬(#638) | [고정 링크](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/03-s0b-skeleton.md) · `git show 842fa82e:docs/superpowers/plans/2026-10-08-devbox-v1/03-s0b-skeleton.md` |
| `03b-s0b-app.md` | S0b 걷는 뼈대 2/2: 화면 골격·Windows 껍데기(#638) | [고정 링크](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/03b-s0b-app.md) · `git show 842fa82e:docs/superpowers/plans/2026-10-08-devbox-v1/03b-s0b-app.md` |
| `04-s1-agents-terminal.md` | S1 에이전트·터미널·프로젝트 1/2: 데몬(#640) | [고정 링크](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/04-s1-agents-terminal.md) · `git show 842fa82e:docs/superpowers/plans/2026-10-08-devbox-v1/04-s1-agents-terminal.md` |
| `04b-s1-app.md` | S1 2/2: 화면·Windows 통합(#640) | [고정 링크](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/04b-s1-app.md) · `git show 842fa82e:docs/superpowers/plans/2026-10-08-devbox-v1/04b-s1-app.md` |
| `05-s2-runs.md` | S2 실행·관찰(#642) | [고정 링크](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/05-s2-runs.md) · `git show 842fa82e:docs/superpowers/plans/2026-10-08-devbox-v1/05-s2-runs.md` |
| `06-s3-knowledge.md` | S3 기록(#644) | [고정 링크](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/06-s3-knowledge.md) · `git show 842fa82e:docs/superpowers/plans/2026-10-08-devbox-v1/06-s3-knowledge.md` |
| `07-s4-s6.md` | S4 편집·Git, S5 개발 도구, S6 마무리·출시의 요약 계획 | [고정 링크](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/07-s4-s6.md) · `git show 842fa82e:docs/superpowers/plans/2026-10-08-devbox-v1/07-s4-s6.md` |
| `07a-s4-editor-git.md` | S4 편집·Git 상세(#646) | [고정 링크](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/07a-s4-editor-git.md) · `git show 842fa82e:docs/superpowers/plans/2026-10-08-devbox-v1/07a-s4-editor-git.md` |
| `07b-s5-tools.md` | S5 개발 도구(#645) | [고정 링크](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/07b-s5-tools.md) · `git show 842fa82e:docs/superpowers/plans/2026-10-08-devbox-v1/07b-s5-tools.md` |
| `07c-s6-release.md` | S6 마무리·출시(#648, 임시 RC #649). §14 성공 기준 판정 원장, §16 S6-6c 공개 직전 13개 확인(PROGRESS §3으로 옮김) | [고정 링크](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/07c-s6-release.md) · `git show 842fa82e:docs/superpowers/plans/2026-10-08-devbox-v1/07c-s6-release.md` |
| `08-review-record.md` | 01-design 초안 검토와 세 계획 통합·검수(§8·§9) | [고정 링크](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/08-review-record.md) · `git show 842fa82e:docs/superpowers/plans/2026-10-08-devbox-v1/08-review-record.md` |
| `09-s0a-results.md` | S0a 구조 확인 결과(ADR 0017의 근거) | [고정 링크](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/09-s0a-results.md) · `git show 842fa82e:docs/superpowers/plans/2026-10-08-devbox-v1/09-s0a-results.md` |

## 줄이기 전 진행 원장

S8-2가 [PROGRESS](superpowers/plans/2026-10-08-devbox-v1/PROGRESS.md)를 200줄 이하로 줄였다(사용자 결정 4). 이전 전문은 아래에 있다.

- S0–S7-13: S7 머지 `842fa82e`. [고정 링크](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/PROGRESS.md) · `git show 842fa82e:docs/superpowers/plans/2026-10-08-devbox-v1/PROGRESS.md`
- S7-14–S7-16과 S8 착수(S8-0이 이 브랜치에 기록): S8-2a 첫 커밋 `71b42eb4`(전문을 담은 마지막 PROGRESS). [고정 링크](https://github.com/jihoon22-lee/devbox/blob/71b42eb44b663e193d32f49f5759d27771a05e25/docs/superpowers/plans/2026-10-08-devbox-v1/PROGRESS.md) · `git show 71b42eb4:docs/superpowers/plans/2026-10-08-devbox-v1/PROGRESS.md`. S8 PR은 squash로 머지하므로 이 커밋은 PR에만 남는다. S8 PR의 영구 링크는 S8-6에서 더한다.

## v0.9 기록

- `72ff50c7`(#637, 2026-10-06): `v0.9.0` 태그(`bc48a839`, #636) 다음의 main 커밋이고 v1 계획의 기준이다. v0.9 네 제품의 코드, v0.9의 `docs/history.md`(그 이전 계획·감사 목록), 2026-10-03 계획과 추적표(`docs/superpowers/plans/2026-10-03-product-readiness/`)가 있다. [고정 링크](https://github.com/jihoon22-lee/devbox/tree/72ff50c7d992fccb54ca51d796cfa7f5b29c7caa) · `git show 72ff50c7:docs/history.md`

## 이 PC에만 있는 근거

- 기록이 가리키는 `devbox-verification/…`(Git common-dir `.git/devbox-verification/`)과 그 압축 보관(`.git/devbox-archive/`), 과제 기록 `.superpowers/sdd/`는 작성자의 PC에만 있다. 저장소와 GitHub에는 없다. 위치와 보관 해시는 [PROGRESS §6](superpowers/plans/2026-10-08-devbox-v1/PROGRESS.md#6-근거-보관)에 있다.
