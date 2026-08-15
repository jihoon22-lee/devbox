# life-log — 자동 Life Log

하루의 PC·Git·파일 활동을 자동으로 모아 일일 로그와 통계를 만드는 앱. 활동 추적(activity-timeline)을 흡수해 별도 데이터 소스 설정이 필요 없다.
산출물: `LifeLog.exe` (`apps/life-log`).

## 주요 기능

- **일일 요약** — PC 사용시간, 앱별 사용, git 커밋 수, 생성 파일 수, 노트 수
- **캘린더 뷰** — 날짜 선택·이동, 일별 활동 타임라인
- **기간 통계** — 주/월 사용량 차트, 앱 순위, 커밋 트렌드
- **git 프로젝트 연동** — git 경로 등록으로 커밋 집계 (프로젝트 설정은 Workbench로 이관)

## 기술

- 백그라운드 폴러·세션 추적 → SQLite → React
- integration snapshot 계약(`crates/integration`) — 외부 DB 직접 조회 없음

## 데이터

- `%LOCALAPPDATA%\com.devbox.lifelog\data.db` — 활동 세션 + 설정

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

상세 계획: [PLAN.md](./PLAN.md)
