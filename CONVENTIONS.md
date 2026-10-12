# devbox — 공통 개발 규약

이 문서는 공통 규약의 원장이다. v1은 Windows 11의 단일 Tauri v2 앱과 WSL Rust 데몬이다.
설계는 [01-design](docs/superpowers/plans/2026-10-08-devbox-v1/01-design.md),
실행 순서는 [00-roadmap](docs/superpowers/plans/2026-10-08-devbox-v1/00-roadmap.md),
진행 상태의 유일한 원장은 같은 폴더의 PROGRESS.md다. 계획 예제와 충돌하면 사용자 지시와 이 규약을 우선한다.

## 1. 환경

pnpm 9, Node 24, Rust 1.98.1 edition 2024를 사용한다. WSL에서 Rust 사용 전 `source ~/.cargo/env`.
편집·프론트 개발·플랫폼 독립 테스트는 WSL에서, 실제 Windows 앱 실행·배포 빌드는 Windows에서 한다.
WSL 성공을 Windows 실기 PASS로 보고하지 않는다. 개인 인증·전역 설정은 변경하거나 저장소에 복사하지 않는다.

## 3. 스택과 코드

Tauri v2·React 19·TypeScript·Vite·순수 CSS와 v1 설계를 따른다. 도메인 crate는 Tauri에 의존하지 않는다.
경계 타입에 `Result<T, String>`·`serde_json::Value`를 쓰지 않는다(RPC RawValue 중계 예외).
RPC는 `method!`·`topic!` 선언에서 TS를 생성한다. 생성물을 손으로 수정하지 않는다.
화면 문구는 한국어이며 docs/ui-terms.md를 따른다. 파일 600줄·컴포넌트 300줄을 넘으면 나눈다.
tracing 로그·TanStack 등 v1 설계가 승인한 스택은 해당 단계에서 구현한다. 불필요한 추측성 공용 모듈은 만들지 않는다.

## 5. 검증

필요한 개발·회귀·CI 검증은 에이전트가 직접 수행한다. 미구현 기능은 해당 구현 단계에서 검증하며 사용자 실기 항목으로 넘기지 않는다. 중간 설치·사용자 실기 요청을 개발·머지 gate로 두지 않는다. 실사용은 계획한 기능을 완성한 뒤 사용자가 진행하며 공개 릴리스의 명시 승인은 별도다.

과제마다 로직의 실패 테스트부터 작성하고 해당 테스트와 직접 영향받는 테스트만 실행한다.
전체 검증·clippy·build·E2E·Windows/WSL 실기는 PR 모든 과제 완료 뒤 한 번 모아 실행한다.
실패를 고친 뒤 실패·영향 범위만 재실행한다. 통과한 무관한 검사는 반복하지 않는다.
완료 커밋·push 전 `pnpm exec biome ci .`를 실행한다. flaky 테스트는 해당 PR에서 수정하거나 제거한다.
`.github`+`scripts`는 합계 1,500줄 이하, 워크플로는 3개 이하를 유지한다.

로컬 검증은 Cargo jobs 2·Vitest workers 2·Rust test threads 2, CPU 4개·nice +10으로 제한한다.
사용 가능한 cgroup에서 memory high 6GiB/max 8GiB·swap max 1GiB를 적용한다.
Git common-dir의 공유 잠금을 비차단으로 획득하고 경합 시 exit 75로 종료한다. 무거운 검증은 worktree 전체에서 하나만 실행한다.
S0b supervisor가 구현되기 전에는 보존된 main의 `.github/scripts/verify-resources.py`를 사용한다.

기존 서비스·tmux·systemd unit·Docker·방화벽·공유 네트워크를 테스트로 변경하지 않는다.
Docker 설치/시작/중지/container/network 조작이나 iptables/nftables·라우팅 변경 검사는 일회성 hosted CI 또는 독립 VM에서만 한다.
별도 WSL/socket/data-root는 네트워크 격리 증거가 아니다. 차단을 옵션·환경 변수로 우회하지 않는다.
테스트는 소유 fixture·고유 인스턴스를 쓰고 소유 자원만 정리한다. 사용자 데이터·secret은 fixture로 쓰지 않는다.
미실행 실기·수동 유보는 PASS와 구분한다.

## 8. Git

시작 시 status/worktree·원격 작업 인증 상태를 확인한다. 원본 main의 사용자 커밋은 보존한다.
v1은 하위 프로젝트마다 `v1/<s>-<name>` 브랜치·전용 worktree 하나를 사용한다.
영어 Conventional Commits로 과제마다 커밋한다. 상세 최종 검증 뒤 push·PR을 한 번 한다.
main은 squash만 사용한다. PR 최종 변경의 `Frontend (pnpm)`, `Rust (Cargo workspace)`, `Rust (Windows)` CI 성공이 필수다.
자동 리뷰가 설정된 PR은 최종 커밋의 코드·보안 리뷰 완료까지 확인한다. 지적은 코드·회귀 근거로 타당성을 판단하고 필요한 수정과 재검증을 마친 뒤 머지한다. 타당하지 않은 지적은 판단 근거를 PR 본문에 남긴다.
보호 규칙을 약화하거나 우회하지 않는다. CI 이전 성공을 재사용하면 같은 입력·범위와 출처를 기록한다.
진행 커밋 완료와 PR 수용 완료를 구분한다. 릴리스 공개는 별도 명시 요청과 수용 근거가 필요하다.
전용 worktree 정리는 clean·merged 확인 → 제거 → prune → 로컬 브랜치 → 원격 브랜치 삭제 순서다.
공유 cache·사용자 자료·dirty/active/locked/unmerged/호스트 소유 worktree는 보존한다.

## 9. 기술 정책

추가 스택은 구체적인 사용자 필요·대안·배포/CI 영향을 확인한다. UI는 Tauri v2 하나다.
v0.9 데이터 이전은 하지 않는다. 기존 버전은 태그와 Git 이력으로 참조한다.

## 11. 기록과 인계

PROGRESS.md가 진행 상태의 유일한 원장이다. 결정·검증·미실행 항목은 현재 계획과 단일 PR 본문·artifact에 기록한다.
새 workthrough·날짜별 복제 보고서를 만들지 않는다. #580을 재오픈하거나 사용자 실기 요청을 만들지 않는다.
인계에는 목표·범위·branch/worktree·결정·검증·다음 과제를 남기고 재개 시 실제 Git/CI와 대조한다.
일반 개발은 별도 스킬을 요구하지 않는다. 승인된 계획은 과제별 구현 → 별도 검토 → 다음 과제 순서로 실행한다.
하위 에이전트는 GPT-6.1 Sol 또는 GPT-6 Astra만 사용한다.
