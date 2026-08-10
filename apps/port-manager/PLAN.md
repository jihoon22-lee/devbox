# port-manager — Port & Process Manager

현재 PC에서 사용 중인 포트와 프로세스를 GUI로 조회·관리하는 앱. Tauri 시리즈의 첫 프로젝트.
산출물: `PortManager.exe`. 모노레포 위치: `devbox/apps/port-manager`.

## 1. 목표
- TCP/UDP 포트 목록, 연결된 프로세스(PID/이름/경로) 확인
- 프로세스 종료(Kill), localhost URL 열기, 정보 복사
- 검색·필터로 원하는 포트/프로세스 빠르게 찾기

## 2. 핵심 기능

### MVP (v1)
| 기능 | 설명 |
|---|---|
| 포트 목록 | proto, port, state, pid, process name |
| 검색 | 포트 번호/프로세스명 부분 일치 |
| 필터 | TCP/UDP, LISTEN/ESTABLISHED 등 state |
| Kill | PID 종료 (권한 실패 시 메시지) |
| Open in browser | `http://localhost:<port>` 열기 |
| 새로고침 | 수동 + 단축키 (F5, Ctrl+R) |

### v2+
- 프로세스 상세 패널 (경로, 실행 시각, 메모리, CPU)
- 즐겨찾는 포트 핀 (config.json 저장)
- 포트 점유 앱 아이콘/명령줄 표시
- 우클릭 컨텍스트 메뉴 (복사, 종료, 경로 열기)
- WSL 포트 연동 (v2 후 wsl-dashboard와 연결)

## 3. 기술 설계

### 데이터 흐름
```
React (테이블/검색/필터)
  ↓ invoke('list_ports') 등
Rust commands
  ↓
core/ (netstat -ano 파싱)  →  이후 crates/process로 추출
  ↓
Windows OS
```

### Rust 모듈
- `src-tauri/src/commands/ports.rs` — `list_ports()`, `kill_process(pid)`, `open_browser(url)`, `get_process_info(pid)`
- `src-tauri/src/core/` (앱 로컬 순수 로직, OS 의존 없음, WSL에서 `cargo test` 가능):
  - `core/models.rs` — `PortInfo { proto, local_addr, port, state, pid }`
  - `core/netstat.rs` — `netstat -ano` 출력 파서 (`parse_netstat_output`, `extract_port`)
- `PortRow` (src-tauri) — `PortInfo` + `process_name`(sysinfo 매핑)을 프론트로 전달
- 두 번째 앱(wsl-dashboard 등)에서 프로세스/포트 코드가 필요해지면
  `crates/process`로 추출하고 Cargo workspace 멤버로 추가한다

### 구현 구조 (실제 배치)
```
apps/port-manager/
├ src-tauri/             # Tauri 앱 (명령 계층 + Windows 빌드 대상)
│  ├ .cargo/config.toml  # (WSL 개발 시) target-dir → Linux 네이티브 경로
│  └ src/{lib.rs, commands/ports.rs, core/}
└ src/                   # React 프론트 (mock 모드 포함)
```
- WSL 개발: `cargo test`(core) / `cargo check`(src-tauri)로 컴파일 검증
- Windows 빌드: `pnpm tauri dev/build` (설치 환경 구축 후)

### 파싱 전략
- `netstat -ano` (Windows 내장) 출력을 정규식으로 파싱
- 별도 크레이트 의존 없이 시작 → 필요 시 `netstat2`/`Get-NetTCPConnection`로 교체 가능하게 인터페이스 분리
- PID → 프로세스명은 `sysinfo`로 1회 스냅샷 후 매핑

## 4. UI 설계
```
Ports                    [Search...]      [Refresh]
+---------+------+--------+------+--------+-----------+
| PROTO   | PORT | STATE  | PID  | PROCESS| ACTION    |
| TCP     | 3000 | LISTEN | 1234 | node   | Kill Open |
| TCP     | 5432 | LISTEN | 5678 | postgre| Kill Open |
+---------+------+--------+------+--------+-----------+
상단 상태바: 전체 포트 수 / LISTEN 수 / 검색 결과 수
```
- 단일 페이지 + 하단 프로세스 상세 패널(선택 시)
- 필터 칩: All / TCP / UDP / LISTEN / ESTABLISHED

## 5. 구현 단계
1. 스캐폴드 + 공통 규약 적용 (Tailwind, 아이콘, 구조)
2. `netstat -ano` 파서 + 단위 테스트 (`cargo test`)
3. `list_ports` command + API 래퍼 + 기본 테이블
4. 검색/필터 구현
5. Kill / Open in browser / 상세 패널
6. 새로고침, 로딩·에러 상태, 즐겨찾기(config) 마무리
7. Windows 빌드 + 설치 패키지 검증

## 6. 테스트
- Rust: `netstat` 파서 유닛 테스트 (샘플 출력 픽스처), `kill_process`는 mock PID
- 프론트: 검색/필터 로직 (vitest), 컴포넌트 스모크 테스트

## 7. 확장/연계
- wsl-dashboard: 포트 정보 + WSL 프로세스 표시 (데이터 소스 공유, `crates/process` 추출)
- activity-timeline: 프로세스 관점의 사용 기록과 대조
- 공통 추출 후보: `crates/process`, `packages/ui`(테이블 컴포넌트·invoke 래퍼)

## 8. 완료 정의(Done)
- 포트/프로세스 조회·검색·필터·Kill·브라우저 열기 모두 동작
- `cargo clippy` 경고 0, `cargo test` 통과, Windows 배포 빌드 성공
