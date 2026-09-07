# Codex 작업 환경

2026-09-07 공식 문서와 로컬 Codex CLI 0.153.4로 확인한 설정이다.
사용자가 요청한 개인 호스트 값이며 저장소 사용자의 공통 모델/계정 요구사항이 아니다.

## 개인 설정

`~/.codex/config.toml`의 기존 설정을 보존하고 아래 값을 병합한다.
기존 `[features.context_management]`가 있으면 중복 table을 추가하지 않고 값을 수정한다.

```toml
model = "gpt-6-astra"
model_reasoning_effort = "high"
model_context_window = 512000
model_auto_compact_token_limit = 430000

[features.context_management]
experimental_mode = true
```

512K/430K는 사용자 선택값이며 OpenAI 권장 기본값이 아니다. 확인 당시 로컬 Astra
카탈로그의 기본 context는 272K였고 API 모델 전체 창은 1,050K였다. 설정값과 실제
클라이언트/계정의 지원·동작을 구분한다. 자동 압축 임계값은 기존 압축 설정이고,
실험적 노트/기록 검색의 내부 동작이 정확히 430K마다 실행된다는 뜻은 아니다.
API의 272K 초과 입력 요금 조건을 ChatGPT 구독 사용량과 동일하게 환산하지 않는다.

설정 변경 후 **새 작업(새 세션)**에서 적용을 확인한다. 이 호스트에서는 사용자가 새 세션의
컨텍스트 변경을 확인했으므로 remote 연결이나 daemon 재시작을 추가로 요구하지 않는다.
새 작업에도 이전 값이 보일 때만 설정 우선순위·호스트를 확인하고 해당 클라이언트 재시작을 검토한다.
CLI·IDE·앱이 다른 Windows/WSL 호스트를 사용하면 각각의 Codex home이 다를 수 있다.
기존 대화의 모델·컨텍스트 설정이 소급 변경됐다고 가정하지 않는다.

## 확인과 복구

- `codex --version`, `codex login status`: 버전과 로그인 방식을 확인한다.
- `codex features list`: `context_management` 활성 설정을 확인한다. 이는 파싱/설정 확인이며
  계정 eligibility나 실제 history search/new-context 실행의 성공 증거는 아니다.
- 새 작업의 `/status`와 클라이언트가 제공하는 context/기능 정보로 실제 적용을 확인한다.
  모델/CLI override, 프로젝트 설정, 다른 호스트, 관리형 requirements가 영향을 줄 수 있다.
- 공식 모델 안내는 Plus/Pro, 설정 레퍼런스는 Plus/Pro/Pro Lite를 명시한다.
  Business/Enterprise/API 키 로그인은 출시 시점 대상에서 제외된다. 최신 지원 조건을 확인한다.
- 문제가 생기면 `experimental_mode = false`로 설정하고 새 작업에서 비교한다.
  기본 context로 복원하려면 두 context override 키를 제거한다. 전체 config 복구는
  이후 다른 변경이 없는지 확인한 뒤 개인 백업과 비교해 필요한 키만 복구한다.
- 개인 백업은 Codex home의 `backups/devbox-agent-setup-<timestamp>/`에 보관한다.
  인증이나 전체 개인 config를 저장소·PR에 복사하지 않는다.

## 지침·스킬·MCP

- [AGENTS](../AGENTS.md)는 필수 제약과 탐색 경로,
  [CONVENTIONS §11](../CONVENTIONS.md#11-codex-지침스킬작업-기록)은 작업 도구 운영의 원장이다.
- `.agents/skills/devbox-change`와 `devbox-migration-review`는 해당 작업에 자동 선택 가능하다.
  `devbox-release`는 `$devbox-release`로 명시 호출한다. 새 세션의 skill 목록에서 확인한다.
- 개인 `workthrough` 스킬은 PR 묶음당 짧은 기록 하나를 생성·갱신하도록 정리한다.
  저장소에서도 동일한 기록 규칙을 유지하므로 해당 개인 스킬이 없어도 협업할 수 있다.
- OpenAI Docs MCP와 GitHub 연결/`gh`, 로컬 셸을 우선 사용한다. 추가 MCP는 실제 기능 부족이
  있을 때 검토한다. 플러그인 cache를 직접 편집하거나 무관한 스킬을 일괄 삭제하지 않는다.
- 개인 모델·context 설정은 이 문서의 예시로만 공유한다. 프로젝트 `.codex/config.toml`에
  고정해 동료의 계정/프로필을 덮어쓰지 않는다. CLI 프로필이 필요하면 최신 방식인
  `~/.codex/<name>.config.toml`과 `codex --profile <name>`을 사용한다.
- `features.memories`는 다른 작업으로 정보를 이어가는 별도 기능이다. 이번 설정에서 추가로
  켜지 않는다. 필수 규칙과 완료 증거는 AGENTS·명세·테스트·CI 원장에 유지한다.

## 공식 근거

- [Config reference](https://developers.openai.com/codex/config-reference): context 키와 실험 플래그.
- [Config basics](https://learn.chatgpt.com/docs/config-file/config-basic): 위치·우선순위·호스트 설정.
- [Profiles](https://learn.chatgpt.com/docs/config-file/config-advanced#profiles): 별도 프로필 파일.
- [Models](https://learn.chatgpt.com/docs/models#experimental-context-management): 실험 대상과 새 작업 시작.
- [Astra model](https://developers.openai.com/api/docs/models/gpt-6-astra): API 창과 장문 입력 요금 조건.
- [AGENTS discovery](https://developers.openai.com/codex/guides/agents-md): 계층·시작 경로·32KiB 기본 한도.
- [Skills](https://developers.openai.com/codex/skills): 점진적 로드·저장소 탐색·명시 호출 정책.
- [MCP](https://developers.openai.com/codex/mcp): 서버 구성·도구 필터.
- [Memories](https://learn.chatgpt.com/docs/customization/memories): 작업 간 참고 계층.
