# life-log — 자동 Life Log

하루의 PC·Git·파일 활동을 자동으로 모아 일일 로그와 통계를 만들어주는 앱. 데이터를 직접 만들지 않고 다른 앱(activity-timeline·wsl-dashboard·everything-plus·knowledge-base)의 데이터를 집계하는 것이 핵심.
산출물: `LifeLog.exe`. 모노레포 위치: `devbox/apps/life-log`.

## 1. 목표
- 각 앱이 쌓은 데이터를 모아 "오늘 하루"를 요약하는 자동 일일 로그
- PC 사용시간·코딩시간·git 커밋·생성 파일·작성 노트를 한 화면에
- (선택) LLM을 연결해 자동 일기 생성

## 2. 핵심 기능

### MVP (v1)
| 기능 | 설명 |
|---|---|
| 데이터 소스 설정 | 각 앱 DB 경로를 설정 UI로 등록 (기본값은 공통 루트) |
| 일일 요약 | PC 사용시간, 앱별 사용, git 커밋 수, 생성 파일 수, 노트 수 |
| 일일 로그 뷰 | 날짜별 카드 + 활동 타임라인(activity-timeline 데이터) |
| 기간 통계 | 주/월 사용량, 앱 순위, 커밋 트렌드 |

### v2+
- 프로젝트별 활동 집계 (git 경로 매핑 → "오늘은 FamilyCard를 가장 많이 개발")
- 자동 일기 (LLM API 호출, 키 설정·로컬 처리)
- 주간/월간 리포트 내보내기

## 3. 기술 설계

### 데이터 흐름 (읽기 전용 집계)
```
[설정된 소스들]
  activity-timeline/data.db     ─┐
  wsl-dashboard git 로그      ─┼→ 집계(commands/aggregate) → SQLite(생략 가능)
  everything-plus 변경 로그   ─┤    ↓
  knowledge-base 문서/일지    ─┘  React (일일 뷰/통계)
```
- 기본 경로 규약: `%LOCALAPPDATA%\Workbench\<App>\data.db` (CONVENTIONS 참조)
- 집계는 **읽기 전용**: 원본 DB를 조회만 하고 수정하지 않음
- 원본이 없으면 해당 소스는 스킵(설정에서 활성화 토글)

### Rust 모듈
- `commands/sources.rs` — `list_sources()`, `set_source_path(app, path)`, `toggle(app, enabled)`
- `commands/aggregate.rs` — `get_day(date) -> DaySummary`, `get_range(start, end) -> RangeStats`
- `commands/gitlog.rs` — wsl-dashboard git 헬퍼 재사용, 프로젝트 경로별 커밋 수/메시지
- `core/readers/activity_timeline.rs` — activity-timeline DB 조회 (외부 SQLite 읽기)
- `core/readers/everything_plus.rs` — everything-plus 변경 로그 조회 (v2)
- `core/readers/knowledge_base.rs` — knowledge-base 문서 수/일지 (v2)
- `core/git/collect.rs` — 설정된 프로젝트 경로들의 오늘 커밋 수집
- `core/summarize.rs` — DaySummary 계산 로직 (단위 테스트 대상) → 이후 `crates/activity`로 추출
- `commands/ai.rs` — `generate_diary(day) -> String` (v2, 비동기, 키 없으면 비활성)

### 데이터 모델
```rust
struct DaySummary {
  date: String,
  pc_usage: Duration,          // activity-timeline 합계
  app_totals: Vec<AppTotal>,   // activity-timeline
  git: GitDay { projects: Vec<ProjectCommit>, total_commits: u32 },
  files: FileDay { created: u32, modified: u32 },   // v2
  notes: NoteDay { written: u32, projects: Vec<String> }, // v2
  top_project: Option<String>,
}
```
- 자체 DB 없이 시작(집계만), 필요 시 캐시용 SQLite 추가

## 4. UI 설계
```
[LifeLog]  2026-08-10  [◀] [▶]  오늘  [주간] [월간]
  ┌ PC 사용       7h 21m        ┐
  ├ Coding        3h 42m        │  → 최근 7일 차트 탭
  ├ Git commits   14            │     (사용시간/커밋 트렌드)
  ├ 생성한 파일   23             │
  └ 작성한 Notes  4             ┘

  Most active project: FamilyCard
  ── 타임라인 ──────────────
  09:22 Chrome  GitHub
  09:41 VS Code FamilyCard  (activity-timeline 연동)
  ── Git ───────────────────
  FamilyCard  14 commits  (12:03 feat: add port scanner ...)

  [AI 일기 생성]  (v2) → "오늘은 FamilyCard 개발에 가장 많은 시간을..."
```
- 설정: 데이터 소스 경로/토글, 프로젝트 git 경로 목록

## 5. 구현 단계
1. 스캐폴드 + 설정 UI (소스 경로 기본값 자동 채움)
2. activity-timeline reader → `get_day` 집계 + 일일 카드 UI
3. gitlog 수집 (프로젝트 경로) + Git 섹션
4. 기간 통계 (주/월, recharts)
5. everything-plus/knowledge-base reader (v2 범위에서 우선순위 결정)
6. v2: AI 일기(선택), 프로젝트별 집계
7. Windows 빌드 검증

## 6. 테스트
- Rust: summarize 계산 (가짜 DaySummary 입력 → 기대값), activity-timeline reader는 임시 DB 픽스처
- 통합: 실제 activity-timeline/wsl-dashboard 데이터로 일일 로그 일치 확인
- 프론트: 카드/차트 렌더링 스모크

## 7. 확장/연계
- activity-timeline·wsl-dashboard·everything-plus·knowledge-base의 데이터 허브 (전 계획의 최종 연결점)
- 공통 추출 후보: `crates/activity`(외부 SQLite 읽기·기간 집계 헬퍼)

## 8. 완료 정의(Done)
- 설정된 소스로 일일 요약·타임라인·주간 통계가 정확히 나옴
- 원본 DB 무수정(읽기 전용) 확인, 소스가 없으면 우아한 스킵
- Windows 배포 빌드 성공
