# Code Pad managed selection test race

## Overview

PR #432의 workspace frontend gate가 Code Pad의 managed LSP persistence fixture에서 간헐적으로
실패했다. fixture가 서버 종류 select에 change event를 보낸 직후 같은 event turn에 적용 버튼을
눌러, React가 managed form과 기본 manifest selection을 commit하기 전에 이전 local callback을
실행할 수 있었다. 실제 사용자는 화면이 managed form으로 바뀐 뒤 버튼을 누르므로 제품 동작이
아닌 test synchronization 결함이다.

## Change

`apps/code-pad/src/components/LspControlPanel.test.tsx`의
`persists only the managed id and version, never an install path` fixture가 다음 두 observable UI
조건을 기다린 뒤 적용 버튼을 누르도록 변경했다.

- `관리형 서버 버전` select가 렌더링된다.
- select value가 catalog의 exact `manifest_id`와 `version` 조합으로 설정된다.

내부 timer나 임의 sleep은 사용하지 않는다. production component, persistence schema, managed
installer 동작은 변경하지 않는다.

## Verification

- `pnpm --filter code-pad exec vitest run src/components/LspControlPanel.test.tsx -t
  'persists only the managed id and version' --maxWorkers=1`을 5회 연속 실행했고 모두 통과했다.
- `pnpm --filter code-pad test -- --maxWorkers=2`: 14 files, 118 tests passed.
- `pnpm --filter code-pad build`: TypeScript와 2,171-module Vite production build passed.
- `git diff --check`로 test와 workthrough만 변경됐는지 확인한다.
- GitHub Linux frontend와 Windows/Rust workspace gates가 모두 green인 경우에만 merge한다.
