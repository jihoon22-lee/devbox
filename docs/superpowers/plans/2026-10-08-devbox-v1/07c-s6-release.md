# S6 마무리·출시 실행 계획

- 작성: 2026-10-25. 기준 `origin/main@867b414e14af9db8a6c75019befbe6ef80e250d8`.
- 현재 승인 범위: **2026-10-25 사용자 “S6 계획대로 작업 진행 해 주세요”에 따라 S6 구현을 승인했다.** 앞선 계획만 작성 제한은 대체되었으며, 과제별 구현·좁은 검증·별도 검토 순서로 진행한다. 현재 S6-1a.1–4·1b.1–2는 로컬 완료했다. 사용자2026-10-25 토큰 복구 후 재개 지시로 앞선 중단·인계 제한을 대체했고1b.3도 직접 검증·독립 APPROVE 뒤 로컬 완료했다.1b.4도 고유18 직접 검증 PASS·독립 APPROVE 뒤 로컬 완료했으며 공개·태그는 §16의 별도 명시 승인 전 실행하지 않는다. 이후 S6-1c·S6-2·S6-3a/3b·S6-4a/4b도 로컬 완료했고(Windows 실기 NOT_RUN), S6 PR 자동 리뷰는 2026-10-26 사용자 결정으로 독립 검토로 대체한다(§16).
- 작업 위치: `v1/s6-release`, `/home/jihoon/projects/devbox-wt/v1-s6-release`. 진행 원장은 [PROGRESS](PROGRESS.md).
- 선행: S3 #644, S4 #646, S5 #645 및 정리 문서가 main에 통합됨. 과거 설치 시험의 PASS·FAIL·NOT_RUN은 소스 SHA별로 유지한다.
- 실행 방식: 사용 가능한 하네스의 **과제별 구현 → 다른 에이전트 검토**. 별도 planning 스킬이나 특정 모델을 요구하지 않는다.
- 이 문서는 [07](07-s4-s6.md)의 S6 요약을 실제 코드에 맞게 구체화한다. 계획 커밋563523db 뒤 사용자 구현 지시를 확인하고 S6-1a.1을 시작했다.

## 1. 먼저 고칠 네 가지 발견

사전 검토 근거는 Git common-dir `devbox-archive/pre-s6-review-20261025/`의 `summary.json`, `s0-s2-review.json`, `s3-s4-review.json`, `s5-review.json`, `root-evidence-review.json`이다. 모두 위 main 기준이며 제품 수정 없이 좁은 소유 fixture로 확인했다. 사전 검토 당시 아래 항목은 미수정이며, 현재 과제별 결과는 PROGRESS를 따른다.

| ID | 실제 재현·한계 | S6 처리 |
|---|---|---|
| S5-ENV-LATE-LOAD · P1 | 환경 A 지연 읽기 → B 선택·편집 → A 완료가 B 미저장 입력을 덮고 현재 환경도 A로 바꿈. 실제 EnvEditor 재현 | S6-1a.1: 읽기 세대와 초안 revision을 함께 검사 |
| S3S4-LSP-STALE-UNOPENED-TARGET · P2 | Rename 요청 당시 닫힌 B를 외부/새 편집기로 바꾼 뒤 unversioned edit가 오래된 offset으로 다른 식별자를 수정. 일반 저장으로 잘못된 내용도 저장됨 | S6-1a.2: 모든 대상의 요청 시점 출처 증명. 정상 미개방 파일 간 Rename도 유지 |
| S0S2-WINDOWS-COPY-IDENTITY · P2 | 서로 다른 등록 프로젝트의 basename이 같으면 동일 Windows 사본을 덮음. 실제 graph+sync, Linux 소유 임시 폴더에서 재현; Windows 명령 실행은 하지 않음 | S6-1a.3: 프로젝트·worktree별 사본 소유권. 기존 root+이름 설계 자체를 정정 |
| S5-LITERAL-AUTH-ECHO · P2 | 직접 Authorization 헤더의 credential echo가 응답 stream/history에 남음. 같은 값의 구조화 Bearer는 가림. 임의 literal JSON까지 secret이라는 주장은 아님 | S6-1a.4: 인식 가능한 credential 헤더·cookie의 명시적 가림 계약 |

네 항목을 기능 연결보다 먼저 닫는다. LSP 재현의 “4 tests pass”는 결함 동작 assertion 2개+안전 제어 2개이며 제품 수용 PASS가 아니다. Windows 사본도 실제 Windows 수용을 대신하지 않는다.

## 2. 작업 순서·공통 완료 조건

| 순서 | 과제 | 의존 | 로컬 커밋 checkpoint |
|---|---|---|---|
| 상세화 | S6-1 계획 | main·사전 검토 | `docs(plan): detail S6 release work and review fixes` |
| AF-1 | S6-1a.1–4 사전 결함 수정 | 계획 실행 지시 | 네 수정 각각 독립 커밋 |
| AF-2 | S6-1b.1–4 기능 연결·기존 후속 | 네 수정 | 연결마다 독립 커밋 |
| AF-3 | S6-1c 의존성 보안 업데이트 | 사전 조사·좁은 회귀 | 의존성별 독립 커밋 |
| AF-4 | S6-2 릴리스·검증 비용 정리 | 위 제품 계약 안정화 | 예산 정리 → 릴리스 job |
| AG | S6-3 업데이트·종료 경계 | S6-2 자산 계약 | core → Windows/UI 통합 |
| AH-1 | S6-4 계측·Windows 수용 준비 | 제품 구현 완료 | 계측·최소 원인 수정 |
| AH-2 | S6-5 문서·S6-6a 버전 | 제품 계약·계측 고정 | 문서·한 번의 1.0.0 bump |
| AH-3 | S6-6b 최종 검사·PR·머지 | 전체 브랜치 검토 | 최종 결과·수정 커밋 |
| 공개 | S6-6c 공개 승인·태그 | main CI·수용 근거·명시 승인 | `v1.0.0` 주석 태그 |

S6는 한 브랜치·한 PR이다. 계획 커밋은 로컬 완료했고 현재 좁은 과제 단계에서는 push/PR을 만들지 않는다. 구현 시에도 과제마다 전체 검사를 반복하거나 중간 PR을 열지 않는다. 마지막 Windows 수용 artifact를 위한 candidate 백업 push/dispatch는 §16의 명시 순서로 PR 전에 수행한다.

각 구현 과제의 동일한 5단계:

1. 해당 파일·현행 계약을 읽고 좁은 실패 시험을 먼저 실행한다. 재현 fixture는 실제 문제가 생긴 경계에 둔다.
2. 최소 구현과 typed 오류·한국어 행동 안내를 함께 만든다. RPC는 `method!`/`topic!`에서 생성하고 생성 TS를 손으로 고치지 않는다.
3. RED→GREEN 및 직접 영향 시험만 실행하고 명령·선택된 시험 수·출처 SHA·자원 결과를 남긴다. 0개 선택·exit75·setup 실패를 시험 성공으로 세지 않는다.
4. 다른 에이전트가 코드·계약·시험 증거를 검토한다. 지적 수정 뒤 영향 범위만 다시 검사한다.
5. `PROGRESS` 갱신 → `pnpm exec biome ci .` → 영어 Conventional Commit. 부모가 커밋한다. 후속 과제는 검토가 끝난 계약을 사용한다.

자원 제한은 CONVENTIONS §5 그대로다. Rust 전 `source ~/.cargo/env`; jobs/workers/threads=2, CPU4·nice+10, 지원 cgroup high6GiB/max8GiB/swap1GiB. 이 환경은 child의 `XDG_RUNTIME_DIR=/run/user/1000`, `DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus`, `DEVBOX_VERIFY_CGROUP=required`를 적용해 제한이 실제 켜졌음을 확인한다. 무거운 검사는 common-dir 잠금 하나, exit75는 조율 후 재개한다. 기존 서비스·Docker·공유 네트워크·prod·사용자 자료를 시험으로 바꾸지 않는다.

파일600줄/컴포넌트300줄을 유지하되 범용 새 프레임워크를 만들지 않는다. 제품/테스트 예산과 `.github`+`scripts` 예산은 별도다. 새 의존성이 필요하면 기존 스택 대안·배포 영향을 먼저 확인한다.

## 3. S6-1a.1 — 환경 늦은 읽기가 새 입력을 덮지 않기

**Files:** `app/src/features/tools/api/EnvEditor.tsx`, 인접 EnvEditor 시험. `useApiTabs` 발행 경로는 필요한 최소 변경만 한다.

**계약:** load 시작 시 `{root, selection, operationGeneration, draftRevision}`을 캡처한다. 새로운 선택·생성·root 전환·unmount는 이전 요청을 무효화한다. 편집도 draftRevision을 바꾸므로 같은 선택의 늦은 읽기가 새 입력을 덮지 못한다. guarded 확인은 시작 전 사용자 의사 확인이며 완료 시 ownership 검사를 대체하지 않는다. 현재 소유 요청만 editing/dirty/선택/환경 발행을 함께 갱신한다. 거절된 응답은 toast나 이전 오류로 새 화면을 덮지 않는다.

- [x] RED: 보존된 재현을 `EnvEditor.loads.test.tsx`로 옮겨 A 지연→B 로드·`/unsaved` 편집→A 완료가 입력을 덮는 실패1을 확인했다.
- [x] 같은 환경 재읽기 중 입력·새 환경 생성·root 전환·실패 역순·unmount·빈 선택·저장 완료/저장 중 입력의 ownership 경계를 시험했다.
- [x] GREEN: 환경 편집기3파일19 PASS, 추가 root 확인 정리1·환경 검토 gate1 PASS로 고유21. B 입력·dirty·선택·발행 환경 유지와 기존 정상 로드/명시적 버림·루트별 저장·검토 ACK를 확인했다. 별도 검토 APPROVE, 완료 Biome 뒤 로컬 과제 커밋으로 보존한다.
- [x] 검토: 독립 검토 지적0·APPROVE. 저장/review ACK 보존과 새 read invalidation 계약을 확인했다. 근거 `s6-1a1/independent-review.json`.
- [x] 커밋: `fix(api): preserve environment drafts across late reads`(로컬).

## 4. S6-1a.2 — LSP WorkspaceEdit 모든 대상의 출처 증명

**Files:** `app/src/features/source/editor/{lspCommands,lspWorkspace,lspFiles,lspSession}.ts`, 인접 시험; `crates/lsp/src/{relay,relay_documents,uri,actor,adapters,model}.rs`와 relay 시험; 필요 최소 `crates/documents/src/{code_authority,sys}.rs`의 bounded reader, `crates/cli/src/daemon/lsp.rs`. 단순 파일 길이 분할은 경계 책임에 따른다.

**구체적 전략:** 열린 문서의 snapshot을 유지하고, unversioned multi-file Rename에 **요청 전 데몬이 캡처한 covered-target baseline 집합**을 더한다. 원자적인 전체 프로젝트 snapshot이라고 주장하지 않는다.

- 별도 취소 가능한 job이 canonical project/worktree code authority 안에서 후보를 순회한다. 실제 relay actor의 `client` 처리 안에서 전체 walk/read를 await하지 않아 다른 공유 LSP 메시지를 막지 않는다. 기존 `FilesService.tree`는 per-directory 반복 scan이며 `documents::sys::read_at(limit)`는 EOF까지 hash하므로 그대로 bounded snapshot이라고 재사용하지 않는다.
- 초기 예산: 파일4,096개, 파일당 기존 CODE_LIMIT5MiB, 총 실제 읽기32MiB, cooperative deadline2초, 순회 entry16,384개, metadata4MiB, client당 pending job1개/daemon 전체4개, token TTL30초. 읽기·열거·hash 중 매 chunk마다 취소/예산을 검사하고 기존 WorkspaceEdit target256개 상한도 유지한다. worker가 실제 끝날 때까지 job accounting을 유지한다. spawn_blocking future를 drop했다고 읽기가 멈췄다고 주장하지 않는다. 캡처5MiB 허용이 기존 LSP packet2MiB admission을 늘리지는 않는다. 숫자는 구현 시 소유 fixture 측정으로 검토할 초기값이며 측정 PASS를 미리 주장하지 않는다.
- token은 실제 stream/client/server generation·canonical root·Rename request ID에 결속한다. explicit covered keys·미포함 이유를 보관하고 cancel/disconnect/TTL 또는 최종 적용 판정 뒤 해제한다. 응답 도착만으로 유일한 provenance를 지우지 않는다. 비동기 claim/read를 포함한 최종 target 검증·apply ACK 또는 cancel까지 daemon token을 유지한 뒤 한 번만 consume한다. renderer에는 daemon-owned immutable typed baseline을 정확한 pending request에 결속해 전달하며 server가 위조한 proof나 응답 시점 새 캡처를 승인하지 않는다. bounded ephemeral 메모리이며 영구 파일 index를 만들지 않는다.
- 요청 시 이미 열린 파일은 renderer `{lease, editSeq, text hash}`와 relay가 실제 동기화한 text/server version·raw diskRev를 함께 캡처한다. **기존 dirty buffer는 유효 입력**이므로 disk text와 같아야 한다고 강제하지 않는다. server dispatch 직전에 세대와 열린 버퍼 snapshot이 변하지 않았음을 확인한다.
- 당시 미개방 파일은 canonical identity·raw DiskRev와 **정규화 text hash**를 따로 캡처한다(CRLF/BOM 차이). 요청 전 job이 확보한 값만 baseline이며 응답 뒤 claim/read한 현재 diskRev로 대체하지 않는다. 서버의 동기화된 문서 상태가 별도로 있으면 그 상태와 모순된 disk baseline을 승인하지 않는다. 이 baseline은 재현된 요청 대기 중 변경을 막는 계약이며 임의 언어 서버의 의미적 정확성이나 파일시스템 전체 원자성을 증명하지 않는다.
- unversioned 응답의 모든 target은 covered 집합에 있어야 한다. 적용 직전 diskRev 및 현재 buffer를 다시 검사한다. 요청 뒤 새로 열린/dirty인 target도 당시 normalized baseline과 비교한다. 이미 열린 target은 당시 buffer와 비교한다. 전 대상 검증 후 적용하며 하나라도 변했거나 출처가 없으면 전체 edit 거절·기존 버퍼/저널 유지.
- 정상 미개방 B의 multi-file Rename은 성공해야 한다. 미포함/예산 초과는 해당 범위를 안전하게 확인하지 못했다는 행동 안내로 거절한다. 이전 응답의 canonical target URI를 제한된 힌트로 **다음 요청 캡처에서 우선**할 수 있다. 재시도는 fresh capture 후 서버에 Rename을 새로 요청하며 옛 edit를 재생하지 않는다. target 집합 자체가 예산을 넘으면 범위를 줄여 요청하거나 수동 검토하도록 명시한다. 모든 미개방 파일을 조용히 금지하지 않는다.
- versioned edit는 기존 relay version 매핑과 canonical authority를 유지한다. unsolicited `workspace/applyEdit`에는 최근 Rename token을 임의 부착하지 않는다. 정확한 versioned owned-document proof가 없으면 `applied:false`로 거절한다. 실패가 공유 server generation을 불필요하게 죽이지 않는다.

- [x] RED: 보존된 두 재현을 올바른 내용 유지 기대로 반전해 실제2 FAIL을 확인했다. bounded reader1·취소 완료 queue1·저장 ACK rawRev1 및 독립 검토 R1 omission1의 추가 causal RED도 각각 기록했다.
- [x] GREEN: 문서 권한12·frontend46·LSP41·실제 CLI 어댑터2로 고유101 PASS(여러 직접 영향 실행의 합). 정상 미개방/dirty-open·stale/부분/삭제/identity 교체·버전/세대/소유/TTL/replay·BOM/CRLF와 UTF16 offset·실제 read budget·capture 응답성/취소 accounting을 확인했다. 초기 fixture/TS 설정 실패와 causal RED는 `s6-1a2/summary.json`에서 구분한다.
- [x] 검토: 요청 전 출처와 응답 후 권한 확인을 구분하고 aggregate 비용을 점검했다. P3 omission 보완은 RED1→좁은2 PASS, 문서의 과거 수치 치환도 원문 복원했다. `s6-1a2/independent-rereview.json` 최종 APPROVE·잔여지적0.

구현 상세: 반환 URI는 요청 전 집합의 canonical key와 정확히 맞아야 하며, 응답 시 alias를 새 canonical 파일로 해석하지 않는다. 누락 target URI는 최대256개 힌트로 다음 명시적 요청의 fresh capture에서 우선하고 이전 edit는 재생하지 않는다. 기존 definition URI 정규화는 유지한다. 일반 응답 authority 실패도 요청 오류로 거절하며 공유 generation을 죽이지 않는다. `didSave`는 raw revision만 갱신해 저장 뒤 Rename과 더 최신 dirty buffer를 모두 유지한다. relay의 synchronized buffer와 모순되어 제외한 canonical disk 후보도 동일 metadata 예산 안에서 `Changed` 사유를 보관하며 기록 공간 부족은 `MetadataLimit`으로 남긴다.

비용 구분: capture의 기본32MiB 실제 읽기/2초 cooperative 경계는 소유40MiB fixture에서1,596ms·32MiB·6 covered로 측정했다. Rename 응답 재정규화/renderer 중복 disk read는 제거했다. 최종 daemon authority는 raw size 합32MiB·각 파일5MiB·target256·2초 cooperative deadline/target별 TTL 검사로 제한하고 첫 불일치에 즉시 중단한다. 성공 읽기는≤32MiB, 변경된 첫 파일이 커졌을 때는≤37MiB다. 이미 시작한 filesystem syscall을 강제로 끊거나 전체 작업 wall time≤2초라고 주장하지 않는다. 최초 미개방 대상의 `documents.claim`과 proof 없는 기존 formatting/unsolicited read는 일반 document/CAS 경로이므로 새 capture IO 상한과 별도다. 기존 스택의 `devbox-documents`·`sha2` workspace 의존 edge만 추가했고 새 패키지/버전은 없다.

- [ ] 실제 언어 서버 Rename 수용은 S6-4에 둔다. fake RPC/CodeMirror 시험을 native PASS로 바꾸지 않는다.
- [x] 커밋: `fix(lsp): bind workspace edits to target snapshots`(로컬).

## 5. S6-1a.3 — Windows 사본을 프로젝트·worktree 소유권에 결속

**Files:** runtime의 graph/execution·copy identity/store/service·observation/reconcile, protocol의 필수 observation DTO·고정 bootstrap, CLI direct/native broker와 native read-only probe·supervisor, 프로젝트 삭제 fence, typed 오류·한국어 안내 및 인접 시험. `01-design.md` 대상 주석·`05-s2-runs.md` Task12 계약도 함께 정정했다.

**결정:** 기존 `[windows] root` 설정 자체는 유지하되 `<root>\\<project name>`를 실행 사본으로 재사용하지 않는다. 새 전용 namespace 아래 **안정 project ID + 실제 worktree identity**를 경로에 사용한다. 표시명은 UI 표시용이며 identity가 아니다. main은 별도 고정 identity, agent worktree는 소유 resolver의 canonical 경로·등록 task/worktree ID에 결속한다. 대소문자/금지 문자/길이 제한에 안전한 ID 구성만 쓴다.

- 새 사본에 schema·project/worktree identity·canonical source를 담은 소유 marker를 배타적으로 만든다. source 파일이 marker를 덮을 수 없는 예약 이름/위치를 사용한다.
- 이미 존재하는 비어 있지 않은 미소유 디렉터리·다른 identity marker·symlink/reparse 경로는 쓰지 않는다. root 아래 새 namespace도 임의 기존 사용자 자료를 소유로 인수하지 않는다.
- **기존 basename 사본은 자동 이동·복사·삭제·정리하지 않는다.** 이전 `node_modules`·산출물·수동 수정은 그대로 남는다. 새 사본 사용 안내와 기존 사본 수동 정리 선택지만 문서화하고 실행하지 않는다. 새 사본에도 기존 deleted=0 정책 유지.
- **배타적인 sync writer와 공유 execution read lease를 분리한다.** 동일 승인 graph/group의 첫 Windows consumer 직전에 coherent copy revision을 한 번 준비한다. 입력 파일을 만드는 선행 WSL prerequisite가 끝나기 전에 미리 고정하지 않는다. Windows service→dependent task·동일 revision의 sibling은 reader로 합류하며 다시 sync하지 않는다.
- compatible reader는 같은 canonical project/worktree identity와 고정 source/copy revision을 사용한다. 다른 source revision이 필요한 시작이나 나중 graph node는 기존 reader가 있는 사본을 바꾸지 않고 명시적 busy/reconcile 안내로 끝낸다. 같은 graph라는 이유로 바뀐 source를 무조건 호환으로 인정하지 않는다. 서로 다른 project/worktree는 독립 실행한다.
- reader별 launch/실제 종료 근거를 추적한다. **마지막 consumer의 종료가 확인된 뒤에만** writer를 다시 허용한다. 한 consumer 완료/취소는 다른 service reader를 해제하지 않는다. launch 전 취소는 자기 pending lease만 정리한다. launch 후 RPC 오류·timeout·native 연결 취소로 Windows child/후손의 종료를 증명하지 못하면 unknown reader를 유지하고 소유 프로세스 확인/명시 reconciliation까지 busy로 둔다. 오류를 종료로 간주하거나 암묵적으로 stop하지 않는다.
- 직접 interop/native fallback 모두 동일 PreparedCopy 계약을 쓰고 fallback에서 legacy target으로 돌아가지 않는다.

- [x] RED: 실제 graph+sync 동일 basename 두 프로젝트에서 A bytes 보존 기대가 실패1→서로 다른 대상 GREEN. 같은 크기/mtime 입력 변경도 revision에 반영한다.
- [x] main/worktree·instance/distro 분리, canonical source 교체·marker 누락/불일치/링크, legacy bytes·Windows 산출물 보존을 소유 fixture에서 확인했다. 동시 namespace marker 초기화 RED1→atomic immutable publication GREEN도 추가했다.
- [x] GREEN: service/dependent·siblings가 같은 revision을 공유하고 source 변경·남은 reader는 writer를 막는다. 마지막 증명 뒤 재실행, WSL prerequisite 뒤 첫 capture, 실제 publisher 종료까지 취소 accounting, 다른 사본 독립성을 확인했다.
- [x] launch 이전 취소·dead owner 복구·dispatch 경계·자동 restart gap·이전 unknown attempt 보존·project delete fence를 확인했다. direct 세션 불일치를 원래 App0 세션으로 조회하는 실제 CLI composition도 소유 fake process/broker에서 확인했다.
- [ ] 실제 Windows 경로/Job/PowerShell/다중 consumer 수용은 S6-4이며 Linux fixture나 structural bootstrap 검사를 실기 PASS로 세지 않는다. 기존 Windows interop port 시험1은 ignored/NOT_RUN.
- [x] 과제 전체 독립 검토 APPROVE·지적0. protocol/helper adapter 부분 검토도 APPROVE이며 기존 root+이름 설계를 이번에 정정한 사실을 보존한다. 근거 `s6-1a3/independent-review.json`.
- [x] 커밋: `fix(runtime): isolate Windows copies by project and worktree`(로컬).

**현재 구현 계약:** 사본 `files`와 marker/control을 분리하고 owner hash에 instance/channel/distro/data를 결속한다. git 목록8MiB/65536파일·64KiB chunk의 content SHA256과 source 전후 검증을 사용한다. durable reader는 Reserved→Dispatching→Submitted→Sealed→Closed이며 새로운 wrapper attempt마다 unpredictable Local Job·nonce를 생성한다. 고정 bootstrap은 사용자 cwd/env/command 이전에 non-killing Job에 들어가 private Assigned(PID/FILETIME/SID/session/run/attempt)를 발행하고 정확한 Grant를 기다린다. runtime은 DB commit 후에만 immutable Grant를 내고 DB writer 내부에서도 cancel/deadline을 재확인한다. committed-but-unpublished grant는 재발행하지 않는다. user stdout/일반 exit/WSL inactive는 reader 해제 증명이 아니다.

명시 stop/restart는 새로운 attempt를 seal하며 unknown을 자동 종료하지 않는다. read-only Job probe의 정확한 Empty/NotFound만 해당 committed attempt를 닫는다. 서비스 restart 간격·오래된 unknown은 logical reader를 유지한다. direct와 native는 같은 필수 제어를 사용하고 direct SID/session 불일치·미가용은 동일 Assigned를 원래 App0에 조회한다. 별도 bounded probe slot4·경로1024개/512KiB batch와 응답 cap을 적용했다. private control 파일을 OS sandbox라고 부르지 않으며 실제 Job/PInvoke 동작 검증은 남아 있다.

검증: 직접 관련 고유94 PASS(runtime42·CLI composition1·프로젝트1·메시지2·helper48), incidental 기존2 PASS를 분리했다. 중복 재실행/모듈 이동3개는 재집계하지 않았다. basename/marker/필수 observation의 causal RED3 및 변경된 preview 계약 기대1 FAIL을 원본으로 보존한다. `s6-1a3/` source·commands·resources·test-cases가 근거이며 실제 memory_enforced=true다. scoped TS·메시지 Biome PASS, 전체 독립 검토 APPROVE. 완료 Biome 뒤 로컬 커밋으로 보존하며 실제 Windows 빌드/실기·전체 check/clippy/build/E2E·push/PR은 미실행이다. 기존 workspace sha2 edge 하나만 추가했고 패키지/버전 변경은 없다.

## 6. S6-1a.4 — 직접 입력 credential 헤더·cookie echo 가림

**Files:** `crates/api/src/{auth,redact,service,history}.rs`, `http/{mod,build,stream}.rs`, `crates/api/tests/` 응답/history 시험. protocol마다 실제 헤더 입력을 받는 경계만 공유한다.

**명시 계약:** 구조화 인증·secret 참조·sensitive 변수/capture에 더하여 다음 **인식 가능한 입력**의 값을 request별 zeroizing redaction seed로 취급한다.

- 대소문자 무관 `Authorization`, `Proxy-Authorization`: 전체 값과 Bearer/Basic 등 credential 부분. 인식 불가능한 scheme도 header 값 자체는 가린다.
- 명시 목록 `X-API-Key`, `API-Key`, `X-Auth-Token` 같은 credential header 이름. 목록은 테스트·문서에서 동일하게 정의하며 임의 이름의 일반 header를 추측으로 모두 secret 취급하지 않는다.
- request raw `Cookie` header·구조화 cookie rows·**그 hop에서 실제 선택되어 송신되는 jar cookie values**를 모두 포함한다. Cookie row/jar 조립이 auth 준비보다 늦으므로 최종 outgoing header 조립 후, 각 redirect hop의 응답을 내보내기 전에 값을 seed에 추가한다. 전체 jar나 다른 origin의 cookie를 불필요하게 resolve/수집하지 않는다.
- **새 계약:** response `Set-Cookie`는 header 이름으로 표시/history 값을 가리고, 새로 발급된 cookie value도 operation seed에 추가한 뒤 그 response head/body/history를 방출한다. redirect hop의 Set-Cookie도 같은 순서다. 기존 stream은 값 seed만 적용하므로 이 기능을 이미 있다고 쓰지 않는다.
- response cookie seed 추가는 새 credential을 body로 echo한 경우까지 같은 privacy boundary로 보호하기 위한 명시적 확장이다. cookie 이름/Expires·Path·SameSite 등 속성을 credential 값과 혼동하지 않는다. 일반 서버 JSON field를 자동 수집하는 확장은 아니다.
- 기존 redactor에는 합산 예산이 없었으므로 operation별 새 상한을 raw seed 1,024개·256KiB, 중복 제거한 표현 변형 4,096개·1MiB로 고정했다. 변형 길이를 먼저 확인한 뒤 할당하며 raw·form URL·percent URL·JSON escaped byte 표현을 지원한다. 초과하면 typed 실패로 응답 relay/history 기록을 차단하며 raw fallback은 없다. 빈 값은 matching seed로 넣지 않되 credential header 표시 가림은 유지하고, 짧은 nonempty 값은 제외하지 않는다.
- streaming chunk 경계·문자 encoding/표현 변형은 기존 sanitizer의 지원 계약을 유지하며 decoded body도 검사한다. seed 확정 전 response bytes를 이미 공개한 뒤 뒤늦게 가리는 구현은 하지 않는다.
- 로그에는 본문/헤더 값을 추가하지 않는다. stream·history 본문·snapshot·후속 노트 export에서 같은 정책을 쓴다. 짧은 credential도 제외하지 않고 과잉 가림 가능성을 명시한다.
- **임의 literal JSON/body의 문자열을 전부 secret으로 가리는 확장은 하지 않는다.** 기존 일반 응답 가독성·literal echo 시험은 유지한다.

- [x] RED: 보존된 owned localhost echo fixture로 구조화 Bearer 제어와 직접 Authorization 실패를 정식 회귀로 만든다.
- [x] GREEN: 대소문자·Proxy-Authorization·API key, raw Cookie/row/실제 선택 jar echo, 요청 seed에 없던 서로 다른 신규 Set-Cookie value의 header/body/history 가림, redirect hop·chunk split·빈/짧은 값·capacity 초과 fail-closed·history_bytes 디코딩을 확인한다. 무관한 일반 literal JSON/body와 비민감 header는 그대로 보존한다.
- 구현 경계: raw Cookie는 개별 값(quoted 값 포함)을 수집하고 이름·속성은 seed로 넣지 않는다. 직접 헤더·cookie row는 OAuth 실패/취소 전 prepare에서도 수집하며, jar는 실제 outgoing hop에 선택된 값만 수집한다. 최신 response cookie로 이전 redirect 목록도 공개 전에 다시 가린다.
- WS handshake·SSE 각 재연결·gRPC 실제 요청/응답 metadata·MCP HTTP 헤더에도 같은 fallible 정책을 적용했다. gRPC reflection은 기존 첫 응답 후 drop을 유지하고 읽지 않은 정상 trailers는 수집했다고 주장하지 않는다. credential이 catalog dispatch 식별자에 섞이면 이름을 바꾸지 않고 고정 typed 오류로 거절한다. 미래 trailer 값으로 이미 보낸 streaming message를 소급 가리지는 않는다. MCP stdio 환경 이름을 HTTP 헤더로 추론하지 않는다.
- raw seed·임시 표현·Basic decode·stream 보류 버퍼는 operation 수명에 묶인 zeroizing 소유다. Aho-Corasick 내부 할당의 zeroization을 보장한다고 쓰지 않는다. typed `api.redaction_failed`는 raw fallback 없이 처리하고 HTTP는 해당 작업의 history 전체를 쓰지 않는다. 실패 사유별 한국어 메시지와 생성 RPC 타입을 반영했다.
- [x] 검토: 실제 credential 수집 시점·공개/기록 경계·operation 수명을 확인한 독립 전체 검토 APPROVE·잔여지적0. 직접 관련74 PASS·incidental1 PASS와 원본 실패·미실행을 구분했다. 근거 `s6-1a4/independent-review.json`.
- [x] 커밋: `fix(api): redact recognized credential header and cookie echoes`(로컬).

## 7. S6-1b.1 — API 응답을 일일 기록·선택 노트에 붙이기

**Files:** `app/src/features/tools/api/{ResponseView,ResponseOrigin,backend,sendState}.*`, 응답 수집 adapter; `crates/api/src/` 응답 provenance/export·history 조회; `crates/notes/src/{model,daily_ops,lib}.rs`; `crates/cli/src/daemon/` API–Notes 연결; `app/src/features/notes/rewrite.ts`; 관련 generated RPC/오류 문구. 새 작은 모듈은 기존 서비스 경계 안에 둔다.

**현행 차이:** `notes.append` target은 Inbox/Daily뿐이다. `validate_command_text`의64KiB 검사는 노트 응답1MiB 요구와 맞지 않는다. `ResponseOriginInfo.url`은 renderer가 캡처한 초안 문자열이므로 안전한 export metadata의 원장으로 쓰지 않는다. NOTE_LIMIT=2MiB와 기존 CAS/dirty rewrite 경로는 유지한다.

**계약:**

- [노트에 붙이기]는 **일일 기록 / 고른 노트**를 선택한다. 고른 노트는 현재 vault 내 기존 note identity를 받으며 임의 OS 절대 경로·다른 vault·코드 문서를 쓰지 못한다. Inbox 기존 CLI 동작도 유지한다.
- typed append target에 note path/identity를 추가하고 현재 vault canonical authority로 검증한다. 삭제/rename/설정 변경 중 대상이 바뀌면 재선택 안내로 거절한다.
- 기존 `SendEvent::Started.history_id`→`SentResponse.historyId`를 사용한다. CLI orchestration이 typed `{historyId, target}`을 받아 `ApiService.history_get`의 HistoryEntry(method, 치환/가림 후 URL, sent_at_ms, response/body)를 읽어 NotesService에 넘긴다. notes crate가 api crate에 의존하게 하지 않는다. 데몬에서 완료 응답의 request/send ID·출처 root·history identity를 결속해 export한다. renderer가 임의 method/URL/body를 안전한 응답인 것처럼 제출하지 않는다. 개인/프로젝트 origin 권한도 재검증한다.
- history 아직 미기록/실패·읽기 불가·본문 truncated를 명확히 처리하고 재송신 fallback은 하지 않는다. UI 수신 완료만으로 durable history가 있다고 가정하지 않는다.
- method·**실제 전송의 가려진 URL**·status·기록된 전송 시각 및 가려진 본문으로 Markdown 블록을 만든다. 늦은 A 응답 아래 현재 B 초안 URL을 붙이지 않는다. URL query/userinfo와 header/cookie echo는 S6-1a.4 sanitizer 경로를 사용한다.
- 본문은 UTF-8 기준 **최대1MiB**, scalar 경계에서 자르고 잘림을 표시한다. 현재 history_get의 byte cap이 마지막 UTF-8 scalar를 잘랐다면 불완전한 끝 bytes를 제외하고 잘림을 알린다. binary는 크기/종류 안내만 붙이고 임의 decoding하지 않는다. body 안의 backtick이 블록을 닫지 못하도록 충분한 fence를 선택하되 fence/metadata overhead도 별도 상한을 둔다(예:16KiB).
- 노트 전용 append admission은 최대1MiB 본문+제한된 wrapper를 허용하고 NUL·invalid input을 거절한다. 공용 command64KiB 한도를 올리지 않는다. 최종 note의 BOM/CRLF 포함2MiB 상한과 rewrite RPC 크기 예산을 검사한다. 16MiB frame hard cap을 핑계로 무제한 문자열을 보내지 않는다.
- `daily_ops::edit_note`의 CAS retry·dirty lease holder rewrite/ACK·disk 확인을 재사용한다. dirty 버퍼 뒤에서 파일을 직접 쓰지 않는다. 복구 초안만 있고 holder 없음/응답 timeout/외부 변경은 보존·typed 실패.
- 중복 클릭은 operation identity로 한 번만 append하고, 성공 후 목록 조회 실패로 다시 붙이지 않는다. 사용자 새 클릭으로 같은 응답을 다시 붙이는 의도와 retry를 구별한다.

- [x] RED: 기존 Notes 경로64KiB+1 거절·선택 target 미지원2 FAIL, API 새 미구현 export seam의 unavailable1 FAIL 뒤 구현했다. 원본 로그와 stub 성격을 구분해 보존한다.
- [x] GREEN: 일일/선택 노트,64KiB+·1MiB/본문+wrapper admission·scalar/fence/binary·2MiB/BOMCRLF·target 위조/삭제/교체·dirty preflight·CAS 외부 변경·late ACK/timeout/restart·refresh 실패 retry 중복 없음. 총 고유71 PASS(직접69·incidental 메시지2; API30·Notes19·CLI3·frontend19).
- [x] UI: 완료된 원래 historyId에서 대상 선택, sourceRoot(미저장 프로젝트 포함) 캡처·typed pending/error/성공 연결. 응답/root 교체와 중복 클릭/같은 intent 재확인을 직접 시험했다.
- [x] 최종 E2E의 요청→응답 경로에 durable history 대기→일일 기록 append와 HTTP 재송신0 assertion을 추가했다. 현재 E2E 실행은 NOT_RUN; 선택/dirty는 좁은 통합 시험으로 확인했다.
- [x] 제품·시험46파일+문서2파일의 전체 독립 검토 APPROVE·지적0. API 부분15파일 검토 뒤 최종 동결·근거를 대조했다. 완료 Biome 뒤 로컬 과제 커밋으로 보존한다.
- [x] 커밋: `feat(api): append sanitized responses to daily and selected notes`(로컬).

**구현 계약 보강:** export는 완료된 현재 세션의 bounded1024 receipt와 durable history 원본 JSON/full body hash·크기·canonical authority를 대조한다. sourceRoot 없는 legacy send나 이전 세션/evicted ID는 export-ineligible이며 HTTP를 다시 보내지 않는다. pending은 별도 사유다. formatter wrapper16KiB−256은 Notes의 hidden operation marker 여유를 포함한다. 선택 token256개/10분과 vault dev/ino·기존 file identity를 검사하며 일반 command64KiB는 바꾸지 않았다.

전용 Notes worker4개 상한·private durable operation receipt의 Attempted/Committed 및 note marker가 retry를 식별한다. Committed는 refresh보다 먼저 기록하고 unknown의 재시도는 관찰만 한다. caller 취소는 worker의 실제 처리 수명을 끊지 않는다. holder의 실제 BOM/CRLF2MiB·wire-text12MiB·rewrite 봉투8MiB는 publish 이전에 검사한다. UI가 raw body/URL을 제출하는 RPC는 없다. 완료 retry는 history가 사라져도 기존 결과를 반환하며 사용자 새 클릭은 새로운 operation UUID다.

근거는 `s6-1b1/{summary,source-freeze,commands,test-cases,resources-index}.json`·API helper·root recovery/CLI artifact다. setup/compile 실패·0개 선택을 성공에 합산하지 않았고 required cgroup memory_enforced=true를 보존했다. 정상 gen-ts/scoped TS/변경 TS Biome PASS, 전체 검사·실제 E2E/Windows·push/PR은 미실행이다.

## 8. S6-1b.2 — 코드 선택을 기존 변환 도구로 보내기

**Files:** `app/src/features/source/editor/CodeEditor.tsx`와 메뉴 연결, `app/src/features/tools/transforms/{handoff,handoff.integration.test}.*`.

**계약:** 기존 `sendToTransforms(text, source)`에 `source: "editor"`만 더한다. EditorView의 명시적 현재 선택을 캡처해 전달한다. 선택 없으면 비활성, 여러 selection은 순서와 구분 정책을 명시한다. 기존512KiB UTF-8 bound·잘림 안내·intentRevision·메모리 전용 store·탐지/도구 선택을 재사용한다. URL/query/localStorage/로그에 선택 본문을 넣지 않는다.

- [x] RED: 실제 CodeEditor JSON 선택 뒤 행동 버튼을 요구한1 FAIL을 확인했다.
- [x] GREEN: 신규 편집기5+기존 직접 영향11로 고유16 PASS. JSON·empty 반응·문서 순서 다중 선택(newline 결합/빈 cursor 제외)·비ASCII exact512KiB와 초과·빠른 연속 보내기/늦은 실패의 최신 입력 보존을 확인했다. 실제 Dirty+undo1의 본문/편집 상태 무변경 강화1도 PASS(중복 제외).
- [x] 별도 검토 APPROVE·지적0. 완료 Biome 뒤 `feat(source): send editor selections to transform tools`로 로컬 완료하며 사용자 요청에 따라 여기서 세션을 마친다.


구현은 CodeEditor의 view capture를 optional location 처리보다 앞에 두고 selection listener·헤더 행동을 추가했다. 기존 handoff의512KiB/intentRevision/memory-only 정책은 바꾸지 않고 source에 editor만 더했다. scoped TS·변경3파일 Biome PASS, 모든 실행 memory_enforced=true. 근거 `s6-1b2/`; 전체/E2E/native/push/PR은 NOT_RUN이다.

## 9. S6-1b.3 — HTTP OAuth authorize를 요청 소유 UI action에 연결

**Files:** API SendCollector/daemon backend adapter·send state·요청 화면, `crates/api/src/oauth*`의 기존 authorize/cancel 계약; MCP의 명시적 브라우저 action을 UI 참고로 재사용한다. 별도 OAuth 프레임워크는 추가하지 않는다.

**기존 §8 판정:** #645 `4367201424`는 S6에 포함. 이벤트를 버려 승인 흐름이 끝나지 않는 기능 누락이며 사전 검토의 새 비밀 유출로 부풀리지 않는다.

- [x] RED: HTTP authorize 이벤트가 실제 요청 화면에 도달하지 않는 좁은 adapter/UI 시험.
- [x] 요청ID·origin·tab generation 소유 pending action을 표시하고 사용자 [브라우저에서 승인] 클릭으로만 검증된 authorize URL을 연다. 이벤트 도착만으로 자동 open하지 않는다.
- [x] 취소·tab/root 전환·재전송은 pending action/callback ownership을 무효화한다. 늦은 완료는 다른 요청을 성공으로 바꾸지 않는다. 기존 state/PKCE·callback timeout·비밀 가림을 유지한다.
- [x] GREEN: authorize→명시 클릭→동일 요청 완료, 취소/늦은 이벤트·잘못된 소유자·브라우저 실패, 승인 전 외부 open0회.
- [x] 전체 독립 검토 APPROVE·지적0. 완료 Biome 뒤 `fix(api): expose request-owned HTTP authorization actions`로 로컬 완료.


구현 결과: 실제 rpcBackend/collector/useSend/요청 화면의 RED1→신규UI12·adapter4와 기존14로 고유30 PASS. Started 없는 authorize/중복 Started는 거절하고 response head/end/failed에서 pending을 해제한다. 같은 controller/tab/root의 현재 action만 명시 클릭으로 validated HTTP(S) URL을 열며 브라우저 오류는 고정 문구다. tab 전환은 OAuth action만 무효화하고 ordinary HTTP background 완료·save-key 변경·1b.1 sourceRoot/노트 action은 유지한다. Runner/import의 별도 승인 UI는 이번 개별 요청 편집기 범위에 포함하지 않는다.

`devbox-verification/s6-1b3/`에 원본 로그·중복 제거 목록·자원·소스 동결. scoped TS·변경10파일 Biome PASS, memory_enforced=true. Rust OAuth/PKCE/callback/redaction 코드와 RPC/generated·의존성은 변경하지 않았다. 실제 native browser/외부 OAuth·전체/E2E/push/PR은 NOT_RUN이며 전체 독립 검토 APPROVE·지적0를 확인했다. 완료 Biome 첫 포맷 실패2파일은 의미 변경 없이 바로잡고844파일 PASS를 확인했다. 원본 실패와 token 동일성 근거를 보존하고 로컬 과제 커밋으로 마무리한다.

## 10. S6-1b.4 — 지원 번들의 사용자 vault 경로 가림

**Files:** `crates/tools/src/support.rs`·`support/{archive,files,redact,summary}.rs`, CLI tools 생성 연결, Notes settings 접근 경계·support 시험.

**계약:** 현재 home/기본 `~/notes` 추정 대신 실제 NotesService의 canonical vault root를 bundle 시작 시 캡처해 전달한다. secrets-locked이면 로그 생략하는 기존 정책·fixed archive names·0600·제한된 읽기를 유지한다. 사용자 지정 vault가 home 밖이어도 정확 경로와 JSON/debug escaped 표현을 `<notes>`로 가린다. 짧은 prefix가 일반 단어/다른 형제 경로를 지우지 않도록 경로 경계를 검사한다. 설정 변경 중 일관된 snapshot을 쓴다.

- [x] RED/GREEN: custom vault 절대/escaped·home 내부/외부·겹친 prefix·secrets locked·일반 형제 경로 보존.

실제 NotesService configured/canonical snapshot을 bundle 전에 캡처한다. 캡처 실패는 고정 `SupportFailed`로 종료하고 기본 vault로 fallback하지 않는다. alias retarget 뒤에도 snapshot은 불변이다. 실제 노출 RED1→integration14·경계 unit1·CLI3으로 고유18 PASS, CLI 첫 compile 실패(선택0)도 보존했다. `devbox-verification/s6-1b4/`에 원본·명령·자원·소스 근거를 남겼으며 memory_enforced=true. locked 로그 생략·고정 archive 이름·0600·읽기/secret 제한은 유지한다. 전체/native·push/PR 미실행. 독립 전체 검토 APPROVE·지적0, 완료 Biome844 PASS(1.073초·memory_enforced=true) 뒤 로컬 과제 커밋으로 보존한다.
- [x] 독립 검토 APPROVE·지적0, 완료 Biome 뒤 `fix(tools): redact the configured notes vault in support bundles`로 로컬 완료.

## 11. S6-1c — 실제 의존성 경고 세 건 정리

**Files:** `app/package.json`, `pnpm-lock.yaml`, 필요 시 root overrides; 관련 preview/diagram·Vitest 시험만.

기준 검토의 세 advisory는 실제 affected version 정보다. 제품 exploit 재현이나 모든 경고 무해 판정은 아니다. 구현 시 공식 advisory/상위 release notes와 현재 lockfile을 다시 조사해 정확 버전을 pin한다(이번 계획 작성에서 업그레이드하지 않음).

- [x] `@vitest/mocker`4.1.11의 `GHSA-qxg6-89mg-rqv8`·`GHSA-xp26-3454-mjgp`: 수정4.1.12 이상 중 호환 patch를 Vitest 관련 패키지와 함께 pin. app에서 명시 mocker/interceptor plugin entrypoint가 안 보인 사실과 devdependency 범위를 기록한다. dependency graph + 대표 mock/실제 component 회귀를 실행한다.
Vitest 과제는 정확4.1.12 pin과 관련8개 package 전환만 적용했다. 공식 두 advisory/현재4.1.11 graph를 dependency RED로 보존했으며 앱 exploit 또는 실패시험을 재현했다고 쓰지 않는다. YAML 비교로 다른 package/importer 무변경, frozen offline install과 기존 mock/component15 PASS(2.924초·memory_enforced=true)를 확인했다. `devbox-verification/s6-1c-vitest/`에 근거를 남겼고 독립 검토 APPROVE·지적0, 완료 Biome844 PASS(1.695초·memory_enforced=true) 뒤 `chore(deps): update Vitest security fixes`로 로컬 완료했다. Mermaid/KaTeX 후속 독립 과제는 아래 기록한다.

- [x] Mermaid 경유 KaTeX0.16.47의 `GHSA-238p-pmpm-9mq7`: 수정0.18.2 이상을 받아들이는 **호환 Mermaid 경로**를 조사한다. 무검증 transitive override로 minor/major 경계를 강제하지 않는다. strict mode·htmlLabels=false·network-free diagram·SVG sanitizer 보존; math diagram rendering·유해 SVG 회귀를 실행한다.
Mermaid11 최신11.17.2는 affected KaTeX 경로여서 upstream Mermaid12.2.0을 정확 pin하고 실제 KaTeX0.18.10을 정상 pnpm9로 설치했다. 무검증 override 없음. 공식12 안내의 dagre/default/classic을 명시하고 strict/htmlLabels=false·network gate·SVG sanitizer를 유지했다. 직접 기존23+신규3=26 PASS·scoped TS·frozen offline install PASS, lock 변화는 Mermaid closure 신규11/삭제7뿐이고 Vitest4.1.12는 불변이다. baseline config RED와 inert-node matcher 시험작성 실패를 구분 보존했다. 실제 sequence MathML은11/12 모두 생성되나 최종 sanitizer가 제거하며 flowchart는 literal dollar text, LaTeX command는 차단된다. 이는 기존 표시 제한이며 수학 화면 지원 PASS로 보고하지 않는다. Node24 설정 호환은 읽기로 확인했고 native/build는 미실행. 근거 `devbox-verification/s6-1c-mermaid/`, 독립 검토 APPROVE·미해결지적0, 완료 Biome845 PASS(1.074초·memory_enforced=true) 뒤 로컬 완료했다. 변경 검사5경로 중 실제4파일 집계 정정은 원본로그와 clarification artifact에 보존했다.

- [x] 상위 호환 수정이 없거나 새 stack이 필요하면 정확 경로/대안/위험과 후속 판단을 PROGRESS에 남긴다. prior prototype pollution+unsanitized insertion의 exploit chain을 확인하지 않고 현재 앱 XSS라고 쓰지 않는다.
- [x] 각 변경 독립 검토·Biome 뒤 `chore(deps): update Vitest security fixes`, `chore(deps): update compatible Mermaid math dependencies` 등 실제 변경에 맞는 커밋.

## 12. S6-2 — 검증 예산을 확보하고 prod 태그 릴리스 job 추가

**Files:** `.github/workflows/{ci,windows}.yml`, `scripts/{win-package,win-sync,win-build}.ps1`, `scripts/tests/windows-*.ps1`, `scripts/{release,dogfood}.py`, `scripts/tests/test_release.py`, `scripts/{check-inner,build-linux,ci-scope}.sh`, 실제 중복이 확인된 부분만. 새 workflow를 기본안으로 추가하지 않는다.

착수 기준 `.github`+`scripts`는1,500줄·workflow2개였고, Windows fixture 진입점 subset의 실제 순 감소8줄 뒤1,492줄이었다. batching/release 구현 후 현재는 **1,881줄·workflow2개**다. 2026-10-26 사용자는 앞선1,800줄 승인에 이어 “2,000줄로 조정하고 계속 진행 (권장)”을 명시 승인했으며 현재 공통 상한은2,000줄이다. 현재 예산은119줄 남지만 S6-4 계측 등을 포함한 최종 fit은 아직 확인하지 않았다. 제품 LOC121277는 별도 과거 관찰값이다. PR Windows22분43초는 SC7 15분 미달이며 product main14분30초가 이전 PR 미달을 지우지 않는다. 01 SC7의 PR15분과 00 §3의 CI15분/Windows40분 표기가 다르므로 두 실제 경과를 병기하고 적용 기준을 명시한다. 시간 목표는 바꾸지 않으며 최종 실측 전 달성을 약속하지 않는다.

**먼저 하는 예산 작업:**

- [x] 파일별 line/call 구조와 중복 setup을 조사하고 fixture/batching/release의 실제 line ledger를 artifact로 남겼다. 반복 install/frontend/native 검증을 한 invocation으로 묶고 필요한 채널별 검증을 보존했다.
- [ ] 실제 Windows job timing·dependency 재빌드 비용은 최종 수용에서 측정한다. 과거 PR22분43초/main14분30초와 새 실측을 구분한다.
- [x] shared Cargo target 도입 대신 prod/accept/dev의 채널별 target을 유지하기로 결정했다. 기존 cache와 config·identifier·embedded resource·receipt 격리를 보존하며 추가 target 공유를 미구현 결함으로 취급하지 않는다.
- [x] 채널 불변 install/frontend 검증은 한 번, 채널별 identifier/resource/self-check는 각 채널에서 유지하도록 package 흐름을 나눈다. Linux provenance gate·실패 전파·환경 복원·owned checkout guard를 없애지 않는다.
- [x] 실제 공유 책임만 통합했다. Windows fixture 공통 진입점·ErrorAction/Reject, release preflight/publish validator와 기존 receipt/gh helper, 하나의 fake adapter를 재사용한다. 기존 dogfood 시험과의 receipt 표본만을 위한 추가 helper는 만들지 않았다. 구현 전후 line ledger를 보존했으며 assertion 삭제·한 줄 압축·count 밖 이전은 하지 않았다.
- [x] 좁은 fixture subset: 기존3suite를 단일 진입점으로 합치고 공통 ErrorAction/Reject만 공유했다. 세 GUID/개별 finally·모든 성공/거절/정리 계약은 유지하며 fixture174→168·workflow123→121로 총8줄을 줄였다. 기존34계약 mapping·AST finally3/fake5 동일·YAML의 호출3→1 외 무변경을 확인했다. Windows5.1 UNC와 검증된 owned Temp local copy 모두 host Restricted 정책에서 suite0/NOT_RUN이므로 실제 동등성은 최종 Windows CI 미실행 항목이다. 정책은 바꾸거나 우회하지 않았으며 staging cleanup과 Linux wrapper/Windows host 자원 경계는 `devbox-verification/s6-2-fixtures/`에 기록했다. 정적 독립 검토 APPROVE·지적0·완료 Biome845 PASS 뒤 로컬 subset 커밋으로 보존하며 전체 S6-2 완료가 아니다.
batching 착수 전1,492줄에 남은 batching/release 검증을 더한 중심 추정은1,709줄(구현 전)이었다. 두 독립 계획 검토 뒤 사용자가2026-10-26 “1,800줄로 조정하고 계속 진행 (권장)”을 먼저 승인했다. 실제 batching131줄/release258줄 증가로1,881줄이 되어 당시 상한보다81줄 초과했고, 이전 추정25줄/192줄은 합계172줄을 과소 추정했다. 특히 회귀 시험 규모 추정이 부족했다. 이 실패와 후속2,000줄 승인 기록은 PROGRESS §2·§5에 함께 보존한다. 현재 예산 충족은 S6-4 helper 등을 포함한 최종 fit 또는 SC7 시간 목표 PASS가 아니다.

**Batching 직접 검증(2026-10-26):** 순서 있는 `-Channel` 배열 한 invocation에서 install/frontend 각1회·native Verify 기존1묶음을 유지하고, 채널별 target/config/resource/self-check/receipt는 그대로 분리했다. frontend 성공 후 소유 임시 JSON에만 beforeBuildCommand 빈 문자열을 덧붙이고 같은 전체 채널 JSON을 env와 CLI 파일로 전달한다. dev hook·single-channel·실패 전파·env/cwd 복원은 유지한다. 검증된 portable PowerShell7.6.9 Linux에서 실제 script의 PackageOnly baseline PASS→ordered 배열 RED→28case PASS; Windows/native PASS가 아니며 기본 Windows CI는 sync/package/dev3suite를 유지한다. 실제1,492→1,623줄(+131)·workflow2; 상한1,800 이내지만 release/musl/shared target/timing은 이번 범위 밖이다. 근거 `devbox-verification/s6-2-batching/`, 독립 검토 APPROVE·지적0·완료 Biome845 PASS 뒤 로컬 완료했다. 검토 후 소유 portable 도구만 정리했고 원본 metadata/hash/log는 보존한다.

- [x] 핵심 안전 assertion을 유지하고 책임 공유를 적용했으나1,800줄에는 맞지 않아 실제 초과분을 공개했다. 사용자 추가 승인 후 현재1,881/2,000줄을 확인했다. 남은 계측을 포함한 최종 예산은 다시 실측하며 상한을 조용히 변경하지 않는다.

2026-10-26 batching e220e25d 뒤 tag preflight/publisher·fake회귀·musl CLI release 시험 게이트·actionlint를 구현했고 독립 코드 검토 지적0·기존 Biome845 PASS를 확인했다. 최초 검토의 **1,881/1,800줄 FAIL·예산 차단1**은 당시 원본으로 보존한다. 이후 사용자2,000줄 승인으로 현재 **1,881/2,000줄 PASS·workflow2**이며 예산 결정 대기는 해소됐다. 실제 musl/Windows build·공개 API·timing은 미실행이고 근거는 PROGRESS §2·§5와 `s6-2-release/`를 따른다.

이번 추가 승인은 현재 release 구현의 예산 차단을 해소한다. 제품/workflow/test는 검토 동결본 그대로이며 승인 반영 문서의 최종 독립 검토·Biome·로컬 커밋 뒤 S6-3a를 진행한다. 공개/tag 승인은 아니며 최종 수용·시간 측정은 별도로 남는다. 이전 미승인 제안 원본 `s6-2-release/budget-decision.json`과 승인 반영 근거 `s6-2-release/budget-resolution/`을 구분한다.

**릴리스 계약:**

- `v*` 태그의 엄격 semver/tag↔manifest 버전 검증 → linux-bin musl 테스트·build → Windows prod build·self-check → `release` job.
- release는 앞 job 성공과 해당 태그 source SHA의 receipt를 확인하며 `contents: write`는 그 job에만 준다. PR/main/dispatch는 릴리스 생성·업로드를 하지 않는다.
- prod NSIS 정확히 하나를 `devbox-<v>-setup.exe`(v는 버전 숫자, 예: `devbox-1.0.0-setup.exe`)로 올린다. accept installer·receipt·old `Devbox_{v}_x64-setup.exe`·`release-manifest.json`은 공개 자산에 섞지 않는다.
- 주석 태그 메시지가 릴리스 노트의 원장이다. 태그 메시지/이름을 shell 코드로 interpolation하지 않고 파일/환경 데이터로 넘긴다. updater가 비교할 GitHub asset SHA-256 digest가 제공되는지 확인한다.
- 중복 tag/release 재실행에서 이미 다른 digest인 자산을 조용히 덮지 않는다. candidate/seal 절차는 만들지 않고 문제가 있으면 다음 patch다.
- PR/수동 실행 accept 산출물·prod provenance receipt와 source SHA 결속은 유지한다. packaging script는 실제 Windows에서만 빌드한다.

- [x] RED/GREEN: 실제 publisher 진입점의 fake subprocess/API6 test methods PASS. tag gate·버전/source/manifest mismatch·Linux 명령 실패 전파·0/2 installer·accept 혼입·태그 본문 특수문자·재실행 digest 충돌·draft/public 경계를 검증했다. 실제 release API 쓰기는0이다.
- [x] 공식 actionlint1.7.11을 기존 CI changes job에 추가하고 로컬 actionlint+shellcheck로 두 workflow를 검사했다. 현재 workflow2≤3·합계1,881≤2,000을 실제 count로 확인했다. hosted Docker action 실행은 최종 CI에 남는다.
- [ ] orchestration fake 시험은 native build PASS로 보고하지 않는다. 실제 Windows build/40분 실측은 최종 수용·공개에서 확인한다.
- [x] batching 독립 APPROVE·Biome845 PASS 뒤 e220e25d로 로컬 완료했고 release 독립 코드 검토는 지적0이다.
- [x] 사용자2,000줄 승인 반영 최종 독립 검토 APPROVE·미해결0·Biome845 PASS 뒤 `feat(release): publish production installers from version tags`로 로컬 완료했다. 실제 태그·공개·native/최종 CI는 아직 미실행이다.

## 13. S6-3 — Rust가 소유하는 업데이트와 안전한 재시작

**Files:** 새 `app/src-tauri/src/core/update*.rs`(순수 상태/자산 검증), `platform/update*.rs`(Windows download/install), core/platform 등록·Tauri commands; `app/src/features/settings/Update.tsx`와 상태 표시줄; `core/quit.rs`, `platform/quit.rs`, `platform/window.rs`, 기존 `core/deploy.rs`/`platform/deploy.rs`. renderer는 도메인 updater 소유자가 아니다. native Cargo에는 현재 HTTP client가 없으므로 reqwest/rustls는 기존 workspace 버전으로, semver는 필요 시 현재 lock의 호환 버전을 확인해 최소 의존 edge만 필요에 따라 추가하고 native TLS provider 초기화·Windows build 영향부터 확인한다. 새 stack/서명 updater는 아니다. 공식 근거: [GitHub release asset digest/API](https://docs.github.com/en/rest/releases/assets?apiVersion=2022-11-28), [Tauri Windows NSIS](https://v2.tauri.app/distribute/windows-installer/)의 currentUser 설치. 공개 asset API의 200/302 응답·redirect와 hash 검증을 함께 시험한다.

### S6-3a 상태·다운로드

- [x] typed 상태 `idle/checking/available/downloading/ready/installing/failed`에 operation generation·버전·bytes·오류/다음 행동을 둔다. Rust managed state 하나, `update_status()` snapshot + `devbox://update` 모든 창 broadcast를 제공한다. window mount/unmount는 작업 생존과 독립이다.
- [x] 릴리스 prod만 하루 한 번/명시 요청에 GitHub latest 확인. `-dev`·dev/accept 채널은 공개 updater를 비활성화한다. accept 시험은 **컴파일된 accept/test 전용 capability**로 소유 fixture I/O를 주입한다. prod의 임의 URL/env bypass는 만들지 않으며 prod 자산·다른 채널을 자동 설치하지 않는다.
- [x] strict release version·정확 asset 이름·단일 exe·HTTPS/GitHub 출처·download size/time bound·SHA-256 digest 필수. prerelease/draft·동일/낮은 버전·digest 없음/불일치·다른 채널은 설치 불가.
- [x] `%LOCALAPPDATA%` 채널 전용 updates 아래 소유 임시 파일로 streaming download→hash 확인→atomic ready. retry/cancel/generation 교체는 자기 임시 파일만 정리하고 다른 실행의 파일을 지우지 않는다. 설치 전 파일 identity/digest를 다시 확인한다.
- [x] RED/GREEN: GitHub fixture 자산 선택·버전·digest·download cancel/race·중복 호출·prod wrong-channel 거절. fixture installer도 소유 marker/source/hash/채널을 검증하고 동결한다. 창 닫기→새 창 snapshot이 같은 진행·취소 권한을 받는 CV-F09 시험.
- [x] 독립 검토·`feat(updater): own update state and verified downloads in Rust`(로컬).

3a 구현·직접 검증: managed Service/worker1·pending1, generation+revision, 모든 창 event/snapshot과 설정 정보/상태줄을 연결했다. 취소 뒤 실제 writer cleanup 완료 전 다음 작업을 시작하지 않는다. 공개 prod stable만 persisted24h 자동/명시 확인하며 compiledaccept/test fixture는 owner/source/hash/channel/file identity로 분리했다. app 버전은 항상 Rust compiled 값이다. HTTP200/302·redirect3·metadata1MiB/installer256MiB·connect10/read15/metadata30/download300초·no-clobber/hash/identity proof를 적용했다. 재검증 seam은3b 설치 직전에도 다시 호출할 대상으로 준비했으며 설치 자체는0회다. 신규 parser/TLS·frontend 실패를 보존하고 Rust27+frontend40=고유67 PASS, scoped TS·변경 TS Biome·Rustfmt PASS. 소스별 재사용/원본/limits 근거는 `devbox-verification/s6-3a/`. 제품·문서·근거 최종 독립 APPROVE·미해결0·완료 Biome850 PASS(1.108초) 뒤 로컬 완료. Windows/실제NSIS/공개TLS handshake/native event 실기와 전체 검증은 미실행이며67 PASS로 대체하지 않는다.

### S6-3b 설치 확인·all-window 종료·재기동

> 2026-10-26 현재: 구현과 고유170 좁은 검증 PASS(coordinator19·frontend/runtime93·native35·runner23), installer 기록 공백 RED→복구10 GREEN 및 최종 소스 검토 미해결0을 확인했다. 제품·문서·근거 최종 독립 APPROVE·미해결0·완료 Biome852 PASS 뒤 로컬 완료·인계로 고정한다. MSVC target 소스 확인은 ring build의 lib.exe 부재로 NOT_COMPILED, 실제 Windows/NSIS는 NOT_RUN이다. 아래 Windows 수용 체크는 로컬 구현 완료와 구분해 최종 candidate까지 남긴다. 사용자 한도 소진 지시에 따라 현 작업 인계를 정리하고 S6-4는 시작하지 않는다. 상세·유일 진행 원장은 PROGRESS의 S6-3b다.

- [x] ready 뒤 [업데이트]는 agents/services 실행 수 요약과 데몬 작업 유지 안내를 보여 주고 **명시적으로 설치 확인**받는다. 네이티브 별도 확인 대화상자/서명 updater는 추가하지 않는다(ADR0016).
- [x] 기존 all-window prepare/dirty flush barrier를 통과한 경우에만 설치를 시작한다. 실패/timeout/창 거절은 앱·작업 유지, installer 실행0회. 여러 창 중 한 창의 확인으로 다른 dirty 창을 건너뛰지 않는다.
- [x] §8 #644 `finish(approved)→새 창 register→RequestExit`를 실제 core/platform 호출 순서로 먼저 좁게 재현한다. updater가 이 경계를 쓰므로 **S6에 포함해 판정**한다. late registration은 승인 generation을 취소하거나 설치 종료 commit 동안 새 창 생성 자체를 막는 동일 정책을 일반 quit와 공유한다.
- [x] boundary를 불가능으로 유보하지 않고 core RED와 공유 admission/최종 fence로 보정했다. 기존 “손실 미재현”을 Windows 데이터 손실 PASS/FAIL로 바꾸지 않았다.
- [x] runner handoff는 `prepared → armed/ready ACK → committed`로 구분한다. permit은 고유 update operation·quit generation·channel·정확한 old app PID+start identity·검증된 installer identity/hash에 결속한다. runner를 준비해도 **설치/종료 commit 승인과 그 정확한 old app의 실제 종료 둘 다** 확인하기 전 `/S` 실행0회다. bridge drain을 시작했다는 사실은 process exit가 아니다.
- [x] prepare 실패/취소·quit 중단이면 runner permit을 disarm하고 만료시킨다. 나중의 무관한 정상 앱 종료가 설치를 되살리지 못한다. commit의 되돌릴 수 없는 경계에서 다른 update·새 창·새 native attempt 진입을 고정된 generation 정책으로 차단한다. caller가 임의 path/permit을 보낸 IPC는 승인 근거가 아니다.
- [x] runner는 기존 app Job과 설치 교체 대상 디렉터리 밖의 **소유·동결된 source/hash 경로**에서 실행한다. ready ACK와 operation identity를 확인하고 대기/실행 lifetime에 명시적 상한을 둔다. PID 재사용·stale permit·timeout은 설치 거절이며 자신 소유 파일만 정리한다.
- [ ] commit+exact process exit를 확인한 runner가 `/S`를 정확히 한 번 실행하고 installer exit·소유 결과를 기록한 뒤 검증된 동일 채널 앱만 relaunch한다. 설치 실패는 다음 시작 상태로 복구 안내한다. app 종료와 함께 runner가 죽거나 NSIS가 아직 실행 중인 old app 파일을 건드리지 않는지 Windows에서 확인한다.
- [x] **앱 native fallback Windows attempt는 WSL 작업과 수명이 다르다.** `Connection.begin_exit`가 bridge/supervisor를 취소하며 native attempt는 stream token/Windows 취소 경로에 묶인다. 기존 S2도 child 종료를 완전히 보장하지 않는다. 따라서 활성 app-owned native Windows attempt가 있으면 설치를 막고 완료 대기 또는 기존 실행 UI에서 사용자가 명시적으로 중지하도록 안내한다. updater가 암묵적으로 stop하지 않는다.
- [x] Rust가 소유한 active-native-attempt counter/generation을 summary와 최종 quit 직전에 재검사하고 설치 commit 동안 새 native attempt 진입을 막거나 승인 generation을 취소한다. 목록 조회 성공만으로 종료 순간까지 안전하다고 가정하지 않는다. 설명은 WSL 관리/direct daemon 작업 유지와 native fallback 설치 차단을 구분한다.
- [ ] 새 앱은 기존 deploy/version 경로로 daemon 바이너리 차이를 처리한다. WSL agent tmux·service unit을 stop/kill하지 않는다. bridge 재연결과 기존 세션 재부착을 실제 수용에서 확인하며 모든 Windows 실행이 앱 업데이트 뒤 유지된다고 약속하지 않는다.
- [x] 기존 accept 앱은 updater가 없고 모두1.0.0-dev다. accept fixture의 version 검증 우회나 같은 버전 NSIS 재설치를 실제 공개버전 업그레이드 PASS로 쓰지 않는다. 순수 version/hash/state 시험, accept 전용 I/O·installer lifecycle 시험, 다음 patch의 실제 업데이트를 별도 결과로 유지한다.
- [ ] RED/GREEN: 확인 전 실행0회, prepared runner+quit 실패/취소 뒤 나중 정상 종료에도 설치0회, bridge drain 지연 중 설치0회, PID reuse/stale permit/wrong generation 거절·exact exit 뒤1회, dirty window·새 window/lease·중복 confirm·다운로드 교체·installer 실패·runner 생존/relaunch·작업 요약 unavailable의 명확한 안내, active native attempt 차단/완료 후 재개·summary 이후 새 attempt 진입·최종 quit 재검사·중지 호출0회. updater state의 fake 성공을 NSIS 설치 성공으로 세지 않는다.
- [x] 독립 검토·`feat(updater): install after all windows approve shutdown`(로컬 구현 완료; 위 실제 Windows 수용은 미실행).

## 14. S6-4 — 성공 기준과 과거 미실행 항목의 실제 측정

### 계측·기존 후속 준비

**Files:** 실제 이벤트 소유 지점 `platform/shortcuts.rs`, bootstrap/connection/UI 첫 데이터 표시; 기존 `scripts/dev-session.py`의 dev timestamp, terminal latency 시험, 필요한 bounded native 수용 도우미. source/runtime의 readiness는 아래 별도 과제로 둔다.

- [x] SC2 monotonic timestamp는 단축키 수신→팔레트 paint, 프로세스 시작→첫 데이터 paint를 같은 run ID로 결속한다. 단순 RPC ping/DOM 존재를 사용자 표시 시간으로 대체하지 않는다. — 결과(계측만, `496b1c78`): native `Instant`→bounded16 trace→이중 rAF ack, `run()` 첫 `Instant`→AgentList daemon 데이터 paint, 같은 `run=` process ID. 실제 설치본 표본 NOT_RUN·Windows 컴파일 NOT_COMPILED.
- [x] SC6는 실제 source 변경 시각→HMR 반영, daemon 변경→ready+browser 재연결을 잰다. dev-session readiness 자체를 전체 reload 시간이라고 쓰지 않는다. — 결과(계측만): `dev-session.py` SIGUSR1 소유 daemon 재시작·monotonic 출력, `app/acceptance/sc6.spec.ts`(UI 표시·buildMs/restartMs 분리), 2단계 정리 수정`7d638599`. 실제 실측 NOT_RUN.
- [x] #642 Windows readiness를 S6에 포함: `crates/cli/src/windows_exec.rs` 직접 경로와 `app/src-tauri/src/platform/windows.rs` native 경로 모두 localhost listener의 PID/생성시각·승인된 child/후손 귀속을 확인한다. PID 재사용·후손 종료·조회 실패는 ready=false/명확 실패다. — 결과(`313a6095`): runtime 단일 gate(durable Committed receipt)와 `WindowsProbe::Listeners`를 direct→native 공유 경로로 사용, receipt 검증 뒤 job 귀속(`IsProcessInJob`)+table 재읽기, 실패·불일치는 typed not-ready. port 없는 Windows 서비스는 기존 `wait_ready` 유지(판정). C# 컴파일·Windows 실행 NOT_RUN.
- [x] RED: Windows 소유 unrelated listener만 연 상태에서는 서비스 미준비, 실제 child/후손 listener만 준비. IPv4/IPv6·zero-port·process exit/timeout·직접/native 동등성 좁은 시험. 기존 호스트 listener·방화벽을 바꾸지 않는다. — 결과: Linux gate RED5→GREEN(unrelated→미준비·owned→준비·Unavailable/IdentityMismatch/Empty/NotFound·Intent/Revoked/Closed·lock), runtime6·protocol8(port0 거절 포함)·CLI21·app19 PASS. IPv4/IPv6·exit/timeout 실제 Windows fixture는 작성만(ignored·NOT_RUN), direct/native Windows 동등성 NOT_RUN.
- [x] 검토·`fix(runtime): bind Windows readiness to the owned process tree`; 계측은 별도 `test(acceptance): measure release readiness criteria`. — 결과: `313a6095` 독립 APPROVE(+기존 회귀 `95a742f9` APPROVE), `496b1c78`+`7d638599` 재검토 APPROVE, 시험 정렬 `25d1bbb0` APPROVE(로컬).
- [ ] Windows gating: `DevboxListeners` C# 컴파일·ignored interop fixture·Listeners 5초 예산 계측·direct/native 동등성은 아래 실제 Windows 수용에서 실행한다. 그 전까지 NOT_RUN이며 실패하면 port 있는 Windows 서비스가 준비되지 않는다.

### SC별 판정 원장

| SC | 실제 측정·기준 | 증거/미실행 처리 |
|---|---|---|
| SC1 | 흔적 없는 전용 Windows 계정+systemd WSL에서 setup→첫 데이터, 사용자 행동 배포판 확인≤1회 | 기존 사용자 자료를 지워 계정을 흉내 내지 않음. 계정/독립 환경 없으면 NOT_RUN |
| SC2 | 전역 단축키→paint≤300ms; cold→첫 데이터 WSL running≤3초/stopped≤10초 | 반복 횟수·cold/warm/cache/배율·raw timing 보존. host WSL shutdown 금지, stopped 측정은 격리 환경/최종 R2 |
| SC3 | 제목+도구+시작 한 번으로 작업 준비/실행; 실제 hook waiting/turn-end가 목록·Windows 알림≤2초 | Claude/Codex 각각 실제 hook, fake hook PASS로 대체 금지 |
| SC4 | 창 닫기·daemon 재시작·업데이트 뒤 WSL agent/terminal/service 유지, app-native Windows attempt는 설치 전 별도 차단 | 소유 test instance·tmux/user systemd, 전후 세션 ID·PID/작업 관찰. WSL 종료는 유지 대상 아님 |
| SC5 | generated code 전수 오류 문구·원인+다음 행동; internal은 진단 ID 동반 | 컴파일/typed map+대표 E2E, raw error/body 노출 없음 |
| SC6 | UI 변경→실제 daemon 연결 browser≤2초; daemon 변경→restart/reconnect≤15초 | monotonic 구간·source 변경·ready 증거, fake 서버 제외 |
| SC7 | PR CI≤15분, 로컬 전체(L3 포함)≤10분, release≤40분; scripts+.github≤2000, workflows≤3 | PR/main·queue/실행·cache별 raw 기록. 이전 PR22m43와 main14m30 구분; 한도 미달을 PASS라 하지 않음 |
| SC8 | `bash scripts/loc.sh 200000`, 제품 코드≤20만(관찰 트리거) | 테스트 제외와600줄 제한 별도 보고. 기준121277은 과거 값 |
| SC9 | 작업 없는 daemon RSS≤60MB·CPU≤0.5% | 실제 `/proc`·systemd, warm-up·표본 간격/기간·background 작업 기록 |
| SC10 | Windows bridge→daemon→PTY→tmux byte echo p50≤10ms/p95≤25ms; 100MB 출력 중 다른 창 ping p95≤100ms, 종료 후 입력≤1초 | 실제 설치본, 샘플/percentile·출력 flood byte 수. renderer paint 제외 |

실측값이 미달이면 원인·최소 수정/재측정 또는 명시적 미달/다음 patch 계획을 기록한다. 근거 없는 목표 완화·측정 생략은 하지 않는다. 자동화 불가와 제품 실패를 분리한다.

### 실제 Windows 수용 실행 순서

- [ ] 구현 중에는 fixture/좁은 검증까지만. **S6-5·S6-6a 버전 고정과 최종 검사 이후 최종 candidate commit을 PR 없이 백업 push한 뒤 workflow_dispatch로 얻은 정확 source SHA의 성공한 windows.yml artifact**로 설치 수용을 한 번 묶어 실행한다. 준비와 실행 시점을 혼동하지 않는다.
- [ ] 저장소·workflow·run·source SHA·receipt/installer SHA256·채널을 대조하고 독립 helper 검토/실행 binding을 남긴다. `windows-acceptance` 잠금, PowerShell 절대 경로, 소유 accept 설치/instance만 사용한다.
- [ ] 시작 전후 prod 설치·파일·unit을 읽기 전용 비교. 소유 표식 없는 accept 자원은 보존하고 해당 실행을 중단·보고한다. UAC/재부팅/사용자 클릭을 우회하지 않는다.
- [ ] S2 설치1–11 NOT_RUN·12 FAIL 및 prod NOT_PROVEN을 재수행 목록으로 옮기고, 후속 interop 회복만으로 과거 결과를 소급 변경하지 않는다.
- [ ] S3 설치4/6/7/8·엄격 autosave1초/SC2, S4 실제 LSP2/3/4/7, S5 설치5/6·external LAN·실제 도구 설치 미실행을 항목별로 재평가한다. 다른 SHA의 성공1/6·3/4 등은 historical로 남긴다.
- [ ] 실제 LSP는 소유 프로젝트/임시 tool 경로로 제공 가능한 서버로 진단·정의·서식·multi-file Rename과 stale refusal을 확인한다. 관리형 installer/숨은 global 설치는 만들지 않는다. 환경 부재면 정확 전제와 NOT_RUN을 기록한다.
- [ ] HTTP→일일/선택 dirty 노트·editor handoff·OAuth 명시 승인·Windows 복사 분리·service→dependent/sibling 공유 revision/readiness·updater all-window/reconnect·지원 bundle masking을 최종 source에서 확인한다.
- [ ] LAN/설치/네트워크 변경 시험은 독립 VM/hosted 환경에서만. fake HTTP/LSP 시험은 해당 실제 수용을 대체하지 않는다.
- [ ] Codex 새 worktree trust waiting·tmux3.4 종료 미확정은 실제 SC3/4/10 실패 시 좁게 원인을 재현한다. 미재현이면 미확정 후속으로 남기고 근거 없는 수정은 하지 않는다.
- [ ] 소유 자원만 정리하고 retained 목록·prod 불변 비교를 남긴다. R1(30분 idle)·R2(WSL shutdown 시간)는 공개 직전 사용자 확인까지 NOT_RUN. 에이전트가 공유 WSL을 shutdown하지 않는다.

## 15. S6-5 — 실제 사용 문서와 최종 계약 정리

**Files:** `README.md`, `AGENTS.md`, `CLAUDE.md`, `docs/ui-terms.md`, `docs/adr/0017-rebuild-single-app-wsl-daemon.md`, 이 계획·PROGRESS와 필요한 설계 계약 위치만.

- [ ] 설치/개발/Windows 스크립트/MCP 등록·문제 해결·진단 ID·지원 bundle·수동 새 앱 설치→v0.9.0 수동 제거를 실제 코드에 맞춘다. v0.9 데이터 자동 이전은 없다.
- [ ] updater 취소/실패·작업 유지·dirty 창 처리·dev/accept 비활성·구버전 복사 디렉터리 수동 보존 정책을 쓴다.
- [ ] 화면 한국어 용어를 docs/ui-terms와 맞추고 ADR0017의 채택 상태를 실제 구현 검토 뒤 변경한다.
- [ ] 과거 실측·검증 SHA·FAIL·NOT_RUN·이전 버전 예제는 역사로 보존한다. 현재 상태 줄만 갱신하며 S6 로컬 완료/PR 수용/공개 완료를 구분한다.
- [ ] 별도 문서 검토·Biome 뒤 `docs: document v1 installation updates and recovery`.

## 16. S6-6 — 한 번의 버전 변경·최종 검사·공개 경계

### S6-6a 버전 고정

- [ ] `Cargo.toml` workspace, `app/package.json`, `app/src-tauri/Cargo.toml`, `app/src-tauri/tauri.conf.json` 및 실제 channel config의 override 유무를 확인한다. 현행 `1.0.0-dev`→`1.0.0`을 **한 번만** 반영한다.
- [ ] Cargo.lock(두 manifest 경로의 실제 lock)·pnpm importer 등 연결된 generated metadata를 정상 도구로 갱신한다. 모든 packaged binary/resource/manifest가 같은1.0.0인지 검증한다.
- [ ] 저장소 전체 문자열을 지워 historical docs/fixtures를 변조하지 않는다. 버전 원장 목록과 런타임 포함 경로의 일치 검사로 검증하며 남은 `1.0.0-dev`는 history/dev-disable fixture 등 이유별로 기록한다.
- [ ] 독립 검토·Biome·`chore(release): set version to 1.0.0`. 아직 tag/release는 없다.

### S6-6b 브랜치 전체·최종 검사·한 PR

- [ ] 전체 브랜치 독립 검토: 네 수정·통합·소유권·redaction·업데이트·문서/근거 일치. 해결되지 않은 항목과 SC 판정을 먼저 고정한다.
- [ ] 자원 supervisor 아래 최종 **전체 check·workspace/native 해당 clippy·build·E2E를 한 번** 계획된 entrypoint로 실행한다. `scripts/check-inner.sh`가 포함하는 항목은 중복 별도 실행하지 않고 missing scope만 보충한다. 실패 뒤에는 실패/직접 영향 범위만 재실행한다.
- [ ] 전체 제품/버전·로컬 검사 완료 → 최종 Biome CI → **candidate commit 고정 → 백업 branch push(PR 없음) → windows.yml workflow_dispatch** 순서로 원격에 정확 source를 제공한다. 이는00 §3의 설치 artifact용 기존 백업 push 예외이며 과제별 중간 push/PR을 하지 않고 이 최종 수용 준비 시점에만 수행한다. dispatch ref의 실제 run head가 candidate SHA인지 확인하고 성공·receipt/hash/채널을 대조한다.
- [ ] 그 Windows artifact로 §14 설치 수용·SC 계측을 수행한다. Windows 전용 build/strict Clippy는 Windows 근거를 쓴다. 로컬 Linux 성공을 Windows 성공으로 바꾸지 않는다. 제품/packaging/runtime 입력이 바뀌면 새 candidate·artifact와 영향 수용을 다시 결속한다.
- [ ] 결과 문서만의 후속 커밋은 실제 native 실행 SHA와 runtime/build 입력 동등성·재사용 출처를 CONVENTIONS §8에 따라 남긴다. 새 문서 SHA에서 binary를 다시 실행했다고 쓰지 않는다. 문서가 바뀔 때마다 불필요하게 hash/build loop를 돌리지 않는다.
- [ ] PROGRESS·단일 PR 본문용 결과에 원본 실패→수정 영향 재검증, ignored/NOT_RUN·historical source·timing·cleanup·hash를 구분해 기록한다. artifact는 common-dir에 bounded JSON/log/trace로 보존하며 새 날짜별 Markdown 보고서를 만들지 않는다.
- [ ] 결과 문서·최소 영향 수정의 검토와 Biome 뒤 필요한 최종 branch push·**S6 PR 한 번**. `Frontend (pnpm)`, `Rust (Cargo workspace)`, `Rust (Windows)` 최종 PR SHA 필수 성공 및 branch protection 유지. E2E 등 실제 요구 job도 확인한다.
- [ ] 자동 리뷰는 현재 사용자 설정상 OFF. #646의 “머지까지만” 독립 검토 대체는 **S6 전체 면제 아님**. **사용자 결정(2026-10-26): “독립 검토로 대체 (Recommended)”.** S6 PR 자동 리뷰는 NOT_RUN(비활성화)으로 기록하고 PASS·한도 소진으로 쓰지 않는다. 이 결정은 S6 PR에만 적용한다.
- [ ] 하네스 독립 코드·보안 검토가 CONVENTIONS §8 상한(완료 SHA 최대3회·4회 이후 실제 손실/유출/미승인 실행 차단 기준만 수정, 나머지 PROGRESS §8)을 그대로 적용해 자동 리뷰를 대신한다. 수정은 회차별 묶고 별도 검토한다. 필수 CI3·branch protection은 바꾸지 않으며 공개/tag 승인이 아니다.
- [ ] 승인된 절차·필수 CI를 모두 충족한 뒤 squash merge, main CI 확인·ff-only 동기화. final branch/merge tree와 source 근거를 비교한다. 공개는 아직 하지 않는다.

### S6-6c 공개 직전 단 한 번의 명시 승인

- [ ] 공개 준비 결과·SC1–10·미실행/미달·설치 산출물·릴리스 노트 초안을 구체적으로 제시한다. 사용자가 판단할 결과를 먼저 완성한다.
- [ ] 아래13개 참고 확인과 함께 **v1.0.0 공개 승인**을 받는다. 사용자는 목록을 모두 직접 실행하지 않고 승인할 수 있다. 승인 없이 tag push·GitHub Release를 만들지 않는다.

| 번호 | 공개 직전 확인 |
|---|---|
| 1–5 | 새 설치 첫 화면, agent 전 과정, service/log, note 작성, v0.9 제거 후 정상 |
| 6–7 | Windows 배율150%/200% 가독성, 키보드만으로 palette→작업→확인→오류 재시도 |
| 8–9 | API 응답 일일 기록 append, editor JSON 선택→변환 도구 |
| 10 | R1:30분 idle 연결·세션 유지(이전까지 NOT_RUN) |
| 11 | R2:사용자 WSL shutdown 뒤 reconnect·소요 시간(이전까지 NOT_RUN) |
| 12 | 방해 금지 알림 센터 기록·해제 후 popup/taskbar |
| 13 | 비내부 WSL 배포판2개 이상 선택 화면; 하나뿐이면 해당 없음 |

- [ ] 승인된 main SHA에 주석 태그 `v1.0.0`과 검토한 release notes를 만들고 push한다. prod setup 하나·digest·source·release≤40분·다운로드를 확인한다.
- [ ] 공개 installer 새 설치1회는 승인된 독립 Windows 환경에서 확인한다. 다음 patch가 아직 없으면 “실제 공개버전 간 업데이트”는 NOT_RUN으로 두며 fixture/accept updater 수용과 구분한다. patch 출시 때 실제 업데이트1회 확인한다.
- [ ] 최종 공개 결과·SC/미결·소유 자원 정리를 PROGRESS에 기록한다. clean/merged 확인 후 worktree→prune→local/remote branch 정리, archive/cache/사용자 자료 보존.

## 17. §8 후속 항목 판정·남은 결정

| 항목 | 이번 계획 판정 |
|---|---|
| #645 credential root4367201426 | 이미 수정·머지됨. 회귀 유지, 새 결함으로 재개하지 않음 |
| #645 HTTP authorize4367201424 | S6-1b.3 포함 |
| #644 quit finish/new-window | S6-3b bounded reproduction·updater 종료 계약에 필요한 보정 포함; 불필요함이 증명된 경우만 이유를 남겨 defer |
| #642 Windows readiness PID 귀속 | S6-4 포함, 직접/native 모두 확인. S6-4a `313a6095` 구현·독립 APPROVE, 실제 Windows 실행·동등성은 §14 수용까지 NOT_RUN |
| custom vault support masking | S6-1b.4 포함 |
| Windows 파일명 casing/cfg 과거 수정 | 종결. 기존 회귀·Windows CI로 유지 |
| 과거 native·SC gaps | S6-4 재수행 목록, 환경 부재·사용자 R1/R2는 정확히 NOT_RUN |
| 세 dependency advisory | S6-1c 호환 조사·좁은 업데이트, exploit 추정 금지 |
| 자동 리뷰 OFF | 해소(사용자 결정 2026-10-26): S6 PR은 독립 검토로 대체, 자동 리뷰 NOT_RUN(비활성화), CONVENTIONS §8 상한·필수 CI3·보호 규칙 유지. #646 예외를 포괄 승인으로 확장하지 않음 |
| 공개·태그 | 구현 지시와 별개로 승인 없음. S6-6c에서만 명시 승인 |

**S6-1 계획의 완료 조건:** 네 새 발견과 기존 요구의 담당 과제·의존·실패 시험·소유 자원·검토/커밋·PR/공개 경계가 빠짐없이 있고, 별도 계획 검토를 통과한다. 계획 작성은 어떤 제품 수정이나 성공 기준 달성을 뜻하지 않는다.
