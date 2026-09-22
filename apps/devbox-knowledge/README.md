# Devbox Knowledge

v0.8의 네 사용자 제품 중 하나다. Notes·Daily·Activity·Search를 통합한다.
B01~B08의 owner 수용은 완료됐고, 최종 B09 후보·공개 결과는 [#541](https://github.com/jihoon22-lee/devbox/issues/541)에 기록한다.

## 실행과 개발

- `pnpm --filter devbox-knowledge dev`: 명시적으로 표시된 browser fixture.
- Windows의 `pnpm --filter devbox-knowledge tauri dev`: 실제 native 제품.
- Browser `?route=notes`와 debug `--route=notes`는 개발용 route 선택이다.
  release 권한을 부여하는 옵션이 아니며 native host가 caller·session·route를 다시 검사한다.
- 제품 identity는 `com.devbox.v08.knowledge`, 데이터는 installation별 namespace다.
  시작만으로 legacy source를 초기화하거나 원본 경로에 새 데이터를 쓰지 않는다.
- 배포는 Suite installer 또는 제품 ZIP 전체를 사용한다. [설치·이전·복구 안내](../../docs/windows-guide.md).

## 유지하는 계약

- Notes/Daily는 원본 vault·Markdown·assets·templates를 보존한다. 활성 vault 변경은 별도 검토한다.
- Activity의 day boundary/timezone/source provenance를 유지하고 선택한 요약만 preview 후 삽입한다.
- Search는 파일/노트 source별 bounded query·timeout·partial·cancel·freshness와 saved query/root/
  exclusion을 지원한다. 사용자 설정은 파생 index DB와 별도로 이전한다.
- 세 legacy source의 consistent snapshot을 새 generation에 반영한다. source ID와 import receipt는
  재실행에서 destination 편집·삭제를 보존한다. corrupt/future pointer를 자동 초기화하지 않는다.
- 신규 store 시작과 원본 import를 분리한다. 이전의 collection consent·살아 있는 작업은
  자동 재개하지 않으며 API/Activity artifact는 source authority와 명시적 review로 수신한다.
- Notes는 읽은 파일의 native revision으로 조건부 저장한다. 외부 수정·삭제·교체가 감지되면
  초안을 유지하고 디스크와 비교, 재읽기 또는 검토한 revision에 대한 명시적 덮어쓰기를 제공한다.
  watcher 알림이 늦어도 저장 시 다시 확인한다. 원자적 파일 교체와 충돌 검사는 별도 보장이다.
- 창 닫기·트레이 종료·정상 exit는 공통 미저장 확인을 거친다. 저장 실패·충돌 시 종료하지
  않으며 저장/버리기/취소를 선택할 수 있다. close-to-tray는 숨기기만 하고 수집 동의를 바꾸지 않는다.
  강제 종료·전원 손실 후 메모리의 미저장 내용 복구는 보장하지 않으며 미저장 메모리를 상시 평문 백업하지 않는다.
- 이전 활성화 의도를 기록한 뒤 저장소 포인터를 쓰기 전에 중단돼도 재개·되돌리기·취소할 수 있다.

## 구현과 근거

기능 코드는 이 제품 host와 `packages/` feature UI, 이름을 가진 `crates/` engine에 있다.
기존 15개 앱의 standalone shell·Tauri bootstrap은 제거했다. 현재 소유권은
[projects](../../docs/projects.md), 기능/데이터 및 R01–R26/S01–S08 대응은
[acceptance trace](../../docs/v0.8-acceptance.md)를 참조한다.

[이전 개발 단계의 상세 README](../../docs/history/v0.8-development/knowledge.md)는 당시 구현
순서·결정의 역사적 기록이다. 그 문서의 hidden/pending 상태를 현재 배포 상태로 해석하지 않는다.
제품 실행/installer 근거와 deterministic fixture·browser·physical device 검사는 구분한다.

### 화면 오류와 비동기 응답 복구

기능별 렌더·lazy import 실패는 해당 화면에서 격리한다. 제품의 노트 세션과 종료 확인은
화면 오류 경계 밖에서 유지되며, 노트 화면 실패 시 메모리의 현재 내용과 조건부 저장을
복구 화면에서 제공한다. 복구는 실행 중인 프로세스의 메모리 보존이며 자동 평문 백업이나
강제 종료 복구를 추가하지 않는다. 재시도해도 청크를 불러오지 못하면 복구 화면을 유지한다.
미리보기와 디스크 inspect는 오래된 응답을 무효화한다. 일반 Markdown 링크는 URL 경로와
fragment를 나누어 한 번 디코딩하고 vault 경계 안에서 연 뒤 heading anchor로 이동한다.

초안 재생성 여부는 native `draft_stale` 코드로 결정한다. 한국어 표시 문구 변경이나
일반 I/O 실패가 초안을 만료된 것으로 바꾸지 않는다.

삭제 완료는 승인 당시의 문서·편집·열기·저장 세대가 유지된 경우에만 편집 buffer를 비운다.
삭제 대기 중 문서 전환·재열기·추가 편집·저장이 있으면 현재 내용을 유지한다. 파일 삭제와
메모리 내용 보존은 별개이며, 보존된 노트는 디스크 상태를 확인한 뒤 저장 여부를 결정한다.

### 동시 저장과 복구 파일

신규 노트는 완성된 파일을 no-clobber 방식으로 게시한다. 기존 노트 저장은 사전 revision
검사에 더해 실제 교체에서 밀려난 파일을 보존하고 다시 비교한다. Linux는 atomic exchange,
Windows는 원본 보존 옵션을 사용한 ReplaceFileW를 사용하며 지원하지 않는 파일시스템에서
일반 덮어쓰기로 대체하지 않는다. 이는 모든 외부 writer와의 전역 트랜잭션을 보장하지 않는다.
검사 이후 외부 변경이 들어오면 새 내용이 이미 반영됐을 수 있으며, 이 경우 성공으로 숨기지
않고 외부 원본과 메모리 초안을 보존해 충돌을 알린다. 자동 rollback은 수행하지 않는다.

충돌·확인 실패 시 같은 폴더의 `.devbox-save-*` 복구 디렉터리 위치를 표시한다.
`previous.md`와 `submitted.md` 중 존재하는 파일 및 현재 노트를 비교한 뒤 사용자가 복구한다.
Windows의 게시 중간 실패에서는 같은 이름에 `.previous.md` 또는 `.submitted.md`가 붙은
복구 디렉터리 옆의 파일도 보존한다.
교체 실패의 중간 상태에서는 파일 이름만으로 내용을 단정하지 않는다. 정상 완료 시 임시
디렉터리는 정리한다. 이 디렉터리는 Unix 0700, Windows 소유자/SYSTEM 전용 DACL로 만든다.
Unix 신규 파일은 0600이며 기존 mode는 게시 전에 보존한다. Windows 갱신은 원본 DACL을
보존하고 권한 병합 실패를 무시하지 않는다. Windows 신규 노트는 소유자/SYSTEM 전용이다.
프로세스 중단 시 남은 복구 파일을 자동으로 제거하지 않는다. 전원 손실 후 복구 보장은
추가하지 않으며 동기화 실패는 반영 후 경고로 구분한다.

반영 상태 불명확·충돌은 초안을 dirty 상태로 유지하고 종료 저장을 성공 처리하지 않는다.
반영이 확인된 저장의 index/동기화/정리 경고는 새 revision과 함께 전달한다. 생성·삭제 후
index commit 실패도 이미 반영된 작업임을 표시하므로 단순 실패로 보고 재실행하지 않는다.

WSL UNC 저장은 Windows DACL API를 사용하지 않는다. Knowledge와 Workspace가 같은 소스에서
빌드한 정적 Linux helper를 각각 포함하며, Knowledge host는 빌드에 고정한 해시·크기를
검증하고 파일/조상 핸들을 고정한 뒤 파일 전용 프로토콜만 호출한다. 배포판 이름 대신
등록 GUID를 사용하고 셸·배포판 프로그램·Workspace 작업 실행 권한은 전달하지 않는다.
Linux에서 임시 폴더 0700, 신규 파일 0600, 기존 mode 보존을 적용한다. WSL2에서는 원자적
exchange를 사용한다. renameat2가 없는 WSL1 wslfs에서는 기존 객체를 보존 위치로 이동한 뒤
no-clobber link로 게시하므로 짧게 경로가 없는 구간이 있을 수 있다. 이 구간의 외부 생성은
덮어쓰지 않고 양쪽 파일과 초안을 보존한다. helper는 입력·파일 작업을 포함한 5초 watchdog으로 종료를 요청하고,
host는 8초 대기 예산 안에 완료를 확인하지 못하면 반영 상태 불명확으로 처리한다.
커널 I/O가 멈춘 경우 실제 종료까지 확인된 것으로 취급하지 않는다. 폴더 동기화 실패는
반영 후 경고다. 기존 서비스·방화벽을 변경하거나 배포판에 도구를 설치하지 않는다.
