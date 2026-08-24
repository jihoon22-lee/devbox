# life-log — 자동 Life Log

하루의 PC·Git·파일 활동을 자동으로 모아 일일 로그와 통계를 만드는 앱. 활동 추적(activity-timeline)을 흡수해 별도 데이터 소스 설정이 필요 없다.
산출물: `LifeLog.exe` (`apps/life-log`).

## 주요 기능

- **일일 요약** — PC 사용시간, 앱별 사용, git 커밋 수, 생성 파일 수, 노트 수
- **캘린더 뷰** — 날짜 선택·이동, 일별 활동 타임라인
- **기간 통계** — 주/월 사용량 차트, 앱 순위, 커밋 트렌드
- **git 프로젝트 연동** — git 경로 등록으로 커밋 집계
- **프로젝트 snapshot** — 등록 프로젝트와 최근 7일 활동의 숫자 요약을 Workbench용 `projects/v1` view로 발행
- **Knowledge 활동 source** — `knowledge-base/activity/v1`의 오늘 작성·수정 수와 최근 수정 시각을 Data Sources에 freshness와 함께 표시

## 기술

- 백그라운드 폴러·세션 추적 → SQLite → React
- `crates/integration`의 자동 발견·검증 API로 모든 snapshot producer를 Data Sources에 표시 — 외부 DB 직접 조회 없음
- Knowledge의 `activity/v1` view는 producer·envelope/view schema, 단일 entry, 불투명 ID 형식·중복·개수 관계를 모두 검증한 뒤에만 사용한다. ID 자체는 frontend로 보내지 않는다
- Knowledge Base가 아직 구버전인 롤링 업그레이드 동안 기존 flat v1 통계도 읽되 `legacySnapshot`으로 구분한다. 손상·schema mismatch는 다른 source를 막지 않으며 producer version·generatedAt·freshness와 안전한 오류를 유지한다
- 시작·프로젝트 변경·60초 주기로 `%LOCALAPPDATA%\devbox\integration\life-log\v1\summary.json`을 원자 교체
- `projects/v1` entry: `path`, `activityWindowStartMs`, `lastActivityAtMs`, `recentSessionCount`, `recentDurationMs`
- snapshot에는 창 제목·앱명·세션 원문·credential을 넣지 않으며, 상대·traversal·device/root 경로는 발행하지 않음

## 데이터

- `%LOCALAPPDATA%\com.devbox.lifelog\data.db` — 활동 세션 + 설정

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`
