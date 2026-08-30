# Run Manager workspace task 설계

날짜: 2026-08-30
상태: 구현 기준 (PR1·PR2 구현)
이슈: #486
대상: Run Manager, Workbench, Code Pad, AppLink/integration

## 1. 목적과 이전 문서와의 관계

Run Manager가 프로젝트의 `.vscode/tasks.json`을 읽어 사용자가 검토한 task만 실행한다.
import 자체는 실행이나 승인이 아니며, 실행 권한은 프로젝트 디렉터리의 filesystem identity와
source revision에 묶는다. 실행 직전에 같은 source를 다시 검증하고, 변경됐으면 기존 승인을
무효화한다.

이 문서는 다음의 기존 제외 문구를 #486 범위에서만 대체한다.

- `apps/run-manager/README.md`의 “VS Code tasks.json parsing은 후보가 아님”
- `2026-08-12-run-manager-design.md`의 generic DAG/task workflow 제외
- `apps/workbench/README.md`의 Run Manager lifecycle 연계 제외

기존 cron/service, owner/attempt CAS, Windows Job Object, WSL process-group, DPAPI 환경변수,
bounded log 계약은 그대로 유지한다. VS Code extension host 자체를 구현하거나 extension task를
실행하는 것은 계속 범위 밖이다.

## 2. 사용자 흐름

### 2.1 import와 승인

1. 사용자가 프로젝트 루트와 실행 대상(Windows 또는 WSL 배포판)을 고른다.
2. Run Manager가 프로젝트 바로 아래 `.vscode/tasks.json` 하나를 native/offline으로 읽는다.
3. preview는 적용된 OS override, task type, command/argv, cwd, 환경 키, dependency와 차단 사유를
   보여 준다. 환경 값은 보여 주거나 가져오지 않는다.
4. 사용자가 지원되는 항목을 골라 import한다. 저장된 항목은 항상 disabled + untrusted다.
5. 사용자가 현재 revision을 별도로 승인한다. process task는 source 승인을 사용하고, shell task는
   현재 revision에 대한 `execute-shell-tasks` 별도 위험 확인을 한 번 더 요구한다.
6. Run/enable 직전과 실제 spawn 직전에 source를 다시 읽는다. identity/revision이 달라졌으면 같은
   source의 승인을 지우고 아직 시작하지 않은 task를 disabled로 만든다.

### 2.2 실행과 중지

- process task는 executable과 argv 경계를 보존하며 shell parser를 통과하지 않는다.
- shell task만 기존 shell adapter를 사용하고 UI에 shell 경계를 명시한다.
- 모든 실행은 기존 durable run claim과 Run Manager 소유 process-tree handle을 사용한다.
- stop은 해당 run/operation이 소유한 handle만 종료한다. PID 문자열이나 외부 process 탐색 결과로
  종료 대상을 추측하지 않는다.
- source 변경은 이미 실행 중인 tree를 자동 종료하지 않는다. 다음 실행을 막고 사용자에게 다시
  preview/승인하도록 안내한다. dependency operation을 중지할 때도 operation이 기록한 exact
  child run만 대상으로 한다.

## 3. source와 trust 경계

### 3.1 파일 경계

- root는 absolute, UTF-8 표시 가능, 4 KiB 이하이고 directory여야 한다.
- root, `.vscode`, `tasks.json`의 symlink/reparse point와 parent traversal을 거부한다.
- 파일은 512 KiB 이하이며 open handle의 identity/metadata를 read 전후 다시 비교한다.
- 한 preview는 5초, task 128개, task당 argv 128개, 문자열 16 KiB, 전체 argv 64 KiB로 제한한다.
- shell, package manager, Cargo, network, VS Code 또는 extension executable을 preview 중 실행하지
  않는다.

### 3.2 opaque identity와 revision

source key에는 다음을 포함한다.

- canonical project filesystem identity의 opaque digest
- 안전한 canonical display root
- `.vscode/tasks.json` object identity와 원문 byte digest
- 선택한 target kind와 WSL distro
- parser/projection schema version

경로 자체는 revision에 직렬화하지 않는다. revision은 lower-case SHA-256 hex이고 command, argv,
환경 값 또는 source text를 포함한 오류를 만들지 않는다.

승인 상태는 저장된 current revision에 대한 `trusted` bit이며, trust 명령의 revision CAS와 revision
갱신 시 trust 초기화로 exact-revision 불변식을 유지한다. shell task는 같은 source revision에
대한 별도 `shell_trusted` bit를 사용한다. source 재검증 실패는 해당 source의 두 승인과 enable을
한 transaction에서 무효화한다.

### 3.3 TOCTOU 규칙

- preview 결과를 apply할 때 source를 한 번 다시 읽고 같은 revision인지 확인한다.
- trust할 때 다시 읽은 한 snapshot을 승인에 사용한다.
- UI의 Run/enable 명령과 adapter spawn이 각각 재검증한다.
- spawn은 DB에 저장된 승인된 argv를 사용한다. 검증 직후 파일이 바뀌더라도 바뀐 파일의 새
  command를 실행하지 않는다.

## 4. JSONC와 task projection

### 4.1 JSONC

bounded scanner가 string escape를 보존하면서 line/block comment와 trailing comma만 제거한 뒤
strict JSON으로 파싱한다. unterminated string/comment, control byte, duplicate semantic label,
과도한 nesting은 전체 source 오류다. root `version`은 정확히 `2.0.0`, `tasks`는 array여야 한다.

### 4.2 OS override

명시적 실행 대상이 Windows면 `windows`, WSL이면 `linux` override를 base task 위에 병합한다.
다른 OS block은 실행 후보에 영향을 주지 않지만 preview에 존재 여부를 표시한다. 다음 필드만
projection한다.

- `label`, `type`, `command`, `args`
- `options.cwd`, `options.env`의 key
- `dependsOn`, `dependsOrder`
- `problemMatcher`
- 동일 필드의 선택된 OS override

`options.shell`, `runOptions`, `presentation`, `group`, `detail` 등은 실행 권한을 넓히지 않는다.
실행 의미가 있는 알 수 없는 구조는 무시하지 않고 해당 task를 blocked로 만든다.

### 4.3 변수

허용 변수는 target에 맞는 separator로 한 번만 치환한다.

- `${workspaceFolder}`
- `${workspaceFolderBasename}`
- `${pathSeparator}`
- `${/}`

`${env:...}`, `${config:...}`, `${command:...}`, `${input:...}`, active editor/file/selection,
workspace-name selector와 알 수 없는 `${...}`는 blocked다. 변수 치환 결과의 cwd는 canonical project
root 내부여야 한다. `options.env`는 key만 preview하고 값은 import하지 않는다. 필요한 값은 기존
Run Manager의 DPAPI 환경변수 편집 흐름에서 사용자가 다시 입력한다.

### 4.4 task type

- `process`: command string + string argv만 허용한다. quote-object는 blocked다.
- `shell`: 일반 source trust와 분리된 별도 위험 확인 뒤 허용한다.
- `$`로 시작하거나 `process`/`shell`이 아닌 extension type: blocked다.
- `${command:...}`/`${input:...}`를 간접 호출하는 task: 항상 blocked다.

blocked 항목도 preview에 label과 고정 reason code를 표시하되 command/source text를 오류나
integration snapshot으로 복제하지 않는다.

## 5. persistence

SQLite schema v3는 기존 `jobs`를 변경하지 않고 side table을 추가한다.

### 5.1 `workspace_task_sources`

- source id와 project identity
- canonical display root와 고정 task file 상대 경로
- target kind/distro
- current revision과 exact-revision `trusted` bit
- created/updated timestamps

### 5.2 `workspace_tasks`

- 내부 task id, unique `job_id` foreign key와 source id
- source index, label, task kind
- argv JSON, 환경 key 이름 JSON, dependency와 `dependsOrder`, 지원되는 problem matcher,
  적용 OS override, availability

기존 `jobs.command`와 `jobs.cwd`에는 source가 관리하는 executable과 resolved cwd를 저장하고,
source target/distro는 source table이 권위다. 실제 process 실행은 이 필드와 side table argv를
하나의 managed projection으로 검증한다. side table이 손상됐거나 JSON bound를 벗어나면 shell로
fallback하지 않는다. PR2는 dependency와 explicit problem matcher의 bounded normalized payload를
같은 projection에 저장하지만, source 재검증 시 원본과 일치하지 않으면 실행 권한을 부여하지 않는다.

### 5.3 `workspace_task_operations`와 receipts

dependency 실행은 기존 `jobs`/`runs`를 대체하지 않고 schema v4 side table에 durable operation과
child run 연결을 기록한다. operation에는 root job, source/revision, fail-fast 여부, 상태와
timestamp를, child에는 layer/sequence, exact `run_id`, 상태와 고정 failure code를 저장한다.
동일 root에는 queued/running/stopping operation을 동시에 둘 수 없다. task-control 요청은
request id를 primary key로 하는 receipt에 action, expected revision, 상태, optional owned
operation id, fixed failure code와 timestamp를 기록한다. receipt의 renderer/snapshot DTO에는
expected revision을 다시 노출하지 않는다.

active operation의 root 또는 dependency job 삭제는 transaction 안에서 거부한다. 완료된
operation의 member job을 삭제하면 operation·child·연결 receipt를 함께 제거해 부분 history나
operation provenance가 없는 receipt를 남기지 않는다.

import batch는 source와 선택 task/job을 한 immediate transaction으로 넣는다. 모든 job은
`enabled=0`, fixed manual-review cron, env ciphertext 없음이다. 같은 source의 기존 label을 다시
가져오면 job identity와 history를 보존한 채 managed projection을 갱신하고, 새 preview에서 빠진
기존 항목은 unavailable+disabled로 바꾼다. 다른 일반 job과 정규화된 name/cwd가 충돌하면 skip한다.

## 6. platform 실행

### 6.1 Windows process

기존 suspended `CreateProcessW` → Job Object assign → resume 순서를 유지한다. process mode는
Win32 argv quoting으로 command line을 만들고 `cmd.exe /C`를 사용하지 않는다. stdin은 EOF,
stdout/stderr는 기존 bounded rotating log에 연결한다.

### 6.2 WSL process

`wsl.exe --exec` 인자는 모두 argv로 전달한다. fixed supervisor는 handshake와 process-group
cleanup만 담고 `"$@"`로 executable/argv를 실행한다. `setsid --wait`가 새 session을 만들면서
Windows-side wrapper의 output/exit 수명도 유지한다. 사용자 command/argv를 `bash -lc` script에
interpolate하지 않는다. distro/cwd와 PID/PGID/SID/marker 검증은 기존 규칙을 유지한다.

### 6.3 실패 코드

renderer와 history에는 다음 고정 코드만 노출한다.

- `workspace-task-source-untrusted`
- `workspace-task-unavailable`
- `workspace-task-source-changed`
- `workspace-task-source-unavailable`
- `workspace-task-configuration-invalid`
- 기존 `spawn-failed`, `wsl-unavailable`, `termination-timeout`

경로, command, argv, source text, 환경 값은 오류에 반향하지 않는다.

## 7. DAG, matcher, Workbench 연계

이 절은 #486 PR2에서 구현한다. PR1의 import projection·source trust 경계는 그대로 유지하며,
아래 기능도 같은 exact revision과 bounded execution 경계를 통과해야 한다.

### 7.1 DAG

- dependency는 같은 source의 exact label만 참조한다.
- missing/duplicate label, self edge, cycle, 128 node/512 edge 초과를 import 전에 차단한다.
- 기본은 parallel, `dependsOrder: sequence`만 sequential이다.
- 선택은 dependency closure를 포함해야 한다.
- operation은 root와 시작한 child run id를 durable하게 기록한다.
- dependency 실패 시 아직 시작하지 않은 downstream은 skipped이고 독립 parallel branch는 명시된
  fail-fast 정책에 따라 중지 요청을 받는다.
- operation은 먼저 queued/pending 상태를 저장하고 layer barrier를 지킨다. launch reservation과
  child attach가 끝나기 전에는 stop/recovery가 parent를 terminal로 앞당기지 않는다. 명시적
  stop은 30초 bounded settle 동안 job별 lifecycle lock 안에서 expected run id와 현재 active
  run id를 다시 대조해 exact active run만 중지한다. 확인되지 않은 cleanup은 `stopping`으로
  남겨 다음 recovery에서 다시 처리한다.

### 7.2 problem matcher

explicit matcher object 중 file/line/column/message/severity group이 명확한 bounded regular
expression만 지원한다. VS Code/extension 제공 `$matcher` 이름은 extension host 없이는 의미를
재현할 수 없어 blocked 또는 unsupported로 표시한다.

한 terminal child run/stream당 최대 4 MiB, 50,000 line, 500 diagnostic을 분석한다. 상한 도달
여부는 `truncated`로 반환한다. 파일은 project root 내부의 canonical regular file로 재검증한다.
Code Pad 이동은 AppLink `Path`의 path + 1-based line/column만 사용한다. log text와 matcher
capture는 handoff 저장소에 넣지 않는다. 실행 중 child에는 diagnostics를 요청할 수 없다.

### 7.3 Workbench 요청과 receipt

Workbench는 raw command/path를 보내지 않는다. 요청은 `task-control/v1` typed one-time handoff로
전달되며 opaque task id, `start|stop`, random request id, expected source revision만 가진다. Run
Manager는 claim lease 동안 현재 DB revision을 확인하고 자기 창에서 사용자 확인을 받은 뒤
수행한다. Start는 source를 다시 검증한 뒤 dependency closure operation을 만들고, Stop은 새
실행 입력을 읽지 않고 해당 task가 root인 active operation만 대상으로 한다. 구버전 Run Manager는
앱을 열기만 하고 자동 실행하지 않는 방향으로 degrade한다.

receipt는 request id, task id, action, accepted/rejected/started/stopped/failed, owned operation의
opaque id, timestamp와 고정 failure code만 가진다. `task-control-receipts` named snapshot도
같은 redaction 경계를 사용한다. Workbench는 request/task/action correlator가 모두 일치하는
receipt만 해당 요청의 결과로 표시한다. stop은 receipt에 연결된 Run Manager-owned tree 외에는
적용하지 않는다.

operation 생성·executor 종료·명시적 stop 뒤에는 redacted `workspace-tasks` named snapshot을
즉시 다시 발행한다. 따라서 Workbench가 새로고침할 때 주기 발행을 기다리지 않고 현재
`operationActive`를 관찰하며, snapshot에는 command/path/env/PID를 추가하지 않는다.

### 7.4 재시작 복구

Run Manager 시작 시 DB에 남아 있던 queued/running/stopping operation은 interrupted failure code와
함께 stopping으로 전환한다. scheduler는 기존 stale-run recovery가 각 run의 owner/attempt와
platform identity를 정리한 뒤 interrupted operation을 재검토한다. launching/running child가
더 이상 active가 아니면 terminal child 상태로 reconcile하고 pending child를 skipped로 바꾸며,
모든 child가 terminal일 때만 parent를 failed로 마무리한다. 아직 active이거나 cleanup 실패가
확인되면 parent는 stopping으로 보존하고 다음 tick에서 재시도한다.

## 8. 프론트엔드

- Import dialog에 `VS Code tasks` 모드를 추가하고 target kind/distro를 명시한다.
- ready와 blocked를 함께 보여 주며 blocked reason, 적용 OS override, env key 미가져옴을 설명한다.
- apply 성공 후 “비활성 초안”과 “source 승인 필요”를 분리해서 표시한다.
- job card는 `process|shell`, trusted/untrusted/source-changed, project basename을 표시한다.
- trust와 shell-risk 확인은 keyboard/focus trap/취소 가능한 modal 계약을 따른다.
- untrusted task의 Run/enable은 UI에서 비활성화하되 native 명령도 동일하게 거부한다.
- operation은 root status와 child progress를 polling으로 표시하고, terminal child에 problem
  matcher diagnostics가 있으면 bounded 항목을 보여 준다. diagnostic 선택은 Code Pad의
  project-relative file과 1-based line/column으로만 이동한다.
- Workbench task-control handoff는 Run Manager의 확인 modal에서 label/kind/action/revision만
  보여 준다. 확인 modal의 ESC/거절은 handoff를 ack하고 rejected receipt를 남기며, 확인 중
  lease를 제한적으로 갱신하고 late/mismatched receipt는 무시한다.
- workspace task는 source가 권위이므로 일반 Job Editor의 command/cwd 편집을 막고 재-import 또는
  “일반 작업으로 분리”라는 별도 명시적 흐름 없이는 source 연결을 해제하지 않는다.

## 9. 검증

### 9.1 PR1 pure/core

- JSONC comments/trailing comma/string escape/unterminated cases
- Windows/Linux override와 base merge
- 허용/차단 변수, quote object, extension/input/command task
- root/file identity 교체, symlink/reparse, same-size rewrite, stale preview/trust
- duplicate label, task/argv/size/time bounds
- process argv가 shell string으로 합쳐지지 않는 round-trip
- dependency/background/runOptions 차단과 problem matcher 존재 표시

### 9.2 PR1 storage/scheduler/platform

- v2 → v3 idempotent migration과 rollback
- atomic disabled/untrusted import, corrupt side table fail-closed
- source change가 trust+enable을 함께 무효화
- manual/scheduled 진입점의 동일 재검증
- Windows direct process Job Object와 WSL direct argv group cleanup
- timeout/cancel/stop 후 owned tree residue 없음
- stop이 외부 PID나 다른 operation run에 적용되지 않음

### 9.3 PR1 frontend/integration

- preview/select/apply/trust/error/focus/keyboard
- blocked reason과 OS override 표시
- stale source 후 action disable과 재-preview
- snapshot/AppLink/handoff에 raw path/command/env/log/secret 없음

### 9.4 PR2 추가 검증

- DAG cycle/missing/sequence/parallel projection과 dependency closure
- matcher bounds·root containment·diagnostic → Code Pad path/line/column
- Workbench start/stop confirm, stale request, producer rename/missing, receipt provenance
- durable operation의 launch reservation/child attach, fail-fast sibling ownership, explicit stop
  settle와 stale-run 이후 재시작 recovery
- `task-control/v1` producer/consumer claim·lease·ack·restore, accepted→started/stopped/failed
  receipt CAS와 redacted `workspace-tasks`/`task-control-receipts` snapshot
- v3 → v4 idempotent migration으로 operation·child run·receipt table/index를 추가하고 기존
  jobs/runs와의 호환성을 유지

실제 Windows acceptance에서는 로컬 drive와 `\\wsl$`/`\\wsl.localhost` 프로젝트, 공백·한글·
case-sensitive path, stopped/missing distro, portable/installer를 모두 확인한다.
