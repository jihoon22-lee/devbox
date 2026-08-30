# Everything+ WSL root and bounded reconciliation

## Overview

Everything+가 Windows native 경로와 WSL UNC 경로를 같은 검색 루트 lifecycle로
다루도록 보강했다. `\\wsl$`, `\\wsl.localhost`, Windows canonicalization이 반환할 수 있는
`\\?\\UNC\\wsl.localhost` 표기를 안전하게 식별하고, WSL 루트에는 Windows 재귀 notify 대신
bounded metadata polling을 사용한다. 스캔·watcher·reconciliation의 모든 큐와 snapshot에는
상한을 두며, 연결 끊김·불완전한 순회·상한 초과가 기존의 완전한 인덱스를 빈 결과로 덮어쓰지
않도록 했다.

이 workthrough의 구현 범위는 issue #490의 Everything+ 부분이며, 앱 버전은 `0.4.0`에서
`0.5.0`으로 올렸다. Windows에서 실제 WSL 배포판을 연결하고 패키지 앱으로 확인하는 검증은
수행하지 않았으며, 해당 evidence는 통합 acceptance issue #493의 범위로 남긴다.

## Context and root cause

프로젝트가 `/mnt/e/projects`에서 `/home/jihoon/projects`로 이동하면서 Windows 앱에서
다음과 같은 WSL UNC 루트를 직접 등록해야 했다.

```text
\\wsl$\Ubuntu\home\jihoon\projects\devbox
\\wsl.localhost\Ubuntu\home\jihoon\projects\devbox
```

기존 Everything+ root validation과 watcher는 일반 Windows 경로를 전제로 했다.
WSL UNC를 native 경로처럼 canonicalize하거나 Windows식 대소문자 무시 prefix로 비교하면
다음 문제가 생긴다.

- `wsl$`와 `wsl.localhost` 또는 distro 이름의 표기만 다른 경로가 서로 다른 루트가 된다.
- Linux tail의 `DevBox`와 `devbox`가 잘못 합쳐진다.
- Windows 재귀 notify가 WSL UNC provider에서 안정적인 변경 감지를 보장하지 않는다.
- distro가 offline이거나 하위 entry 하나를 읽지 못했을 때 빈 snapshot으로 판단해 기존 rows를
  삭제할 수 있다.
- notify callback/channel과 debounce map이 이벤트 폭주에 따라 무제한으로 커질 수 있다.
- parent root의 reconcile이 nested root의 `content` 정책을 무시하면 content-disabled 영역에
  content rows를 남기거나, 반대로 child의 content-enabled 설정을 잃는다.

따라서 WSL path identity와 containment만 공용 규칙으로 추출하고, 각 앱의 watcher lifetime과
mutation model은 분리했다. 이 PR에서는 shell, distro 내부 long-running process, 네트워크
fetch를 추가하지 않는다. 파일 bytes는 기존의 local Windows/WSL filesystem provider를 통해서만
접근한다.

## Changes made

### 1. Shared WSL identity and filesystem walk authority

- `crates/wsl/src/path.rs`
  - 사용자 입력의 `\\wsl$\<distro>\...`, `\\wsl.localhost\<distro>\...`와
    `\\?\UNC\wsl.localhost\...`를 모두 파싱한다.
  - transport alias와 distro 이름은 ASCII case-insensitive로 비교하고, Linux path tail은
    Unicode와 대소문자를 그대로 보존한다. 공백·한글 component도 일반 경로 데이터로 유지한다.
  - 빈 tail, traversal, control character, 허용하지 않는 distro 이름, oversize 입력은 거부한다.
  - `wsl_unc_contains(root, candidate)`를 추가해 distro/transport는 접고 Linux path는
    component boundary 기준으로만 descendant를 판정한다. 일반 native root와 WSL candidate를
    혼동하지 않는다.

  핵심 identity 규칙은 다음과 같다.

  ```rust
  // transport/distro spelling is folded; Linux tail is not.
  normalize_path(r"\\wsl.localhost\Ubuntu\home\jihoon\DevBox")
      == "//wsl$/ubuntu/home/jihoon/DevBox";

  wsl_unc_contains(
      r"\\wsl$\Ubuntu\home\jihoon\DevBox",
      r"\\wsl.localhost\ubuntu\home\jihoon\DevBox\src\main.rs",
  ) == Ok(Some(true));
  // The same distro's `devbox` is a different Linux path.
  ```

- `crates/filesystem/src/walk.rs`
  - `WalkResult`에 `incomplete`를 추가했다. directory entry나 metadata를 읽지 못한 사실을
    기존처럼 조용히 버리지 않고 snapshot의 authority를 낮춘다.
  - `collect_limited`는 기존 file-count `truncated`와 새 `incomplete`를 함께 반환한다.
    누락된 파일이 없다는 것을 증명할 수 없을 때 consumer가 deletion을 추정하지 않도록 하는
    공용 계약이다.

- `apps/everything-plus/src-tauri/src/core/db.rs`
  - WSL root를 저장할 때 transport alias와 distro spelling을 canonical `//wsl$/<lower-distro>`
    형태로 통일하고 Linux tail의 case는 보존한다.
  - 저장된 root와 이벤트/file의 ownership lookup이 alias를 하나의 root로 인식하며,
    `DevBox`와 `devbox`는 별도 root/file로 유지된다.

### 2. Safe bounded indexing and last-known-good preservation

- `apps/everything-plus/src-tauri/src/commands/indexing.rs`
  - `MAX_ROOT_SCAN_FILES = 250_000`의 bounded snapshot을 사용한다. root를 걷기 전에
    ordinary directory인지, symlink/reparse component가 없는지 확인하고, 스캔 결과가 complete한
    경우에만 derived rows를 clear한다.
  - 불완전한 snapshot에서도 확인된 additions/updates는 반영할 수 있지만, snapshot에 없는
    파일을 deletion으로 해석하지 않는다. 전체 재인덱스의 format marker도 모든 root scan이
    complete할 때만 기록한다.
  - root 설정 또는 `content` 정책이 bounded read/DB commit 중 바뀌면 stale snapshot을
    commit하지 않고 queued restart에 맡긴다. full scan 중 add/remove/toggle이 유실되지 않도록
    lifecycle lock, cooperative cancel, one-shot restart를 사용한다.
  - nested roots의 가장 깊은 matching root가 content policy를 소유하도록 유지했다. 예를 들어
    parent가 `content=false`, child가 `content=true`인 경우 parent reconcile도 child 문서를
    content-index한다. nested child를 제거하면 남은 ancestor 정책으로 다시 수렴하도록 targeted
    reindex를 요청한다.

  안전한 clear 경계는 다음 형태다.

  ```rust
  let walked = collect_root_files(Path::new(&root.path))?;
  if walked.truncated {
      last_error = Some("root_scan_limit");
  } else if walked.incomplete {
      last_error = Some("root_scan_incomplete");
  } else {
      clear_root(&conn, &root.path)?; // complete snapshot only
  }
  ```

- `apps/everything-plus/src-tauri/src/core/watcher.rs`
  - WSL-aware containment을 순수 logic에 연결했다. native Windows 경로의 case rule은
    유지하면서 WSL Linux tail에는 Windows-wide lowercase를 적용하지 않는다.
  - debounce map에 pending path 4,096개, ready batch 512개, path 32 KiB 상한을 추가했다.
    상한을 초과한 이벤트는 버리지 않고 owning root reconciliation으로 전환한다.

- `apps/everything-plus/src-tauri/src/commands/watcher.rs`
  - native root는 `notify` recursive watcher를 유지하고, WSL root는 5초 주기의 bounded
    metadata polling을 사용한다. polling은 의도적인 capability이며 native realtime으로
    가장하지 않는다.
  - callback channel은 bounded sync channel 1,024개다. 단일 notify event에서 받는 path는
    256개, path byte는 32 KiB로 제한한다. callback error, channel overflow, debounce overflow,
    polling diff overflow는 root 하나를 bounded set에 coalesce해 full reconciliation을
    요청한다.
  - polling snapshot도 `MAX_ROOT_SCAN_FILES`를 공유한다. complete snapshot에서만 deletion
    diff를 만들고, truncated/incomplete snapshot에서는 additions/updates만 반영하며 이전
    snapshot을 보존한다.
  - full index가 실행 중 ready event가 되면 debounce queue를 busy-loop로 재arm하지 않고 owning
    root restart 하나로 합친다. persisted root는 앱 재시작 시 초기 reconcile을 요청한다.
    WSL distro가 offline인 persisted root도 설정 목록이나 polling set에서 제거하지 않고
    `polling` mode와 `root_unavailable` error로 남겨, distro가 돌아온 다음 자동으로
    polling/reconcile할 수 있다.

### 3. Stable root status and UI capability disclosure

- `apps/everything-plus/src-tauri/src/core/models.rs`
  - `RootSourceKind` (`native`/`wsl`)와 `WatchMode` (`native`/`polling`/`unavailable`)를
    추가했다.
  - `RootStatus`는 frontend contract에 맞춰 camelCase로 serialize되며 source, watch mode,
    last sync, pending 수, stable error code를 함께 반환한다.

- `apps/everything-plus/src/types.ts`
  - 위 root status capability를 TypeScript union으로 고정했다.

- `apps/everything-plus/src/api.ts`
  - browser mock도 native source/watch mode를 포함하도록 갱신해 offline frontend test가
    실제 payload shape과 같은 경로를 사용하게 했다.

- `apps/everything-plus/src/App.tsx`
  - root chip에 `WSL`과 `WSL 주기 확인`을 표시하고, `연결 끊김`, `범위 상한`, `부분 스캔`,
    pending 상태를 구분한다.
  - tooltip은 raw path나 OS error를 노출하지 않고 capability와 보존 동작을 설명한다.
    WSL UNC 예시도 Settings placeholder에 제공한다.

- `apps/everything-plus/src/App.test.tsx`
  - WSL polling status와 unavailable status가 각각 올바른 label/title을 표시하는 regression
    test를 추가했다.

### 4. Documentation, release metadata, and contract

- `apps/everything-plus/README.md`
  - WSL aliases, Linux case sensitivity, polling capability, stable errors, bounded queue,
    last-known-good preservation, reconnect behavior를 사용자 계약으로 문서화했다.
- `docs/superpowers/specs/2026-08-31-wsl-file-workflows.md`
  - issue #490의 WSL identity, Everything+ lifecycle, bounded watcher, nested policy,
    downstream Code Pad/Knowledge Base capability boundary와 #493 acceptance boundary를
    상세 설계로 고정했다.
- `apps/everything-plus/package.json`
- `apps/everything-plus/src-tauri/Cargo.toml`
- `apps/everything-plus/src-tauri/tauri.conf.json`
  - Everything+ 버전을 `0.5.0`으로 동기화하고 `devbox-wsl` workspace dependency를 추가했다.
- `.github/scripts/windows-packaged-smoke-config.json`
  - packaged smoke 대상 Everything+ version을 `0.5.0`으로 맞췄다.
- `Cargo.lock`
  - 새 local workspace dependency edge를 반영했다.
- `THIRD_PARTY_NOTICES.md`
  - lockfile provenance를 재생성했다. 새 외부 dependency를 추가한 변경은 아니다.

## Stable error contract

Everything+는 root path나 OS/SQLite 원문을 UI/API 오류에 반향하지 않는다. root 상태에는 다음
stable code만 사용한다.

| Code | 의미 | 기존 인덱스 처리 |
|---|---|---|
| `root_unavailable` | root가 사라졌거나 directory가 아니거나 link/reparse boundary를 통과했거나, WSL distro/provider가 offline | last-known-good rows 유지 |
| `root_scan_limit` | 한 snapshot이 250,000 regular-file bound를 초과 | 기존 rows 삭제 금지 |
| `root_scan_incomplete` | directory entry/metadata 일부를 읽지 못함 | 누락을 deletion으로 추정하지 않음 |
| `incremental_index_failed` | bounded incremental mutation 반영 실패 | root 상태에 실패를 표시하고 다음 reconcile 가능 |
| `indexing_failed` | worker 시작/전체 indexing 경계 실패 | global status에만 축약 상태 표시 |

성공적으로 complete한 snapshot을 다시 얻으면 polling root의 error를 지우고 one-shot
reconciliation을 요청해 stale rows와 global state를 수렴한다. 즉, offline 동안에는 기존
검색 결과를 보존하고 reconnect 후에만 삭제를 포함한 새 snapshot을 authority로 채택한다.

## Bounded behavior

| 영역 | 상한/주기 | 초과 시 동작 |
|---|---:|---|
| root scan | 250,000 files | `root_scan_limit`, deletion 보류 |
| SQLite write batch | 250 records | 작은 transaction으로 commit |
| notify channel | 1,024 messages | owning-root reconcile |
| one-event paths | 256 paths | callback payload 잘라내고 reconcile |
| path/debounce key | 32 KiB | 해당 root reconcile |
| pending debounce | 4,096 paths | 해당 root reconcile |
| ready batch | 512 paths | 남은 pending은 다음 batch |
| WSL poll | 5 seconds | bounded metadata diff |
| reconcile set | root당 1개 | duplicate request coalesce |

이 경계는 이벤트 폭주나 대형 WSL tree가 UI와 인덱서의 메모리를 무제한으로 늘리지 않게
하면서도, 초과된 변경을 조용히 버리지 않고 root 전체의 재수렴으로 전환한다.

## Verification results

전용 Linux-native Cargo target과 낮은 병렬도에서 다음 로컬 검증을 완료했다.

```text
cargo test -p wsl -p filesystem -p everything-plus -j1
  PASS — everything-plus 133, filesystem 19, wsl 36 tests

cargo check -p wsl -p filesystem -p everything-plus -j1
  PASS

cargo clippy -p wsl -p filesystem -p everything-plus --all-targets -j1 -- -D warnings
  PASS

cargo fmt --all -- --check
  PASS

git diff --check
  PASS

pnpm --filter everything-plus test
  PASS — 25 frontend tests

pnpm --filter everything-plus build
  PASS — initial bundle 233.54 kB, gzip 72.78 kB

python3 .github/scripts/check-dependencies.py check
  PASS

bash .github/scripts/check-catalog.sh
  PASS — catalog/version/smoke configuration consistency
```

주요 regression은 다음을 포함한다.

- WSL transport/distro alias 수렴과 extended UNC parsing
- 같은 distro에서 Linux tail case-sensitive identity
- spaces·한글 경로 round-trip 및 component-boundary containment
- missing root를 빈 authoritative snapshot으로 취급하지 않음
- offline 동안 기존 index 유지 후 complete reconnect snapshot에서 수렴
- parent reconcile이 nested content-enabled root 정책을 보존
- polling diff에서 incomplete snapshot의 deletion 보류
- callback, channel, debounce, polling diff의 bounded behavior
- frontend의 WSL polling/unavailable status disclosure

이 검증은 WSL parser, filesystem walker, Everything+ Rust logic와 browser frontend에 대한
자동화 evidence다. Windows compile CI, Windows packaged installer, 실제 WSL Desktop/Ubuntu
distro에서의 Everything+ 실행·reconnect·파일 변경 acceptance는 실행하지 않았으며, #493의
통합 Windows gate에서 별도로 확인한다.

## Follow-up boundary

- #493에서 Windows + WSL physical acceptance를 수행한다.
- Code Pad와 Knowledge Base는 spec에 기록한 동일 WSL identity/capability 원칙을 각각의
  기능 경계 PR에서 구현한다.
- WSL 내부에서 LSP를 실행하는 protocol은 이번 변경에서 암묵적으로 추가하지 않는다.
