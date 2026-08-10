# activity-timeline — PC Activity Timeline

PC에서 사용자가 무엇을 했는지 자동 기록하고 타임라인·통계로 보여주는 앱. 백그라운드 상시 실행이 핵심 난제.
산출물: `ActivityTimeline.exe`. 모노레포 위치: `devbox/apps/activity-timeline`.

## 1. 목표
- 활성 창(foreground window)의 앱·제목·시간을 자동 기록
- 하루를 시간순 타임라인으로, 기간별 앱 사용 통계로 표시
- 창을 닫아도 계속 동작 (트레이 아이콘 상시 실행)
- 이후 life-log의 핵심 데이터 소스

## 2. 핵심 기능

### MVP (v1)
| 기능 | 설명 |
|---|---|
| 활성창 감지 | `windows` crate `GetForegroundWindow` + `GetWindowText` |
| 세션 기록 | 같은 앱+제목이 지속되면 세션으로 병합 (poll 간격 2~3초) |
| 타임라인 | 날짜별 시간순 그룹, 상단 하루 통계 |
| 앱 통계 | 기간(일/주/월)별 앱 사용시간 차트·리스트 |
| 트레이 | 종료/열기 메뉴, 상시 백그라운드 |

### v2+
- 유휴 감지 (`GetLastInputInfo`, 3분 이상 미입력 → idle 세션 분리)
- 사용자 라벨링 (특정 창 제목 → "업무/공부" 등)
- 앱 아이콘 표시, 주간 리포트

## 3. 기술 설계

### 아키텍처 (핵심)
```
[ 트레이 상시 프로세스 ]
   poller (tokio 간격 루프)
      ↓ GetForegroundWindow + GetWindowText
   세션 병합 로직 (core/sessionizer)
      ↓ INSERT
   SQLite (data.db)
      ↑
   commands (조회: timeline, stats, status)
      ↑ invoke
   React UI
```
- 트레이는 Tauri v2 `tray-icon` + `tauri::WindowEvent::CloseRequested`에서 창 숨기기
- poller는 `tokio::spawn` + `AppHandle`을 상태로 보관
- idle/윈도우 닫힘 등으로 새 프로세스 시작 시 세션 종료 처리

### Rust 모듈
- `core/poller.rs` — 주기 루프, foreground window 조회
- `core/sessionizer.rs` — 연속 창 감지 → 세션 병합 (단위 테스트 대상) → 이후 `crates/activity`로 추출
- `core/idle.rs` — v2 유휴 감지
- `db.rs` — SQLite 초기화 + 마이그레이션 (`rusqlite` bundled) → 이후 `crates/database`로 추출
- `commands/tracking.rs` — `start_tracking()`, `stop_tracking()`, `is_tracking()`
- `commands/queries.rs` — `get_timeline(date)`, `get_app_stats(start, end)`, `get_sessions(date)`

### DB 스키마
```sql
CREATE TABLE sessions (
  id INTEGER PRIMARY KEY,
  app TEXT NOT NULL,          -- 프로세스명 (예: chrome.exe)
  title TEXT,                 -- 창 제목 (예: GitHub)
  start_ts INTEGER NOT NULL,  -- epoch ms
  end_ts INTEGER,
  duration_ms INTEGER
);
CREATE INDEX idx_sessions_start ON sessions(start_ts);
```

### 데이터 모델
- `Session { id, app, title, start_ts, end_ts, duration_ms }`
- `DayTimeline { date, sessions: Session[], totals: AppTotal[] }`
- `AppTotal { app, duration_ms, sessions_count }`

## 4. UI 설계
```
[ActivityTimeline]  2026-08-10   [◀] [▶]  오늘    [● 추적 중]
하루 통계: VS Code 2h48m · Chrome 2h13m · Terminal 1h24m
────────────────────────────────────────────
 09:22  Chrome   GitHub
 09:41  VS Code  FamilyCard
 10:08  Terminal Ubuntu
────────────────────────────────────────────
기간 통계 탭: [7일] [30일] → 앱별 막대/도넛 차트 (recharts)
```
- 탭: Timeline / Stats
- 트레이 메뉴: "열기", "추적 중지", "종료"

## 5. 구현 단계
1. 스캐폴드 + 트레이 아이콘 + 창 닫기 시 숨김
2. `GetForegroundWindow` 조회 command + 데모 확인
3. poller 루프 + 세션 병합 로직 + 단위 테스트
4. SQLite 스키마 + 기록 저장
5. timeline/stats 조회 command
6. Timeline UI + 날짜 네비게이션
7. Stats 탭 (차트) + 유휴 감지(v2)
8. 시작 시 자동 실행 옵션, Windows 빌드 검증

## 6. 테스트
- Rust: sessionizer 병합 규칙 (연속/전환/공백) 테스트, poller는 가짜 window로 목
- 통합: 실제 실행 5분 → 세션 기록 확인, 재실행 시 이어서 기록
- 프론트: 타임라인 그룹핑/통계 계산 vitest

## 7. 확장/연계
- life-log: `%LOCALAPPDATA%\Workbench\activity-timeline\data.db`를 직접 읽어 일일 통계 소스로
- wsl-dashboard: WSL 사용 시간과 대조
- 공통 추출 후보: `crates/database`(마이그레이션 헬퍼), `crates/activity`(세션 집계), 날짜 범위 유틸

## 8. 완료 정의(Done)
- 창을 닫고도 트레이에서 계속 기록, 하루 타임라인·통계 정확
- 세션 병합·유휴 분리 테스트 통과
- Windows 부팅 후 자동 시작(수동 설정)까지 동작
