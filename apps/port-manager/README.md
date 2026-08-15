# port-manager — Port & Process Manager

현재 PC에서 사용 중인 TCP/UDP 포트와 연결된 프로세스를 GUI로 조회·종료하는 앱.
산출물: `PortManager.exe` (`apps/port-manager`).

## 주요 기능

- **포트 목록** — proto / port / state(LISTEN·ESTABLISHED 등) / PID / 프로세스명
- **검색·필터** — 포트 번호·프로세스명 부분 일치, TCP/UDP·state 필터
- **프로세스 종료(Kill)** — PID 종료, 권한 부족 시 안내 메시지
- **Open in browser** — `http://localhost:<port>` 열기, 정보 복사
- **새로고침** — 수동 + 단축키 (F5, Ctrl+R)

## 기술

- Windows `netstat -ano` 출력 파싱 (한국어 CP949 등 OEM 코드페이지 처리)
- 공용 크레이트 `crates/process` 사용
- 한국어 Windows 콘솔 창 깜빡임 없이 자식 프로세스 실행

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

