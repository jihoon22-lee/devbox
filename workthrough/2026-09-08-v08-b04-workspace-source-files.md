# B04 Workspace Source and Files

WP #546 / R02–06, R08, R13, R16–21, R24–25 and S01. Draft PR #557 owns
Registry, Overview/Source/Files/Dependencies, legacy import and WSL-native LSP.
Runtime and Terminal/session owners remain B05/B06. B01/B02/B03 are complete (3/9);
this packet does not close #546, #541 or #542.

## Result and implementation boundary

The existing Workbench, Repo Manager and Code Pad UI/tests share
`packages/workspace-features`, with lazy product routes, retained drafts and scoped
CSS. Legacy entry points consume the same components. Native component builds
omit standalone bootstrap/web assets and use immutable generation-specific stores,
common snapshots/handoffs/caches. Opening the product does not migrate an old
identifier, activate legacy data or start LSP. v0.7's public topology remains intact.
The architecture contract is [v0.8 Workspace](../docs/architecture/v0.8-workspace.md).

- **Registry and definitions:** opaque Project/Worktree/Repo IDs, root/common-Git
  objects, aliases/rebind and target/distro binding are distinct from trust.
  One native writer owns the product directory/lock. Registration uses expiring
  one-time native previews and rechecks linked-worktree pointers/backlinks, bytes
  and physical identities. Pointer transport checks precede IO. Activation cannot
  replace an existing pointer; corrupt/future metadata and bounded prepared
  generations remain preserved. Project/local definition review shows before/after
  content, checks revisions and explicit removal/precedence, and revokes execution
  approval on changes. Export includes shared manifest fields only.
- **Source and Dependencies:** retained Git panels support status/diff/history,
  selected stage/commit, fetch/ff-only pull/configured-upstream push, worktree
  creation and explicit root/sibling cleanup. Diff lines open Files. Native review
  captures commands, config/includes/hooks, definitions and environment; actual Git
  children use that captured environment, including after a native chooser.
  Cleanup scope separately approves up to eight registered siblings and rejects
  unknown Git-returned paths before IO. Open editor grants block removal by
  physical ancestor identity. Cleanup preserves branches and Registry records.
  Dependency Lens keeps local graph/duplicate/stale-lock diagnostics and separately
  reviewed, bounded remote opt-in/redaction. Import/open does not trigger network,
  hooks, package installation or distro startup.
- **Files:** native project or OS-picker consent admits at most 64 documents/choices.
  Save/rename/delete check document revision, object, content and ancestor evidence;
  DOS aliases, symlinks/reparse points and foreign transports are checked before IO.
  CodeMirror, single-file mode, encoding/CRLF, bookmarks, two views, preview and
  recovery remain shared. Dirty drafts survive route switches and block context
  changes. Recovery binds context, entry bytes and file snapshot to one-use native
  tokens; raw legacy recovery mutation is unavailable in product mode.
- **Windows LSP:** installation/cache/archive import runs through a separate bounded
  queue. Expiring picker tokens grant no file/editor access; private pinned copies
  undergo catalog/digest/Node-lock verification. Context-local configuration uses
  native revisions and preserves corrupt/future evidence. Execution review binds
  executable/runtime/package/environment/definitions, including probes and retries.
  Owned Windows Jobs retain starting/retrying/cancelled children until confirmed
  retirement, including product exit. Native document authority mirrors every open
  buffer and guards WorkspaceEdit preview/atomic apply/rollback. Explicit journal
  recovery works with disabled/missing/corrupt settings, preflights all targets and
  backups, preserves partial results, and deletes only unchanged owned backups.
- **Concurrency:** context/filesystem permits span admission, queues and worker
  retirement after caller cancellation. Files and Git coordinate disk operations.
  Installation retains its own operation lock. Initialization now publishes the
  host once using `OnceLock`, eliminating a contended read mutex that could return
  `busy`; worker/admission/context limits remain unchanged. Windows Source history
  previously showed a busy failure, but confirmation of this correction is pending.

## Data conversion and recovery

Fixed native sibling identifiers provide the Workbench profile/template and Code
Pad session/recovery/LSP inventory. Repo Manager's scan/selection is transient
React state; no preference file is invented. A cancellable bounded job pins source
objects, bytes and full inventory, preserves exact data without clobbering, and
publishes the manifest last. This detects observed changes, not an app-wide JSON
transaction; no SQLite copy occurs here. Corrupt/future files and incomplete jobs
remain preserved. Restart catalog entries require full byte revalidation; verified
snapshots remain usable after source loss. Snapshot preservation alone is not conversion.

Workbench profile import preserves original IDs and complete metadata alongside
new Workspace IDs. New/reuse/keep-both/skip decisions commit in one Registry CAS.
Separate Windows folder review commits registration and profile binding together;
unlink preserves records. Imported ports are defaults under project/local values;
environment/service references remain metadata for the respective owners.

Code Pad session import keeps original document IDs, cursors/bookmarks, both views
and eligible recent files. Other paths remain in the snapshot. Recovery import
keeps unsaved text/base hash/timestamp, replaces only explicitly reviewed conflicts
and preserves unrelated buffers. Capacity overflow rejects the whole merge without
oldest-buffer eviction. Closed tabs, native context, expiry, one-use previews and
exact destination preimages guard both imports. Consumer metadata and receipts
commit atomically, so repeat import preserves later edits/discards. Bounded exact
preimages are durably preserved before replacement and support checked restoration.

Hosted session autosave and recovery save/discard carry native revisions. Stale
writes preserve editor text; the recovery dialog chains revisions, reports
unavailable entries individually and retains backups on conflict. Standalone wire
formats remain compatible. Import creates no file/execute authority or repository
write. Actual buffer recovery remains a separate Files preview/apply action.
Normal metadata publication and a preserved preimage are separate atomic steps;
no transaction across arbitrary files, registry or external vault is claimed.

## Reviewed LSP configuration import

Verified Code Pad LSP settings now reach current-project native configuration and
its actual management dialog. Before/after review shows preserved server/runtime
preferences with the current root and forced disabled state; history restoration
also disables execution. Closed native tabs, one-use context/expiry/preimage tokens
and the same exclusive context permit as normal saves guard replacement. Normal
settings saves retain receipts; repeated imports preserve later choices. Exact
bounded preimages are preserved before atomic configuration/receipt publication.
Unknown/future/corrupt source or destination data remains untouched. Import does
not resolve a command, probe a runtime, open project files or inherit execution
approval. Offline-root metadata remains accessible.

Initial focused checks passed five portable config/import tests and strict Clippy
in **38.762 seconds**, peak **4,021,374,976 bytes**. Product **37 UI tests** and build
passed in **23.087 seconds**, peak **1,576,886,272 bytes**. Final focused validation passed **139 Workspace Rust tests**, strict Clippy and
five generated-fixture checks in **34.211 seconds**, peak **2,052,390,912 bytes**,
including token expiry/foreign context/offline metadata and disabled restoration.
Final affected verification selected all and passed in **429.869 seconds**,
cgroup peak **5,043,556,352 bytes** under 8 GiB. The new Windows actual-control/
repeat/restore fixture has not run on Windows yet. The completed product run targets the earlier `3dc56c6` recovery/host/path changes,
not this configuration import.

## Last workspace onboarding and recovery completion

The verified Code Pad session proposes its one persisted `workspace_folder` through
metadata-only path classification. Explicit Windows review accepts the native job
ID, reuses OS Registry discovery and requires separate registration/selection/trust.
WSL and unsupported paths remain visible without resolving them or starting a
distro. All registration probes, including imported profiles, use the separate
bounded probe pool. Snapshot replacement rejects stale job IDs.

Focused native classification/source/Registry tests, strict Clippy, **41 product UI
tests**, build and five generated Windows fixture checks passed in **70.666 seconds**,
cgroup peak **4,628,320,256 bytes**, within 8 GiB. Tests cover removed source/offline
metadata, stale jobs, late UI results, disabled review and explicit token-only
registration review. The Windows fixture now reviews the saved root after source
removal and verifies cancellation leaves Registry unchanged.

The `3dc56c6` packaged run passed Source/Files and the new session/bookmark flow,
then failed because completed recovery paths were ignored by the Files parent.
Files now opens those paths through normal native file admission after session
hydration. Existing document buffers retain the normal reopen behavior. Focused recovery/open validation passed **44 Files UI tests** and the product
build in **24.759 seconds**, cgroup peak **1,583,370,240 bytes**. Final affected verification selected all and passed in **427.846 seconds**,
cgroup peak **5,813,968,896 bytes** under 8 GiB. The final Windows execution
of these changes is pending.

## Windows corrections and verification

Code Pad's direct link-count probe now uses Rust extended-path handling. The added
Windows long-cache regression exposed another ordinary-path Win32 call in the
atomic session/index writer. It now canonicalizes the existing parent and retains
both leaf names for `MoveFileExW`, supporting missing-target first publication and
replacement without resolving the target leaf. Windows tests cover both writes,
residue, long-cache access and real hard-link rejection; WSL does not execute them.

Recovery-focused validation passed **135 Workspace Rust tests**, strict Clippy,
**34 product UI tests**, **47 Files UI/API tests**, five generated-fixture checks
and product build in **69.905 seconds**, cgroup peak **2,541,629,440 bytes**.
Follow-up host/session checks passed **nine portable tests** and strict Workspace/
Code Pad Clippy in **42.765 seconds**, peak **4,775,411,712 bytes**. A wrapper call
missing its separator failed before checks and was corrected. An earlier affected
run was cancelled to update the remaining Source fixture revision arguments;
recovery's corrected affected all passed in **425.804 seconds**, peak
**5,494,153,216 bytes**. Final affected verification after the host/index corrections also selected all
and passed in **434.537 seconds**, cgroup peak **5,937,729,536 bytes**, under the
shared 8 GiB budget.

| Windows evidence | Actual result |
|---|---|
| [0066509 CI](https://github.com/jihoon22-lee/devbox/actions/runs/34322593145) | PASS, including native Windows recovery tests. |
| [0066509 product](https://github.com/jihoon22-lee/devbox/actions/runs/34322593106) | Native authority/WAL, packaged Source/Files (actual chooser/worktree/Git/edit/cleanup), API/Knowledge and installer coexistence PASS. Rust archive import succeeded; subsequent status failed on a 264-character cache path. Full LSP acceptance did not pass. |
| [3db1a59 CI](https://github.com/jihoon22-lee/devbox/actions/runs/34326324526) | FAIL: real debounce tests exceeded CI's five-second default and the Windows long-cache regression failed while publishing the installed index. Time budgets and the second direct Win32 path are corrected locally; new Windows results pending. |
| [3db1a59 product](https://github.com/jihoon22-lee/devbox/actions/runs/34326324514) | Native authority/WAL, API/Knowledge and coexistence PASS. Linked-worktree Source history returned busy before a diff, so downstream Files/session/LSP checks were not reached. Empty invoke probes do not establish the exact native admission cause. |
| 3db1a59 baseline | 14/15 runtime cases passed; WSL Desktop startup failed and baseline pnpm download hit TLS ECONNRESET. Run Manager's known owned-task baseline failure still misses its performance budget. R24 remains incomplete. |
| [3dc56c6 CI](https://github.com/jihoon22-lee/devbox/actions/runs/34329557876) | PASS, including Windows check, Clippy and tests; the long-cache atomic writer regression is resolved on Windows. |
| [3dc56c6 product](https://github.com/jihoon22-lee/devbox/actions/runs/34329557848) | Native authority/WAL, packaged Source/Files, session import/bookmarks/repeat/history, API/Knowledge and coexistence passed. Imported recovery reached explicit apply, but no editor tab reopened; remaining recovery/LSP checks were not reached. |
| [3dc56c6 baseline](https://github.com/jihoon22-lee/devbox/actions/runs/34329557848) | Public v0.7 assets and all 15 runtime cases PASS; six measured budgets PASS. Run Manager's known owned-task spawn failure remains, and live PTY restore/integrated comparison are unmeasured: `r24Passed=false`. |

The generated Windows session fixture now owns an exclusive synthetic Code Pad
folder, verifies the snapshot, removes only that owned source, and exercises real
CodeMirror bookmarks, stale autosave, repeat import and previous-session restore.
Its recovery extension checks imported buffers, stale discard rejection, disk
unchanged until explicit recovery, repeat after discard and previous recovery
restore. The `870f8a8` run passed recovery reopening, repeat/discard and previous-state
restoration. It reached disabled LSP settings import before the admission race below;
full packaged LSP/journal recovery acceptance is still pending.

Vitest/mocker were updated to 4.1.11 after CI reported
[GHSA-82fw-gwwq-j7x9](https://github.com/advisories/GHSA-82fw-gwwq-j7x9).
The corrected audit and notice policy checks passed; production dependencies were
not changed. Earlier detailed checkpoints and intermediate failures are retained in
[17986e9's packet](https://github.com/jihoon22-lee/devbox/blob/17986e9/workthrough/2026-09-08-v08-b04-workspace-source-files.md).

## Remaining acceptance and rollback limits

Template editing Windows acceptance and WSL instantiation,
older references and provider handoff remain incomplete. WSL-native Git/LSP,
remote Files IO/cancellation and Windows/WSL end-to-end acceptance remain required.
Native filesystem cancellation is cooperative; current Files IO is serialized.
No unrun Windows/manual check or preservation-only step is counted as PASS.

Reverting product code does not delete legacy snapshots, imported destination data,
preimages or new user edits. Native journal recovery owns filesystem rollback and
may preserve partial results. Suite migration/activation/installer rollback and
exact-main stable promotion belong to B08/B09; this draft provides no release claim.

## Workbench 템플릿 가져오기와 프로젝트 생성

- 기존 `ProfileTemplateStore`의 bounded strict 검증을 재사용한다. 검증 완료한 Workbench 보관 작업 ID로만 읽으며 원래 ID·이름·빈 경로·WSL·Git·포트·서비스 기본값을 Registry의 독립 사본으로 보관한다. import/reuse/keep-both/skip은 revision·원본 사본 digest·one-use 만료 token과 최종 byte CAS를 확인하고 기존 프로필·프로젝트를 유지한다.
- 프로젝트 관리에서 템플릿과 실제 Windows 폴더를 선택하고 native probe 결과를 검토한다. 등록, 새 프로필 ID와 실제 템플릿 출처 기록, 폴더 연결을 하나의 Registry 저장으로 반영한다. 환경 데이터는 비어 있고 현재 선택·실행 신뢰는 별도다. 취소·기존 폴더 연결 충돌은 저장을 남기지 않는다.
- Rust template/owner/route 검증과 strict Clippy, Workspace UI **44개**, build, 생성된 Windows fixture 표현식 **5개** PASS(44.074초, peak 3,887,083,520 bytes/8 GiB). 후속 UI 상태 표시 정리까지 포함한 최종 `pnpm verify:affected`는 all을 선택해 **445.250초 / peak 6,198,820,864 bytes / 8 GiB**로 PASS했다. 초기 집중 실행에서 잘못된 pnpm filter로 UI가 선택되지 않은 결과는 UI PASS에서 제외하고 올바른 `devbox-workspace` filter로 위 검증을 실행했다.
- `.github/scripts/windows-workspace-template-import.mjs`는 hosted 전용 원본의 byte 보존·원본 없이 보관본 읽기·repeat/replay·실제 템플릿 생성 UI·출처/원자적 연결·선택/신뢰 미변경·해제 후 데이터 보존을 검사한다. `870f8a8`의 hosted Windows fixture는 실제 템플릿 생성과 9개 보존/권한 검사를 통과했다. 템플릿 편집은 아래 후속 구현에 포함하며 WSL 생성은 미완료다. 앞선 `7d857f8` [Windows 제품 수용](https://github.com/jihoon22-lee/devbox/actions/runs/34332819276)은 native authority/WAL 테스트 단계 실패로 packaged shell/복구/LSP 실행을 건너뛰었다. 실패는 `settings_import.rs`의 history 변조 단계에서 발생했다. 이전 import/repeat/restore는 통과했으며 아래 긴 경로 수정으로 후속 검증한다.

## Windows 긴 보관 경로 교체

- `7d857f8` 제품 native fixture는 131/132 통과 후 LSP 설정 history 변조용 atomic write에서 `files_store_unavailable`로 실패했다. 공용 writer는 Rust로 긴 임시 경로를 생성한 뒤 확장 형식 없는 경로를 직접 `MoveFileExW`에 전달하고 있었다.
- disposable 로컬 Windows native probe에서 일반 경로는 오류 3으로 실패하고 같은 경로의 extended 형식은 성공함을 재현했다. 기존 parent를 한 번 canonicalize한 뒤 임시/목적 sibling 이름을 붙여 전달한다. overwrite/write-through flags와 bounded sharing-error retry를 유지하며 없는 부모를 생성하지 않는다.
- 깊은 한글/공백 generation 경로·64자리 history 파일의 최초 생성/교체/임시 파일 없음 회귀 검사와 기존 atomic write 3개, LSP import 2개, strict Clippy PASS(37.114초, peak 4,911,210,496 bytes/8 GiB). Windows Rust 실행은 CI에서 확인해야 한다. 최종 `pnpm verify:affected` all PASS(**638.188초 / peak 6,444,978,176 bytes / 8 GiB**). 일반 Windows CI도 동일한 history 교체에서 실패했으므로 다음 head에서 두 workflow를 다시 검증한다.

## 기본 창 상태 검토·이전·복원

- Workbench/Code Pad/Repo Manager의 실제 `window-state-v1.json`만 고정 inventory로 보관한다. 새 snapshot schema v2는 창 파일을 포함하고 v1은 당시 고정 inventory를 그대로 사용하여 기존 content ID·원본 bytes·재검증 호환성을 유지한다. unknown/future/corrupt 입력은 성공 개수나 적용 권한으로 바꾸지 않는다.
- native job ID로 확인한 상태를 기존 `window-state`의 모니터/DPI 보정에 연결한다. 현재/적용 후 크기·위치·최대화를 검토하고 명시적 교체 동의와 one-use/3분 token으로 적용한다. 창/모니터 변경·만료·재사용을 거부한다. 현재 native geometry는 Overview generation의 bounded immutable history에 먼저 보관하고 같은 절차로 복원한다. 프로젝트 Registry·선택·실행 신뢰는 변경하지 않는다.
- native 창 API는 UI 스레드에서 실행하고 기존 coalescing writer/normal bounds memory를 사용한다. metadata IO는 bounded worker에서 수행한다. deadline/취소를 각 OS 변경 전에 확인하며 OS 일부 실패와 파일 저장을 단일 transaction으로 주장하지 않는다. 보관한 이전 geometry는 부분 실패 후에도 남는다. main→main만 대응하며 secondary mapping은 B07이 소유한다.
- 회귀 검증: v1/v2/restart compatibility, 원본 제거 뒤 가져오기, stale/취소/재사용/확인 누락, partial native failure와 손상 history 보존, UI 교체 동의/복원/late token, 실제 hosted native 창 UI fixture를 추가했다. 집중 native window/owner/role 8개와 snapshot 9개, inventory 4개, adapter monitor/DPI 회귀 1개, strict Clippy PASS. 기존 Registry UI mock의 새 history 응답을 보완한 뒤 Workspace UI 48개와 build PASS(24.193초, peak 1,543,094,272 bytes/8 GiB); 생성된 fixture 표현식 5개도 PASS. 필터 때문에 실행되지 않은 adapter 일반 테스트는 이 집중 결과에 포함하지 않는다. 최종 `pnpm verify:affected` all PASS(**797.673초 / peak 6,445,101,056 bytes / 8 GiB**), 공용 adapter 전체 테스트도 포함했다. 앞선 `870f8a8` Windows native authority/WAL은 통과해 긴 경로 교체 수정이 확인됐으나 packaged shell 단계가 실패했다. 해당 job 완료 후 상세 로그를 확인하며 새 창 상태 fixture의 Windows 실행은 아직 하지 않았다.

## Windows 수용 fixture의 admission 경합

- [870f8a8 제품 수용](https://github.com/jihoon22-lee/devbox/actions/runs/34336335989)은 native authority/WAL, 템플릿 9개, Source 22개, Files 9개, 실제 복구 후 편집기 재열기·반복·이전 복구 복원, LSP 설정 import를 통과했다. API/Knowledge와 설치 공존도 통과했다. 전체 packaged 결과는 FAIL: `windows-workspace-session-import.mjs:127`의 의도적으로 오래된 LSP 저장 요청이 6ms 만에 `unavailable / workspace.dispatch / rejected`로 admission 전 거부되어 revision 충돌 확인까지 도달하지 못했다. background context activity와의 경합이며 수락된 쓰기 결과가 아니다. artifact의 source는 해당 PR merge SHA `95e6732daa39c154fdede8a47d68f7b2031109cf`다.
- fixture 요청은 정확히 이 native pre-admission envelope만 최대 20회·50ms 간격으로 재시도한다. 원래 문맥/입력/deadline을 고정하고 새 request ID를 사용한다. 수락된 실패, transport 실패, 다른 권한/출처 오류는 재시도하지 않는다. native 권한·context exclusion은 유지한다. 템플릿과 창 상태 증거는 각각 완료 직후 기록한다.
- generated JavaScript와 fresh-ID/fixed-context/deadline, 재시도 상한, accepted/ambiguous failure 비재시도 **7개** PASS(0.520초). 최종 `pnpm verify:affected` all PASS(**369.214초 / peak 1,651,732,480 bytes / 8 GiB**). 다음 Windows 결과는 완료 후 기록한다.

## 이전 Code Pad identifier의 독립 가져오기

- 기존 standalone에 남아 있는 `com.workbench.codepad`를 명시적 `code-pad-legacy` source로 추가했다. 현재 `com.devbox.codepad`와 각각 선택해 보관하며 고정 native identifier만 허용한다. 원본 root 이름 변경·병합·자동 조회를 하지 않는다. 같은 bytes도 source별 snapshot ID를 유지한다.
- 세션·미저장 복구·LSP 설정·마지막 폴더·창 상태가 기존 검증 완료 job 및 명시적 적용 절차를 사용한다. current/older 동시 존재, 원본 bytes 유지, 이전 원본 제거와 재시작 후 보관본 검증·읽기, 같은 bytes의 독립 ID, corrupt/future window 및 동적 identifier 거부를 검증했다. Registry 선택/신뢰는 바꾸지 않는다.
- 집중 Rust importer/inventory/snapshot 및 strict Clippy, Workspace UI 52개·build·generated request 7개 PASS(**42.476초 / peak 2,726,805,504 bytes / 8 GiB**). 초기 검증의 test expression 치환 오류를 바로잡고 위 검증을 다시 실행했다. 최종 `pnpm verify:affected` all PASS(**646.062초 / peak 6,444,781,568 bytes / 8 GiB**). 이 실행 중 아래 생성 고지 파일을 갱신했으며 이후 전체 Rust compile/test도 완료했다.
- hosted Windows 세션 fixture는 두 전용 원본을 함께 둔 상태에서 이전 source만 보관하고, 원본 제거 후 재검증·offline metadata 조회와 current 원본/Registry 보존을 확인한 뒤 기존 전체 세션 흐름으로 이어진다. Windows 실행은 다음 head에서 확인한다.

## 고지 파일의 lockfile fingerprint 동기화

- `161d2af` CI 의존성 정책은 취약점이 아닌 `THIRD_PARTY_NOTICES.md`의 오래된 Cargo.lock fingerprint 때문에 실패했다. 창 adapter의 직접 의존성 두 줄 추가 후 생성 파일 동기화가 빠져 있었다. CI의 pnpm audit는 알려진 취약점 없음으로 통과했다.
- 저장소 생성기로 고지 파일을 다시 생성했다. diff는 Cargo.lock SHA-256 한 줄뿐이며 외부 의존성·버전·license 목록은 같다. 위 최종 affected all은 생성 이후 Rust compile/test까지 통과했다. 독립 dependency policy check도 PASS(2.326초)하여 Cargo.lock/pnpm-lock과 고지 파일 일치를 확인했다. 다음 CI에서 최종 결과를 확인한다.


## Workspace 템플릿 생성·수정·보관

- Overview에서 기존 순수 템플릿 검증기를 재사용하여 새 템플릿 작성, 목적지 사본 수정, 보관 확인, 수정 후 복원을 제공한다. native 생성 ID와 명시적인 local origin을 사용하며 가져온 사본의 snapshot/template 출처를 유지한다. snapshot ID 누락을 local로 오인하지 않도록 origin 조합을 strict 검증한다. 새 local/archive 필드를 모르는 이전 버전은 읽기를 거부하고 기존 bytes를 보존한다.
- 편집 시작 당시 Registry revision과 최종 byte CAS를 확인한다. stale/식별자 변경/활성 이름 중복/크기 한도는 저장을 남기지 않고 UI 초안을 보존한다. 같은 내용 저장·이미 보관된 항목은 revision을 늘리지 않는다. 보관은 record와 기존 프로필 연결을 유지하고 신규 생성 선택에서 제외한다. 동일 snapshot의 반복 가져오기는 Workspace에서 수정·보관한 상태를 그대로 재사용한다.
- 프로젝트/작업 폴더/기존 프로필/실행 신뢰는 템플릿 편집으로 바뀌지 않는다. 실제 생성은 기존 native 폴더 검토와 원자적 프로필 연결을 사용한다. 새 IPC는 Overview Registry에만 허용하고 source/local/archive 메타데이터는 renderer 입력으로 받지 않는다.
- 최종 집중 검증: native template/import/owner/route **9개**, UI template/Registry/Source **17개**, strict Clippy, Workspace build, generated Windows 요청 **7개** PASS(**22.014초 / peak 1,725,837,312 bytes / 8 GiB**). owner fixture는 Linux에서 명시적인 test WSL target metadata를 사용한다. 이는 실제 WSL 연결 PASS가 아니다. 기존 Registry UI assertion은 새 template details와 프로젝트 form의 동일 포트를 구분하도록 범위를 지정했다.
- hosted fixture에 실제 수정·stale 저장 거부·보관 취소/확인/복원·반복 가져오기와 기존 프로필 보존·새 local 템플릿 생성/프로젝트 연결을 추가했다. 실제 Windows 실행은 아직 하지 않았다.

## 후속 Windows 결과와 Source 초기 화면

- [161d2af 제품 수용](https://github.com/jihoon22-lee/devbox/actions/runs/34339636174)은 native authority/WAL, 템플릿, 새 기본 창 UI 가져오기/이전 native geometry 복원 **7개**, API/Knowledge, installer coexistence를 통과했다. artifact source는 PR merge SHA `61a4853313c76fd83bd5e1807560dc7c750d2945`다. packaged shell은 이후 Source의 Git 승인 버튼 대기에서 실패해 이 실행의 Files/복구/LSP는 PASS로 계산하지 않는다.
- Source의 첫 commit frame에서 초기 조회 effect가 실행되기 전에 버튼이 활성화되는 구간을 제거했다. 처음부터 busy로 표시하고 조회 종료 후 허용한다. layout-phase 회귀 테스트가 초기 비활성화 및 조회 후 한 번의 검토 요청을 확인한다. Windows 실패의 정확한 버튼 이름을 다음 artifact에 남기도록 fixture label도 보완했다. 이 변경이 위 Windows 경합을 해결했는지는 다음 패키지 실행으로 확인한다.

- 전체 affected 첫 실행은 초기 bundle **291,317 > 280,000 bytes**에서 실패해 Rust 검증까지 진행하지 않았다. 템플릿 관리 버튼을 열 때 editor/validation 모듈을 lazy load하도록 변경했다. 후속 UI 17개·build·generated 요청 7개는 통과했고, 올바른 `--scope all`로 실행한 bundle checker가 19개 앱 모두 PASS했다(0.776초). 처음 scope 없는 checker 호출은 인자 오류로 실패했으며 PASS에 포함하지 않는다. 한도는 변경하지 않았다.

- lazy 변경을 포함한 최종 `pnpm verify:affected` all **442.866초 / peak 6,443,999,232 bytes / 8 GiB** PASS. Workspace 초기 bundle은 **277,925 bytes / 280,000 bytes**, gzip **82,039 / 90,000 bytes**다.


## GUI 없는 파일·LSP 코어와 설치 상태 조회

- Code Pad의 기존 기본 standalone은 desktop을 포함하며 Workspace component는 desktop을 명시한다. 기본 feature 없이 파일/세션/LSP 코어를 컴파일할 때만 Tauri·플러그인·watcher·window adapter를 제외한다. 기존 native 구현을 복사하지 않으며 standalone의 WSL 경로 저장/Windows LSP 설치 기능은 유지한다. 실제 WSL helper/배포 경로 연결은 다음 단계다.
- GUI 없는 core check, 파일 회귀와 strict Clippy를 수행했다. 첫 Clippy에서 UI wrapper 전용 함수의 미사용 경고를 발견해 같은 desktop 경계에 넣었다. 후속 **181개 native core 테스트와 strict Clippy PASS(12.119초 / peak 621,592,576 bytes / 8 GiB)**. normal/build dependency tree에 Tauri/GTK/WebKit이 없음을 확인했다.
- [4dda0c5 제품 Windows 수용](https://github.com/jihoon22-lee/devbox/actions/runs/34341801214)은 native authority/WAL, 템플릿/창 상태, Source 22개, Files 9개, 이전 Code Pad identifier 4개와 전체 세션·미저장 복구·LSP 설정 import/repeat/restore를 통과했다. artifact source는 `5a3d4401ecf3f63f5790d136a25f6f6f4781c732`다. 관리형 Rust archive의 실제 picker/import/cache 확인까지 진행한 뒤 LSP 설치 UI의 초기 조회 오류로 packaged 단계가 실패했다.
- `LspControlPanel`과 `ManagedInstallerPanel`이 같은 설치 상태를 동시에 조회하고 있었다. 설치된 archive/tree digest를 확인하는 동안 native installer의 단일 operation lock이 두 번째 조회를 거부할 수 있다. 부모의 중복 조회를 제거하고 기존 자식 `onChanged` snapshot을 선택 UI와 공유한다. 조회 실패에는 명시적 새로 고침을 제공한다. 집중 UI 및 두 consumer build PASS(23.549초 / peak 1,631,207,424 bytes / 8 GiB). 느린 초기 조회가 한 번만 호출되는 회귀와 실패 후 새로 고침이 설정·설치를 변경하지 않는 회귀를 추가했다.
- 같은 Windows run의 Knowledge WSL1 fixture는 검색 작업 retirement 후 전용 corpus를 Linux에서 옮길 때 Permission denied로 실패했다. Windows redirector handle 지연을 고려해 동일한 전용 `mv -T`의 명시적 permission-denied 결과만 10초 내 재시도한다. timeout/signal/다른 결과는 반복하지 않으며 실제 이동과 후속 offline/reconnect 검사는 계속 필수다. 성공 시 시도 횟수를 artifact에 남긴다. JavaScript syntax check PASS; 실제 재실행은 아직 하지 않았다.

- 위 변경의 최종 `pnpm verify:affected` all **485.173초 / peak 5,443,448,832 bytes / 8 GiB** PASS. GUI를 포함한 기존 desktop consumer들도 이 실행에서 검증했다. `4dda0c5`의 [일반 CI](https://github.com/jihoon22-lee/devbox/actions/runs/34341801216)는 Windows Rust와 dependency policy를 포함해 PASS했다. 수정한 설치 UI/WSL1 fixture 및 템플릿 편집은 다음 Windows head에서 확인한다.


## 저장 완료와 LSP 문서 revision 동기화

- `df28e1c` [제품 Windows 수용](https://github.com/jihoon22-lee/devbox/actions/runs/34345048429)은 템플릿 편집 15개, 창 상태 7개, Source 22개·Files 9개, 이전 Code Pad와 세션/복구/LSP 설정 import/repeat/restore, Rust·Node archive import/cache, 실제 LSP 실행 승인·초기 hover를 통과했다. Knowledge의 전용 WSL corpus 이동과 후속 검증도 통과했다. artifact source는 `cd22645fa13838c84090edb8049cd708c286bbd7`다. 실제 편집/저장 후 두 번째 hover가 `file_snapshot_changed`로 실패해 전체 packaged 수용은 FAIL이다.
- 저장 완료는 실제 제출한 text A와 반환된 native revision을 고정한다. 이전 revision의 didChange가 늦게 거부되어도 didSave에서 현재 disk/hash와 일치하는 A를 재동기화한다. 이후 입력 B가 있으면 didSave 뒤에 보내며 Files의 dirty hash를 중간에 지우지 않는다. stale/위조 saved text와 더 최신 native revision은 거부하거나 오래된 알림을 건너뛴다.
- 집중 native LSP actor 회귀, TS save/change ordering와 API 테스트, 두 consumer build PASS(**55.063초 / peak 2,779,910,144 bytes / 8 GiB**). 실제 두 번째 hover와 전체 journal 복구는 다음 Windows run으로 확인한다.
- 같은 head의 [일반 CI](https://github.com/jihoon22-lee/devbox/actions/runs/34345048518)는 카탈로그의 정규식이 Code Pad의 optional dependency 표기를 놓쳐 FAIL했다. 검사기는 해당 TOML 선언과 default feature의 전이 의존성을 읽어 기본 desktop의 단일 인스턴스 플러그인을 확인한다. 다른 Cargo 의존성의 TOML 1.1 multiline table은 Python TOML 1.0 parser에 넣지 않는다.

## WSL native observation과 배포 연결 준비

- `filesystem::project::ProjectObservation`을 Windows와 새 private Linux helper가 함께 사용한다. 경로 transport 검사 후 root/Git/common-dir/backlink의 retained object 및 pointer bytes를 검증한다. Linux는 retained descriptor의 filesystem ID/type + inode/birthtime을 사용한다. Rust musl의 `Metadata::created`가 지원되지 않아 고정 Linux statx ABI를 직접 사용하며 evidence가 없을 때 완화하지 않는다.
- helper는 metadata 전용 `hello/observe_root/validate_root/release_root`만 제공한다. version/session/request/sequence, 64 MiB frame, 최대 16 root, 180초 root expiry, 최대 30초 deadline을 검사한다. EOF/잘못된 frame/timeout은 metadata worker를 종료한다. write/Git/LSP를 추가하기 전 owned child retirement와 mutation recovery가 필요하다.
- Windows는 등록 GUID·등록 정보와 backing-directory File ID를 고정한다. native system WSL의 running 목록은 distro를 시작하지 않는다. explicit start 허용 전에는 stopped 오류를 반환한다. static ELF는 같은 CI source SHA에서 빌드하고 manifest 크기/hash/ELF 구조를 확인해 제품 resource에 넣는다. build script가 검증 digest를 컴파일하며 client는 실행 전에 다시 hash와 read-only file lease를 유지한다. native `--cd <Windows resource directory> --exec ./devbox-workspace-wsl`을 사용해 Linux mount root를 가정하지 않는다.
- GUI 없는 helper/shared-root/기존 Windows probe 및 strict Clippy PASS(**65.908초 / peak 4,704,894,976 bytes / 8 GiB**). musl static build와 실제 musl 3개 테스트 PASS(**6.704초 / peak 337,575,936 bytes / 8 GiB**). 최초 musl birthtime와 libc statx 노출 실패를 수정한 뒤 다시 검증했다.
- 현재 실행 중인 로컬 WSL에 한정한 실제 Windows pipe probe 7개, Windows 전용 임시 resource copy/hash/read lease와 그 directory에서의 실행을 포함한 probe **9개 PASS**. root 재검증, foreign token/replay 거부, helper 재실행의 stable identity/new token, EOF 종료, 원본 marker 보존을 확인하고 전용 임시 파일을 제거했다. 이는 Tauri 앱의 WSL 기능 완료나 WSL1/WSL2 suspend 수용을 의미하지 않는다.
- 최신 native Windows MSVC compile/strict Clippy(실제 platform source의 독립 harness), helper 테스트와 카탈로그 검사 PASS(**7.205초 / peak 500,142,080 bytes / 8 GiB**). 제품 CI에 같은 artifact를 사용한 Rust client의 owned WSL1 fixture와 stopped/start 검사를 추가했다. 아직 실행하지 않았다. Registry/Files/Git/LSP admission은 기존 제한을 유지한다.

- artifact staging의 source SHA 불일치·bytes 변조·동적 interpreter·잘못된 ELF header 거부 회귀와 helper check, notices 생성 PASS(**2.319초 / peak 208,187,392 bytes / 8 GiB**). 첫 affected 실행은 scope 회귀에서 native helper를 20번째 앱으로 계산해 **3.348초에 FAIL**했다. helper를 private crate로 분류하고 `apps/*/native` 수정이 frontend 대신 native package와 Windows consumer를 선택하도록 고쳤다. manifest/dependency 범위 회귀도 추가해 PASS했다.

- 현재 작업 트리의 최종 `pnpm verify:affected` all **976.231초 / peak 6,445,092,864 bytes / 8 GiB PASS**. 공유 Cargo feature 변경으로 기존 앱 테스트 바이너리도 재빌드했다. Windows 실행 결과는 다음 CI에서 확인한다.


## WSL 등록 검토와 실행 파일·배포판 식별 보완

- WSL helper 파일뿐 아니라 volume root부터 resource directory까지 native handle을 유지하고 delete sharing을 허용하지 않는다. 각 열린 대상에서 reparse/type을 확인해 hash 검사와 실행 사이의 부모 폴더 교체도 차단한다. WSL2는 GUID/등록 정보/BasePath 외에 실제 backing image의 File ID를 유지·재검증해 같은 directory 안의 VHD 교체를 구분한다. image 내용은 읽지 않는다. 경로는 Microsoft [DistributionRegistration](https://github.com/microsoft/WSL/blob/master/src/windows/service/exe/DistributionRegistration.cpp)의 BasePath/VhdFileName 계약을 따른다.
- 실제 Windows API flags로 helper 파일 write/rename와 부모 directory rename 거부를 확인한 채 Windows resource path에서 WSL helper를 실행했다. 현재 WSL2 backing image의 metadata-only 반복 식별, token/replay/restart/EOF와 원본 marker 보존까지 **10개 PASS**. 전용 Windows/Linux 임시 리소스와 handle을 정리했다. 이는 Rust client/Tauri WSL 기능이나 WSL1 실행 PASS가 아니다.
- Windows-only resource lease 회귀의 Clippy 위치 경고를 수정했다. 후속 MSVC platform compile/strict Clippy, filesystem tests, Workspace Clippy PASS(**12.145초 / peak 1,204,297,728 bytes / 8 GiB**). 최초 경고 실행과 수정이 적용되지 않은 중간 재시도는 FAIL로 기록한다.
- Host가 native resource directory를 선택하고 WSL 프로젝트 검토를 기존 Registry writer에 연결한다. native distro GUID/실제 Linux root 객체를 확인한 expiring token만 등록·재연결에 사용한다. 기존 metadata CAS·duplicate discovery·취소는 공유하며 등록은 선택/실행 신뢰를 주지 않는다. `list_wsl_distros`/`preview_wsl`은 Overview의 별도 bounded probe pool을 사용한다.
- WSL 폴더 입력은 명시적으로 열 때 lazy load하고 배포판 목록 조회만 수행한다. 중지 배포판은 시작 선택을 요구한다. 닫힌 화면의 늦은 결과는 native 검토를 취소한다. 초기 busy와 refresh generation이 다른 등록 동작·오래된 목록 게시를 막는다. WSL 오류 문자열도 실제 해당 오류가 발생할 때 불러온다.
- native Registry owner 7개·Clippy PASS(**95.706초 / peak 5,475,811,328 bytes / 8 GiB**). 후속 route/owner/UI 검증에서 11 UI 테스트는 통과했지만 기존 TS target의 `Array.at` 미지원으로 build가 실패해 `pop`으로 수정했다. MSVC platform all-target Clippy, helper 4개, Workspace all-target Clippy, UI 11개·접근성·build·bundle PASS(**28.711초 / peak 2,034,245,632 bytes / 8 GiB**). 당시 초기 bundle은 279,726/280,000 bytes였다. 후속 오류 문자열 lazy 변경의 최종 build/affected는 별도로 확인한다.
- hosted `tests/wsl_native.rs`는 원본 marker를 유지하면서 등록 preview/cancel/replay·실제 untrusted Registry 등록·stopped/start 후 동일 binding·repeat discovery를 검사한다. **실제 Windows 실행은 아직 하지 않았다.** Files/Git/LSP admission은 계속 WSL 전용 구현을 요구하며 Windows 파일 접근으로 대체하지 않는다.

- 오류 메시지 lazy load를 포함한 최종 집중 검사: artifact 거부 회귀, dependency/notices 및 카탈로그 계약, UI 11개·접근성·build·bundle PASS(**22.837초 / peak 1,572,782,080 bytes / 8 GiB**). Windows 통합 fixture는 실행하지 않았다.

- WSL 등록 최종 `pnpm verify:affected` all PASS(**663.243초 / peak 6,444,986,368 bytes / 8 GiB**). 초기 bundle **278,634/280,000 bytes**. 앞선 `f3c9642`의 일반 [CI](https://github.com/jihoon22-lee/devbox/actions/runs/34353884389)는 통과했지만 [Windows 제품 검사](https://github.com/jihoon22-lee/devbox/actions/runs/34353884387)는 저장 후 두 번째 LSP hover에서 여전히 실패했다. 새 WSL 코드는 해당 실행에 포함되지 않았으며 LSP 경합은 별도로 수정·검증한다.

## Windows LSP 저장 알림의 파일 상태 경합

- Files metadata mirror/session 작업이 잠금을 가진 동안 native `didSave`가 `files_unavailable`로 즉시 실패하는 회귀를 재현했다(**52.747초 FAIL**). 파일 read permit·metadata lock을 알림 전 단계에서 원래 요청 만료 시각까지 기다리고, 그 사이 context retirement를 확인한다. snapshot 검증과 buffer 동기화는 같은 lock 안에서 처리한다. LSP 전송·파일 변경을 재실행하지 않는다.
- 같은 actor 회귀가 만료 요청 거부, 유효 요청의 잠금 대기·해제 후 성공, 저장 뒤 새 dirty buffer 보존과 rename 경계를 확인했다. 집중 actor test·all-target strict Clippy·Windows fixture 구문 검사 PASS(**45.690초 / peak 3,933,020,160 bytes / 8 GiB**). 중간 첫 compile은 formatting 후처리의 기존 sync 호출 누락으로 실패해 복원했다.
- Windows fixture는 전용 renderer의 문서 알림·파일 mirror에 대해 method/outcome/issue code/소요 시간만 최대 128개 남긴다. 파일 내용·경로·인자·임의 error message는 기록하지 않는다. 두 번째 hover 전에 실제 UI didSave 성공을 확인한다. Windows에서 이전 실패의 원인이 이 경합과 일치하는지는 다음 실행으로 확인한다.

- 저장 경합 수정과 trace를 포함한 최종 `pnpm verify:affected` all PASS(**433.335초 / peak 6,443,667,456 bytes / 8 GiB**). 실제 Windows와 WSL1 검증은 새 head의 CI에서 확인한다.

## 공용 파일 owner와 WSL 읽기 경계

- `FileOwner`, Windows 경로 판별과 순수 protected-storage 검사를 private native library로 이동했다. Windows host와 Linux helper가 같은 encoding/CRLF·snapshot·alias·dirty-buffer 검사를 사용한다. Tauri의 보호 경로 수집은 host에 남기고 native root lease와 path admission만 주입한다. 기존 파일 회귀 **10개**를 함께 이동했다. 초기 trait-object Scope 변환 누락을 수정한 뒤 check/test/strict Clippy PASS(**36.434초 / peak 2,307,178,496 bytes / 8 GiB**).
- helper의 `files_attach/open/close/sync_editor`는 root token에 고정된 문맥을 요구한다. Windows client도 distro GUID를 확인한다. 아직 product Files route와 write/Git/LSP로 연결하지 않았다. root-filesystem device/ID와 실제 mount type을 함께 검사해 POSIX 경로의 drvfs/9p·다른 파일시스템·다른 배포판을 통한 Windows owner 우회를 막는다. `/tmp` tmpfs가 올바르게 거부돼 실제 fixture 위치를 기본 Linux 파일시스템으로 옮겼다. mount 구문/alias·실제 객체·context/범위/revision 및 기존 회귀 19개와 Clippy PASS(**7.752초 / peak 1,106,939,904 bytes / 8 GiB**). 첫 enum naming Clippy 실패도 수정했다.
- 실제 subprocess pipe의 문맥·파일 읽기·버퍼 원본 보존·EOF·replay 검사를 추가했다. 새 테스트의 Rust pattern 오류를 수정한 뒤 이 2개와 MSVC platform compile/Clippy, Workspace/helper all-target Clippy PASS(**8.818초 / peak 1,213,272,064 bytes / 8 GiB**).
- musl C compiler가 없어 첫 build가 실패했다. 시스템 설치 권한을 요구하지 않고 Ubuntu 저장소의 고정 `musl{,-dev,-tools}` 1.2.5-3build1 패키지를 전용 도구 경로에 풀어 사용했다. musl/gnu의 statfs type 차이는 byte stamp로 통일했다. 이후 musl 19개 테스트와 static release build PASS(**123.165초 / peak 1,264,128,000 bytes / 8 GiB**). Cargo는 Code Pad의 미지원 cdylib를 제외하고 rlib를 사용한다고 경고하며 static helper를 정상 생성했다. EOF는 현재 읽기/metadata 범위에서만 검증했고 mutation/child retirement는 다음 단계다.

## 편집 버퍼 단일 writer와 WSL 등록 값 검증

- 늦은 LSP change가 NativeEditorMirror의 더 최신 미저장 hash를 지우는 회귀를 재현했다(**71.310초 FAIL**). LSP open/change/format 결과가 UI buffer hash를 쓰지 않도록 분리했다. 서버의 document/dirty 상태는 기존 LSP manager가 소유하고 모든 UI buffer의 native acknowledgement는 기존 mirror가 담당한다. 지원 언어가 없는 dirty 문서·stale save·rename 전후 회귀를 포함한 actor/Clippy PASS(**50.457초 / peak 3,849,076,736 bytes / 8 GiB**). 중간 rename 후처리 lock의 mut 누락도 수정했다.
- `5a9ed93` [Windows 제품 검사](https://github.com/jihoon22-lee/devbox/actions/runs/34359477964)는 실제 Rust client의 첫 connect에서 `wsl_registry_changed`로 실패했다. 이후 Windows 편집 검사도 기본 순서 때문에 건너뛰었으므로 LSP 수용으로 보고하지 않는다. 독립 native/packaged 검사는 제품 build 성공 시 계속 실행해 각 실패 근거를 남기도록 고쳤다.
- Registry LastWriteTime 전체를 identity로 저장하던 부분을 등록 값 전체의 bounded sorted digest와 유지 중인 read-only key handle로 바꿨다. 같은 값 rewrite는 유지하지만 DefaultUid/Flags/unknown value 변경, 키 삭제·재생성과 backing object 교체는 거부한다. 시각은 일관된 snapshot 확인에만 쓰고 최대 3번 metadata read를 시도한다. Microsoft [DistributionRegistration](https://github.com/microsoft/WSL/blob/master/src/windows/service/exe/DistributionRegistration.cpp)와 [LxssUserSession](https://github.com/microsoft/WSL/blob/master/src/windows/service/exe/LxssUserSession.cpp)의 등록 값 갱신 경로를 확인했다. 실제 실패와의 일치는 다음 Windows 실행에서 확인한다.
- Windows의 전용 HKCU fixture로 identical rewrite·DefaultUid/unknown policy·key replacement 검사를 추가했다. 실제 Windows 실행은 아직 하지 않았다. native platform MSVC compile/Clippy, shared/helper/Workspace Clippy, notices 생성과 카탈로그 검사 PASS(**9.022초 / peak 1,172,197,376 bytes / 8 GiB**). 다음 WSL1 fixture는 capture/launch 단계를 구분하고 실제 helper 파일 읽기·wrong context 거부도 확인한다.

- musl 실제 subprocess pipe 2개·ELF 구조/크기 검사·dependency/notices PASS(**5.424초 / peak 1,155,100,672 bytes / 8 GiB**). static ELF는 **1,127,168 bytes**, SHA-256 `18004fac338b5d45087a077f8b6a76b5ded643b3a7a366a4ba1698e565e93c29`다. 로컬 작업 트리 산출물이므로 exact-source CI/릴리스 증거로 사용하지 않는다.

- 첫 최종 affected는 scope 회귀가 새 `product-contract → workspace-wsl` 역의존 소비자를 예상 목록에 포함하지 않아 **3.098초 FAIL**했다. 실제 dependency graph에 맞게 `secrets` 소비자 기대값에 helper를 추가했다.

- 이번 묶음의 최종 `pnpm verify:affected` all PASS(**661.307초 / peak 6,445,309,952 bytes / 8 GiB**). 초기 bundle **278,634/280,000 bytes**, gzip 82,204/90,000 bytes. `5a9ed93` 일반 CI 34359478236은 전체 PASS; 제품 34359477964는 WSL connect 실패와 Windows editor skip 때문에 FAIL이며 새 변경의 Windows 결과는 아직 없다.

## WSL 저장·취소와 Windows 검증 순서

- helper의 명시적 `files_save`는 현재 native revision·disk snapshot을 요구하고 Code Pad의 encoding/CRLF 원자 교체를 재사용한다. 시작·staging 전·교체 직전에 취소와 native root/document 권한을 확인한다. 같은 요청의 이전 revision은 재사용할 수 없다. Files UI/Git/LSP 연결은 아직 남아 있다.
- 임시 저장 파일은 생성 handle과 부모 metadata handle의 native identity를 유지한다. 실패 정리는 여전히 같은 부모/파일인 staging만 제거하고 교체된 다른 객체는 보존한다. Linux metadata handle은 O_PATH로 내용 읽기 권한을 요청하지 않는다. Windows와 기존 journal 저장/복구도 같은 임시 파일 owner를 사용한다.
- EOF/잘못된 frame/timeout은 진행 중 저장의 precommit guard를 취소한다. 정상 종료에서 owner를 drop하고 Windows client는 버려진 응답을 bounded drain하며 종료를 기다린다. 막힌 syscall은 5초 후 종료되므로 원본 또는 완전한 새 파일을 유지하지만 staging이 남을 수 있다. 응답 없는 저장은 재조회·대조가 필요하며 자동 replay는 없다. 자식 프로세스 실행은 아직 제공하지 않는다.
- 파일 owner 교체/취소 회귀와 Code Pad 파일 21개·Clippy PASS(**20.448초 / peak 2,569,728,000 bytes / 8 GiB**), metadata handle 회귀·Clippy PASS(**1.554초 / peak 255,520,768 bytes**). helper 19개·실제 pipe 3개와 Workspace/helper Clippy PASS(**7.521초 / peak 1,165,172,736 bytes / 8 GiB**). pipe는 CRLF 저장·revision 갱신·stale 요청 거부와 8 MiB 저장 중 EOF의 완전한 원본/새 파일 경계를 확인한다. Windows 실행 근거는 아니다.
- `9b7ef92` [일반 CI](https://github.com/jihoon22-lee/devbox/actions/runs/34365592921)와 [제품 검사](https://github.com/jihoon22-lee/devbox/actions/runs/34365592910)는 FAIL이다. native Windows 137개는 통과했지만 identical registry write의 timestamp가 반드시 달라진다는 테스트 가정이 실패했다. 실제 값 변경/복원을 검사하도록 수정했다. WSL은 helper 실행 전 Lease capture의 `wsl_registry_changed`로 실패했으며 digest 변경만으로 해결됐다고 판단하지 않는다. 다음 실행에서 등록과 storage path/open 실패를 구분하고 nonce 소유 배포판 backing 경로의 metadata를 먼저 보존한다.
- packaged renderer는 첫 Workspace 시작 제한시간에서 실패해 LSP trace를 남기지 못했다. Cargo integration test가 일반 앱 바이너리를 다시 빌드하므로 native tests를 최종 Tauri 패키징보다 먼저 실행하도록 순서를 고쳤다. 독립 검사·패키징은 앞선 테스트 실패에도 진행한다. hosted WSL fixture에는 별도 합성 파일의 한글/CRLF 저장·stale revision 거부를 추가했다. 다음 Windows 결과로 실제 개선 여부를 확인한다.
- musl static build, musl helper 19개·실제 pipe 3개, Code Pad 파일 21개, Workspace LSP 27개, MSVC platform Clippy PASS(**106.826초 / peak 3,579,199,488 bytes / 8 GiB**). 새 PowerShell metadata preflight는 실제 Windows parser PASS이며 hosted 경로 실행은 아직 하지 않았다.
- [ReplaceFileW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew)가 replacement를 data sharing 없이 여는 제약을 확인했다. 전용 Windows TEMP 합성 파일의 실제 API probe에서 writable handle 유지 시 error 32, metadata-only handle로 전환 후 교체 PASS를 재현했다. staging writer를 flush한 뒤 동일 object의 metadata handle을 먼저 확보하고 writer를 닫도록 수정했다. handle 공백은 없으며 기존 ID 검사는 유지한다. 전용 임시 파일/폴더는 제거했다. 이 수정 후 최종 검증을 다시 수행한다.
- metadata handle 전환 후 Code Pad 파일 21개, helper 19개·실제 GNU pipe 3개 및 Code Pad/helper/Workspace all-target strict Clippy PASS(**33.379초 / peak 2,282,815,488 bytes / 8 GiB**). 실제 Windows Rust 저장·journal 수용은 최종 CI에서 확인한다.
- 최종 `pnpm verify:affected` all PASS(**760.270초 / sampled RSS peak 4,681,764,864 bytes, cgroup peak 6,445,240,320 bytes / 8 GiB**). 초기 bundle **278,634/280,000 bytes**, gzip **82,201/90,000 bytes**. 새 Windows Rust/helper/packaged 실행은 다음 CI에서 확인한다.

## Native 파일 동작과 목록·미리보기 재사용

- helper의 rename/delete는 native revision·snapshot과 교체 직전 취소/권한 검사를 사용한다. 기존 Windows file owner에도 같은 권한 재검사를 적용했다. Linux 이동은 [renameat2의 RENAME_NOREPLACE](https://man7.org/linux/man-pages/man2/rename.2.html)를 사용해 hard-link/unlink 사이의 부분 이동을 제거하며 미지원 파일시스템에서 fallback하지 않는다. attached editor root는 idle TTL 때문에 문서 revision을 잃지 않고 release/EOF까지 유지한다. preview 전용 root는 기존 TTL을 따른다.
- Code Pad의 folder/preview를 GUI 없는 helper에서도 사용한다. Windows Files와 helper의 목록 조회는 하위 폴더를 읽기 전에 admission을 확인하고 directory handle/identity·취소·최종 root를 검사한다. 50,000 files/400,000 entries/128 depth/8 MiB 응답 상한과 incomplete/truncated를 유지한다. Markdown 로컬 이미지는 읽기 전후 같은 native admission을 검사하며 취소되면 결과를 게시하지 않는다. sanitization/크기/remote-image 계약은 기존 renderer를 사용한다. WSL Files UI·Git/LSP는 아직 연결하지 않았다.
- 파일 동작 22개·helper 19개·pipe 3개·Clippy PASS(**31.002초 / peak 2,593,628,160 bytes / 8 GiB**). 새 guarded folder 5개·helper 19개·pipe 3개·전체 관련 Clippy PASS(**31.035초 / peak 1,697,177,600 bytes**). 네 번째 pipe는 실제 목록·기존 destination 보존·한글 rename·stale delete 거부·명시적 delete·EOF를 검사한다. 첫 실행은 테스트의 존재하지 않는 메서드 호출로 **0.521초 FAIL**, 수정 후 pipe 4개·Clippy PASS(**6.190초 / peak 702,693,376 bytes**).
- GUI 없는 preview 5개·helper/Workspace check/Clippy PASS(**40.028초 / peak 1,593,884,672 bytes**). 이후 image admission/취소 회귀와 실제 pipe Markdown preview, Linux atomic rename을 추가했다. 최종 집중 검사: GUI-free command 관련 35개, GNU/musl helper 각각 19개·실제 pipe 각각 4개, Code Pad/helper/Workspace Clippy 및 musl static release build PASS(**103.819초 / peak 1,826,439,168 bytes / 8 GiB**). 정적 ELF 구조 검사 PASS, **3,085,056 bytes**, SHA-256 `81823cbacf8b658b94adaa8494e7cd9b9114430bcd2f619595d44d082b17ea49`; 로컬 변경 산출물이며 exact-source CI 근거는 아니다.
- `1fd607a` 결과를 확인했다. 일반 Windows CI는 Code Pad **205 PASS/1 FAIL/1 ignored**이며 새 staging 테스트가 열린 파일을 가진 부모 폴더도 이동할 수 있다고 가정한 것이 원인이다. 실제 Windows API probe에서도 leaf MoveFileW는 성공하고 parent MoveFileExW는 error 5로 거부됐다. 테스트는 Windows에서 이동 거부·staging 정리·handle 해제 후 이동을 확인하고, 이동 가능한 환경에서는 교체된 부모/파일 보존을 검사한다.
- 제품 native 검사는 **137 PASS/1 FAIL**로 LSP WorkspaceEdit가 첫 파일 적용 전 실패했다. private journal 이동/교체에도 기존 session writer와 같은 canonical Windows parent를 적용하고 긴 temp/target 경로 회귀를 추가했다. test 빌드에만 기존 native 실패 이유를 남겨 다음 실행에서 구간을 구분한다. PowerShell 호스트의 291자 직접 MoveFileW probe는 성공했으므로 이 관찰만으로 Rust unit executable의 실패 원인을 확정하지 않는다.
- packaged UI는 첫 hover·편집·disk save까지 실행됐지만 didSave trace 확인에서 실패했고 trace는 빈 배열이었다. Tauri 2.11.5의 `invoke`가 non-writable/non-configurable이어서 기존 대입이 반영되지 않는 fixture 오류를 확인했다. 실제 IPC fetch의 응답 clone에서 method/outcome/고정 issue/시간만 관찰하도록 고쳤으며 원래 응답은 즉시 전달한다. readonly invoke·응답 보존·민감한 body/응답 미기록을 포함한 Node fixture **9개 PASS**. 실제 UI didSave와 두 번째 hover·journal 수용은 다음 Windows 실행이 필요하다.
- WSL은 여전히 helper 실행 전 capture의 `wsl_registry_changed`로 **0.21초 FAIL**이다. 초기 경로 metadata는 모두 통과했다. 다음 실행에서는 읽기 중 갱신·retained key 값 변경·등록 교체·backing directory 객체 변경을 서로 다른 고정 issue로 구분하며 identity 검사를 완화하지 않는다. WSL/native/UI 단계 직후 중간 artifact를 보존해 뒤의 독립 migration 검사를 기다리지 않고 실패 근거를 확인하도록 했다.
- Windows 실패 대응 후 로컬 집중 검사는 Code Pad command 관련 45개, 실제 LSP actor 문서/저장/rename 회귀 1개, Code Pad/helper/Workspace all-target Clippy, MSVC platform Clippy, Node fixture 9개 PASS(**100.850초 / peak 3,393,785,856 bytes / 8 GiB**). 새 Windows parent/long-journal 테스트는 다음 CI에서 실제 실행한다.
- 최종 `pnpm verify:affected` all PASS(**673.069초 / sampled RSS peak 4,578,197,504 bytes, cgroup peak 6,445,051,904 bytes / 8 GiB**). 전체 frontend/TypeScript·Rust tests·doc tests 및 관련 정책 검사를 통과했다. 현재 수정의 실제 Windows 실행은 다음 CI에서 확인한다.

## WSL Files 연결과 감시·복구

- 등록된 WSL context를 선택할 때 native helper 관찰과 Registry binding을 다시 비교한다. Files의 POSIX open/save/rename/delete/list/preview/buffer 동작은 별도 WSL owner로 보내며 Windows path IO에 전달하지 않는다. Windows native chooser 파일은 같은 뷰에서 독립 grant로 동작한다. helper가 확인한 경로/revision metadata만 session/recovery 저장의 근거로 사용한다.
- 세션·미저장 복구·가져오기·이력은 기존 Windows 전용 저장소와 CAS를 유지한다. 복구 미리보기와 명시적 적용은 Linux의 원래 encoding/CRLF와 native snapshot을 사용한다. metadata 기록은 파일 권한이나 실행 승인을 생성하지 않는다.
- 선택 context의 5초 폴링은 원래 부모/root 아래 새 leaf의 metadata를 읽되 열린 문서의 grant/revision·dirty hash를 바꾸지 않는다. 이벤트와 지연된 탭 정리에 원래 context를 붙여 다른 배포판의 같은 POSIX 경로를 구분한다. 원래 요청의 남은 deadline을 helper에 넘기며 종료가 확인되지 않은 연결 owner는 앱 종료 시 유지한다.
- 첫 Linux check는 새 native context reader의 임시 MutexGuard 수명 오류로 **5.972초 FAIL**이었다. 명시적 local value로 고친 뒤 Workspace check·helper 19개·metadata 회귀 1개 PASS(**44.537초 / peak 3,240,841,216 bytes / 8 GiB**). 확장 집중 검사: helper 20개·실제 pipe 5개, Files/import/recovery 8개·metadata 1개, UI/API 45개·TypeScript 및 관련 strict Clippy PASS(**50.157초 / peak 3,112,660,992 bytes / 8 GiB**). 새 pipe는 외부 교체 후 dirty revision 보존·stale 복구 거부·명시적 한글/CRLF 복구·EOF를 검사한다.
- 별도 LLVM 경로와 기존 SDK/CRT를 사용한 **전체 Workspace MSVC all-target check PASS(172.230초 / peak 2,817,265,664 bytes / 8 GiB)**. 플랫폼 일부만 확인하던 검사와 달리 실제 Windows Files/Registry/테스트 소스까지 컴파일했다. 기존 compiler cache의 끊어진 symlink는 수정하지 않고 전용 cache를 만들었다. 이는 Windows 실행 PASS가 아니다.
- `c10bbea` [일반 CI](https://github.com/jihoon22-lee/devbox/actions/runs/34380665218)는 전체 PASS다. [제품 CI](https://github.com/jihoon22-lee/devbox/actions/runs/34380665195)는 FAIL이며 native Workspace **138 PASS**, packaged UI 저장→didSave→두 번째 hover는 PASS다. journal의 직접 preview/cancel·변경 파일 거부는 통과했지만 UI 검토가 conflict로 끝났다. 복구에도 정제된 IPC trace와 구체적인 fixture action을 추가했다. Files metadata mutex의 일시적 경합은 파일 충돌이 아니므로 원래 deadline/최대 5초 내에서 기다린 뒤 열린 탭을 검사하도록 했으며, 실제 packaged 원인이 이것뿐인지는 아직 확정하지 않았다.
- `c10bbea` WSL1은 여전히 prelaunch capture에서 **0.25초 FAIL(`wsl_registry_changed`)**이다. 초기 backing path metadata는 통과했고 새 값/객체 비교 issue는 나타나지 않았다. 등록 목록을 전후 timestamp로 검증하고 조회 중 항목 삭제/목록 변경에만 최대 3회 snapshot 재시도를 적용했다. private registry fixture에 GUID 하위 항목 열거를 추가하고 backing 오류도 구분했다. retained key·모든 값·storage object identity 조건은 유지한다.
- WSL 실제 시작/파일 수용과 packaged journal 수용은 다음 Windows 실행에서 확인한다. WSL reveal·열린 문서의 연결 복구·Git/LSP, references/providers 및 B05~B09는 남아 있다. **B04/#546은 미완료, 전체 3/9**다.
- 종료 확인/남은 deadline 전달 이후 최종 집중 검사: native journal recovery 5개(150ms metadata 경합 회귀 포함), Node fixture 9개, musl helper 20개·실제 pipe 5개·static release build, 전체 Workspace MSVC all-target strict Clippy PASS(**81.992초 / peak 3,277,172,736 bytes / 8 GiB**). 새 WSL Windows bridge fixture는 컴파일만 통과했으며 실제 배포판 실행은 CI에서 확인한다.
- 최종 `pnpm verify:affected` all PASS(**510.161초 / sampled RSS peak 5,220,958,208 bytes, cgroup peak 6,444,560,384 bytes / 8 GiB**). 초기 bundle **278,745/280,000 bytes**, gzip **82,232/90,000 bytes**. 위 최종 변경의 Windows Rust 실행/packaged WSL·journal 수용은 다음 CI에서 확인한다.

## 실제 Windows 검사와 WSL 모드 판별 수정

- `1ad2e1d` 소스에서 Windows unit 실행 파일을 만들어 실제 로컬 Windows의 전용 TEMP와 비밀값 없는 환경에서 실행했다. Registry 1개·journal recovery 5개·WSL metadata 1개·파일 교체/지연 정리 1개, **총 8개 PASS**. 단위 테스트의 임시 폴더/키는 정리했다. Windows 제품 실행이나 WSL 배포판 acceptance로 계산하지 않는다.
- unit 실행 파일 빌드 **174.449초 / peak 4,269,891,584 bytes / 8 GiB PASS**. 실제 Windows 테스트별 경과는 703/2345/705/393ms다. recovery 프로세스의 sampled working set peak는 25,161,728 bytes, 파일 교체 테스트는 7,315,456 bytes이며 너무 빨리 끝난 나머지 두 프로세스는 sample을 얻지 못했다. Linux wrapper의 RSS를 Windows 메모리로 보고하지 않는다.
- `1ad2e1d`의 초기 [WSL 실행 근거](https://github.com/jihoon22-lee/devbox/actions/runs/34387287195)는 **0.30초 FAIL(`wsl_backing_path_unsafe`)**로 구간을 특정했다. [Microsoft WSL 소스](https://github.com/microsoft/WSL/blob/03f6b0e5dd8bdbcb90406813699f616534a25eb3/src/windows/service/exe/LxssUserSession.cpp#L1049)를 확인한 결과 Registry `Version`은 filesystem format이고 WSL1/2는 `Flags`의 VM_MODE(0x8)로 구분한다. 기존 코드는 현대 WSL1의 format=2를 WSL2로 오인해 존재하지 않는 `ext4.vhdx`를 검사했다. 이전 storage artifact의 `version:1`은 보고서 schema 값이며 실제 mode 측정값이 아니었다.
- mode를 Flags로 판별하고 filesystem version과 모든 registry 값 검증은 유지했다. Windows fixture는 format=2/Flags=7의 WSL1과 Flags=15의 WSL2 backing 경로를 모두 검사한다. 초기 storage artifact에도 실제 filesystemVersion/flags/wslVersion을 구분해 기록한다. WSL2 backing 검사나 identity 조건을 생략하지 않는다.
- 모드 판별 수정의 실제 Windows Registry 회귀와 PowerShell fixture parser PASS(Windows test 0.01초, 실행 전체 703ms). MSVC unit 빌드 **25.673초 / peak 4,361,244,672 bytes**, 전체 MSVC strict Clippy **5.701초 / peak 1,558,781,952 bytes PASS**. 실제 WSL 시작 여부는 새 CI에서 확인한다.
- `1ad2e1d` packaged journal trace는 UI preview→cancel→다시 preview→apply까지 모두 성공했고 원본 복구 확인을 통과했다. 이후 프로젝트 선택 해제가 active context reader와 겹쳐 admission 단계에서 거부됐다. 선택 변경은 먼저 caller/replay를 확인하고 한 개의 bounded waiter가 기존 worker의 최종 종료를 기다리게 했다. 획득 후 context/origin/deadline/shutdown을 다시 확인하고 effect를 한 번만 수행한다. session의 짧은 순수 metadata 구간도 정상 lock 대기로 처리해 describe와의 경합을 실패로 만들지 않는다.
- context permit 3개·native component 경계 9개와 Linux/전체 MSVC strict Clippy PASS(**49.499초 / peak 3,127,840,768 bytes / 8 GiB**). 선택/해제 요청도 기존 native 30초 상한 안에서 대기/검증하도록 frontend deadline을 맞췄다. 최종 전체 영향 범위 검증을 다시 수행한다.
- 최종 `pnpm verify:affected` all PASS(**508.270초 / sampled RSS peak 6,666,821,632 bytes, cgroup peak 6,445,342,720 bytes / 8 GiB**). 초기 bundle **278,778/280,000 bytes**, gzip **82,235/90,000 bytes**. 새로운 실제 WSL 시작과 packaged 선택 해제 수용은 다음 CI에서 확인한다.
