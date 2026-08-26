# Code Pad Quick Open Workthrough

- Date: 2026-08-26
- Issue: #277 `feat(code-pad): Quick Open`
- Branch: `feat/code-pad/quick-open`
- Base: `40fc105d9b6e636c4f61a7bdfcf0c3550cd20334`
- Target: Code Pad 0.4.0 / v0.5.0 P1-09-14
- Status: implementation, direct review and local PR-wide gates complete; GitHub Actions pending

## Outcome

Code Pad Quick Open은 기존 bounded workspace snapshot을 파일명·상대 경로로 fuzzy 검색한 뒤
재귀 directory tree로 표시한다. 긴 경로를 한 줄 ellipsis로 버리지 않고 파일명과 전체 부모 경로로
분리해 줄바꿈하며, root file과 nested directory를 명확한 group으로 제공한다.

검색 input이 keyboard control surface를 소유한다. `Ctrl/⌘+P`로 열고 `↑/↓`·`Home/End`로 tree의
실제 표시 순서를 이동한 뒤 `Enter`로 선택하며 `Esc`로 닫는다. modal이 열린 동안 Tab은 배경 UI로
빠져나가지 않고 input에 남으며, 닫힌 뒤에는 열기 전 focus가 복원된다.

새 filesystem command, walk, Git grep, LSP 요청, network, process 실행 또는 persistence는 추가하지
않았다. fuzzy score와 tree는 기존 `list_workspace_files` 결과를 frontend memory에서 파생하고,
파일을 열 때는 backend가 반환한 canonical absolute path를 기존 open 흐름에 그대로 전달한다.

## Existing Boundary

기존 Code Pad에는 다음 기반이 이미 있었다.

- workspace 선택 시 backend가 canonical root 아래 파일을 제한적으로 열거
- 최대 50,000개/10초/깊이 32의 workspace snapshot
- absolute `path`, workspace-relative `relativePath`, byte `size` DTO
- substring/subsequence 기반 local fuzzy score
- frontend 결과 최대 200개 표시
- toolbar와 global `Ctrl/⌘+P` 진입점

그러나 결과는 평면 목록이었고 Quick Open 전용 CSS가 없어 dialog, overflow, focus state가 정의되지
않았다. 긴 상대 경로는 파일명과 directory context를 구분하지 않았으며, 작업 session에서 복원된
workspace는 한 번만 등록된 global keyboard listener가 초기 null state를 capture해 목록 refresh를
건너뛸 수 있었다.

## Scope

### Included

- 기존 workspace snapshot의 deterministic fuzzy ranking
- matched result만 포함하는 재귀 directory grouping
- root file과 directory group 분리
- 파일명과 전체 parent path의 별도 wrap 가능한 표시
- directory best-match score 기반 deterministic group order
- 화면 tree preorder와 keyboard selection 순서 일치
- `Ctrl/⌘+P`, `↑/↓` wrap, `Home`, `End`, `Enter`, `Esc`
- search input focus, modal Tab containment, close 뒤 focus return
- combobox/listbox/group/option semantic과 `aria-activedescendant`
- 현재 workspace path와 match count 표시
- workspace 부재, loading, no-result, truncated snapshot, 200-result limit 상태
- restored workspace의 latest Quick Open handler 사용

### Excluded

- LSP server, status, log, retry 또는 cache UX
- Git grep, text-content search 또는 symbol search
- 두 번째 filesystem walk 또는 background watcher 변경
- preview styling과 editor/preview active state
- tab/panel pinning 또는 multi-pane navigation 변경
- workspace snapshot backend contract와 상한 변경
- storage/session schema 변경
- network, download, sidecar 또는 외부 search tool
- unsafe path를 직접 입력하거나 결과 DTO를 재구성하는 흐름

## Design Decisions

### 1. Reuse the bounded snapshot

Quick Open은 `WorkspaceFile[]`만 입력으로 받는다. query 변경은 이 배열을 filter/sort/group할 뿐
Tauri command를 다시 호출하지 않는다. 따라서 typing latency가 filesystem과 분리되고 검색 한 번마다
workspace를 걷지 않는다.

snapshot 자체는 기존 backend가 canonical workspace root, depth/file/time bound를 적용해 만든다.
frontend는 표시용 relative path와 기존 open용 absolute path의 책임을 섞지 않는다.

### 2. Keep matching deterministic

query와 candidate는 NFKC normalization과 locale-independent lowercasing을 거친다. substring은 fuzzy
subsequence보다 우선하고, segment boundary, contiguous run, gap, 시작 위치와 전체 길이로 score를
정한다. 동점은 locale 환경에 좌우되지 않는 code-unit ordering으로 해결한다.

빈 query도 동일한 comparator를 사용하므로 fixture와 Windows WebView2에서 순서가 재현된다.

### 3. Group only matched files

`groupQuickOpenMatches`는 filter 뒤 최대 200개 결과만 tree에 삽입한다. 일치하지 않은 sibling이나
빈 directory는 나타나지 않는다. directory node는 normalized relative path, 마지막 segment, child
directory와 direct file만 가진 presentation DTO다.

각 directory는 자신과 descendant 중 최고 score로 정렬하며 direct file은 fuzzy score로 정렬한다.
filesystem path를 생성하거나 join하지 않으므로 presentation path가 open target으로 승격되지 않는다.

### 4. Derive keyboard order from rendered order

초기 draft는 global fuzzy result index로 selection을 계산하면서 root/directory group을 별도 순서로
렌더링했다. 이 경우 아래 화살표가 화면상 다음 행이 아닌 다른 group으로 이동할 수 있었다.

수정된 흐름은 grouped tree를 preorder로 flatten하고 다음 모든 동작에 같은 array를 사용한다.

```text
grouped tree
  -> flattenQuickOpenTree
  -> selected index
  -> aria-activedescendant
  -> scrollIntoView
  -> Enter absolute path
```

pointer hover도 같은 index를 선택하므로 keyboard와 pointer 사이에 별도 state가 없다.

### 5. Show long paths without changing their identity

relative path는 display 단계에서 filename과 parent directory로만 분해한다. 두 text region은
`overflow-wrap: anywhere`와 `word-break: break-word`를 사용하며 exact relative path는 button의 title과
accessible label에 남는다.

directory depth는 inline `depth * padding`으로 계산하지 않는다. nested section마다 13px margin을
한 번씩 누적해 깊은 path가 과도하게 밀리는 문제를 피한다.

### 6. Keep the global shortcut current

global shortcut effect는 listener churn을 피하기 위해 한 번 등록된다. 대신 render마다 최신
workspace-aware `handleQuickOpen`을 ref에 저장하고 listener는 ref를 호출한다. session hydrate 후
workspace가 복원됐지만 listing root가 아직 다르면 기존 `loadWorkspaceSnapshot`을 실행한다.

toolbar button과 shortcut은 같은 handler를 공유하므로 snapshot refresh와 오류 처리가 갈라지지 않는다.

### 7. Use one keyboard surface

search input은 `role=combobox`, popup은 `role=listbox`, directory는 `role=group`, file은 `role=option`을
사용한다. option button은 programmatic selection target이지만 tab stop은 아니다. input이 방향키와
Home/End를 처리하고 selected option ID를 `aria-activedescendant`로 알린다.

dialog에는 input 외의 interactive control을 추가하지 않았다. Tab/Shift+Tab은 input에 남고 modal
뒤의 editor로 focus가 빠져나가지 않는다. unmount cleanup은 이전 active element가 아직 document에
연결돼 있을 때만 focus를 돌려준다.

## Data Flow

```text
workspace select/session restore
  -> existing list_workspace_files(canonical root)
  -> bounded WorkspaceFile[] in React memory
  -> Ctrl/Command+P or toolbar
  -> optional stale-root snapshot refresh
  -> filterQuickOpenFiles(query)
  -> first 200 results
  -> groupQuickOpenMatches
  -> flattenQuickOpenTree(render order)
  -> keyboard/pointer selection
  -> existing onOpen(canonical absolute path)
  -> existing openPath/openFile flow
```

## Failure and Empty States

| State | UI behavior | External effect |
|---|---|---|
| workspace not selected | selection rows omitted, setup guidance shown | none |
| snapshot loading | loading message and count state | existing single snapshot read |
| no match | empty-result guidance | none |
| backend snapshot truncated | bounded-index banner remains visible | none |
| more than 200 matches | top-200 banner remains visible | none |
| restored workspace listing absent | latest handler refreshes exact restored root | existing read-only listing |
| selected item closes dialog | prior focus is restored if still connected | existing file open only |

## Security and Privacy Review

| Risk | Control |
|---|---|
| relative display path becomes an arbitrary open target | `onOpen` receives original backend `file.path`, never recomposed display segments |
| search causes unbounded filesystem work | query operates on existing bounded snapshot only |
| raw path injected as markup | React text nodes and DOM attributes only; no raw HTML |
| deep path forces horizontal overflow | filename/parent/directory values wrap anywhere |
| locale changes ordering | NFKC + `toLowerCase` + deterministic comparator |
| visual and keyboard order select different file | selection array is flattened directly from rendered tree |
| modal leaks focus to editor | input owns Tab and cleanup restores prior connected element |
| stale global listener opens an empty snapshot | listener invokes latest handler through ref |
| secret/credential persistence | no new storage, history, log or clipboard flow |
| unintended external action | no process, network, LSP, Git or download operation added |

Workspace paths remain visible because locating a user-selected local file is the feature's purpose. They are not
written to logs or a new store, and the existing session/workspace persistence boundary is unchanged.

## Tests

### Pure matching and grouping

- substring ranking before fuzzy subsequence
- filename segment boundary bonus
- non-subsequence rejection
- root files separated from nested directories
- nested directory creation and deterministic grouping
- visual tree flatten order
- Windows separator normalization for display splitting
- long filename and complete parent context preservation

### Component keyboard and semantics

- workspace absence omits stale options
- nested groups and complete path title
- initial combobox focus
- `aria-activedescendant` selected option
- Enter opens the backend absolute path, not display title
- Home/End movement
- Tab stays in the modal input
- Escape calls close
- truncated snapshot banner
- unmount restores the previous focus target

### App integration

- hydrated session restores workspace root
- Ctrl+P calls `listWorkspaceFiles` with the restored root
- dialog shows the returned relative path
- existing app-link, editor, session, file operation and LSP fixtures remain green

Focused validation from an exact Linux-native source mirror:

```text
pnpm --filter code-pad exec vitest run \
  src/lib/quickOpen.test.ts \
  src/components/QuickOpen.test.tsx \
  src/App.test.tsx --maxWorkers=2

Test Files  3 passed (3)
Tests       34 passed (34)

pnpm --filter code-pad test -- --maxWorkers=2
Test Files  14 passed (14)
Tests       108 passed (108)

pnpm --filter code-pad build
passed
```

The first focused run exposed one fixture that compared the display title with the open target. The implementation
correctly returned the backend absolute path; the fixture was corrected to assert that security-relevant contract.

## Documentation

- app README describes bounded fuzzy tree search, long-path display and full keyboard flow
- architecture records Quick Open as a bounded frontend tree over the workspace snapshot
- UX plan replaces the stale “no Quick Open CSS” assessment with the implemented semantics and fixtures
- native-first plan records ranking, tree order, limits, focus, external-effect boundary and W1 evidence
- roadmap advances the next P1-09 feature to #278 LSP management UX

## Build

All 17 frontend application/package workspaces build successfully. Code Pad production assets are:

| Asset | Size | gzip |
|---|---:|---:|
| CSS | 19.78 kB | 4.85 kB |
| primary JS | 1,671.88 kB | 506.92 kB |

The existing Vite warning for Code Pad chunks over 500 kB remains. This feature adds no package, Rust dependency,
capability, sidecar, download or storage schema.

## Files

- `apps/code-pad/src/lib/quickOpen.ts`
- `apps/code-pad/src/lib/quickOpen.test.ts`
- `apps/code-pad/src/components/QuickOpen.tsx`
- `apps/code-pad/src/components/QuickOpen.test.tsx`
- `apps/code-pad/src/App.tsx`
- `apps/code-pad/src/App.test.tsx`
- `apps/code-pad/src/App.css`
- `apps/code-pad/README.md`
- `docs/architecture.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-15-ux-improvements-design.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
- `workthrough/2026-08-26-code-pad-quick-open.md`

## PR-wide Gates

- focused Quick Open/App frontend tests: passed, 34 tests
- all Code Pad frontend tests: passed, 108 tests
- Code Pad production build: passed
- `git diff --check`: passed
- `cargo fmt --all -- --check`: passed
- `cargo clippy -p code-pad --all-targets --jobs 1 -- -D warnings`: passed
- `cargo test --workspace --jobs 1`: passed
- `cargo check --workspace --jobs 1`: passed
- all 17 frontend/package workspace builds: passed
- `pnpm audit --audit-level moderate`: no known vulnerabilities
- dependency notices and dependency policy regression tests: passed
- build-manifest notice tests: passed
- catalog consistency: passed
- `cargo deny --locked check`: advisories, bans, licenses and sources passed; configured duplicate warnings only
- GitHub Actions Linux, Windows, frontend, dependency and catalog gates: pending

## W1 Packaged Checkpoint

- restored workspace에서 `Ctrl+P`가 exact root snapshot을 읽는다
- Windows separator와 깊은 relative path의 filename/parent가 잘리지 않고 wrap된다
- root file과 nested directory group이 화면에 구분된다
- `↑/↓`·`Home/End` selected row와 Enter로 열린 파일이 일치한다
- Tab이 dialog 뒤 editor로 빠지지 않고 Esc/선택 후 이전 focus가 복원된다
- 50,000-file snapshot과 200-result UI 상한 안내가 유지된다
- query 입력이 추가 filesystem walk, LSP, Git 또는 process/network action을 발생시키지 않는다
- 표시 path가 open target으로 재조합되지 않고 backend absolute path만 열린다

## Next

#278 Code Pad LSP management UX is the next P1-09 feature. It remains a separate PR because status/log redaction,
retry/backoff races, managed runtime cache state and single-scroll panel layout have a materially different native
process and privacy boundary. #279 editor/preview distinction also remains separate.
