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
