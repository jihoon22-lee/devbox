# 저장소 정합성 정리 — 설계

- 날짜: 2026-08-11
- 범위: 문서/코드의 낡은 참조 정리 + everything-plus 인덱싱 수명주기 버그 수정
- 산출: PR 2개 (문서 정리 / everything-plus 수정)

## 배경

10개 앱을 만드는 동안 문서가 8개 앱 기준에 머물렀고, v0.2.2의 life-log 경로 수정이
프론트 문자열 두 곳에 반영되지 않았다. 별개로 everything-plus의 인덱싱 경로에서
동작 버그 네 건이 확인됐다.

신규 앱(code-pad, run-manager)과 knowledge-base 확장 spec이 모두 `CONVENTIONS.md`·
`AGENTS.md`를 참조하므로, 그 문서들이 사실과 어긋난 상태에서 spec을 쓰면 오류가
전파된다. 따라서 다른 작업보다 먼저 처리한다.

## PR ① 문서/표기 정리

`docs/workspace/sync-stale-references`

### 사실 오류 3건

| 위치 | 현재 | 수정 |
|---|---|---|
| `apps/life-log/src/api.ts:60` | mock 경로가 `%LOCALAPPDATA%\Workbench\activity-timeline\data.db` | 실제 경로(`com.workbench.activitytimeline`)로 |
| `apps/life-log/src/App.tsx:171` | 설정 화면 안내문이 옛 경로 | 실제 경로로 |
| `CONVENTIONS.md:98` | "React 18" | React 19 (실제 `^19.1.0`) |

### 앱 개수 8 → 10

`AGENTS.md:3` · `CONVENTIONS.md:1,3,42-51,81,193,235,237` · `docs/development.md:3,12-20` ·
`docs/roadmap.md:3` · `docs/windows-guide.md:3,160` · `docs/architecture.md:59,60`

특히 다음 두 곳은 단순 숫자 치환이 아니라 목록 자체가 불완전하다.

- `CONVENTIONS.md:81` 산출물 목록 — `WSLDesktop.exe`·`DevboxManager.exe` 누락
- `docs/development.md` 구조 목록 — wsl-desktop·devbox-manager 두 앱 통째로 누락

### 현황 서술 1건

`AGENTS.md:12` "apps/port-manager부터 개발 진행 중" → 10개 앱 구현 완료로 갱신.
같은 줄의 "`crates/`, `packages/`는 아직 비어 있음"은 **여전히 사실이므로 유지**한다.

### 건드리지 않는 것

`CONVENTIONS.md` §3의 프론트엔드 스택 목록(Tailwind, zustand, lucide-react,
@tanstack/react-table, recharts, react-router-dom, @uiw/react-codemirror)은 현재
어느 앱에서도 쓰이지 않지만, 이는 불일치가 아니라 **아직 사용하지 않은 허용 목록**이다.
그대로 둔다.

## PR ② everything-plus 인덱싱 수명주기

`fix/everything-plus/index-lifecycle`

### 버그 ① 부분 재인덱싱이 전체 인덱스를 삭제

`commands/indexing.rs:86-90`에서 `clear_all()`로 전체를 지운 뒤 `targets`만 다시
인덱싱한다. 루트 A가 등록된 상태에서 루트 B를 추가하면 `spawn_index(st, ["B"])`가
호출되어 **A의 인덱스가 사라진다.**

### 버그 ② remove_root이 Windows 경로에서 파일을 삭제하지 못함

`core/db.rs:87`

```sql
DELETE FROM files WHERE path LIKE ?1 || '/%'
```

루트 입력 placeholder는 `C:\projects`이고 경로 정규화가 없으므로 저장 형태는
`C:\projects\foo\bar.rs`다. LIKE 패턴은 `C:\projects/%`가 되어 **한 건도 매치하지
않는다.** 결과적으로 루트를 제거해도 `roots` 행만 지워지고 인덱싱된 파일은 남아
검색 결과에 계속 노출된다.

같은 파일의 `upsert_file`은 `rsplit(['/', '\\'])`로 두 구분자를 모두 처리하는데
(`db.rs:108`) LIKE만 `/`를 가정한 불일치다.

### 버그 ③ file_content 고아 레코드

`clear_all`은 `file_content` → `files` 순으로 지우지만 `remove_root`은 `files`만
지운다. FK에 CASCADE가 없어 내용 인덱스가 남는다.

근본 원인이 하나 더 있다. `upsert_file`이 `INSERT OR REPLACE`를 쓰는데, 이는 충돌 시
기존 행을 삭제하고 새로 삽입하므로 **재인덱싱할 때마다 `files.id`가 바뀐다.**
`file_content.file_id`는 옛 id를 가리킨 채 고아가 된다. 지금은 전체 재인덱싱이
매번 `clear_all()`을 부르며 가려져 있었으나, 부분 재인덱싱을 도입하면 드러난다.

### 버그 ④ 인덱싱 중 DB 락 전체 점유

`commands/indexing.rs:75`에서 인덱싱 스레드가 시작하자마자 DB 락을 잡고 끝날 때까지
보유한다. `index_status`도 같은 락을 요구하므로 인덱싱 내내 블로킹된다.
**v0.2.0에서 추가한 "re-index 진행률 표시"가 실질적으로 동작하지 않는다** —
진행 중에는 응답이 없다가 완료 후 최종 상태만 반환된다.

### 수정 방향

**1. 경로 정규화를 근본 해법으로 채택한다.**

`normalize_path()`를 `core/db.rs`에 두고 `add_root`·`upsert_file` 진입점에서
구분자를 `/`로 통일한다. LIKE에 `\` 분기와 ESCAPE 절을 덧대는 것보다 짧고, 버그 ②의
원인 자체를 없앤다. Windows API는 `/` 구분자를 그대로 받으므로 파일 열기에 지장이 없다.

**2. 기존 DB는 마이그레이션하지 않는다.**

`meta(key, value)` 테이블에 `schema_version`을 두고, 버전이 낮으면 `clear_all()` 후
버전을 올린다. 인덱스는 언제든 재생성 가능한 파생 데이터이므로 마이그레이션 코드를
쓰는 것보다 전체 재인덱싱을 유도하는 편이 짧고 안전하다.

**3. 루트 단위 삭제를 도입한다.**

`clear_root(conn, root_path)`가 `file_content` → `files` 순으로 지운다.
`remove_root`과 부분 재인덱싱이 함께 쓴다. `spawn_index`는 전체 재인덱싱이면
`clear_all()`, 부분이면 대상 루트마다 `clear_root()`를 호출한다.

**4. `upsert_file`을 id 보존 방식으로 바꾼다.**

`INSERT OR REPLACE` → `ON CONFLICT(path) DO UPDATE SET ... RETURNING id`.
재인덱싱해도 `files.id`가 유지되어 `file_content` 고아가 구조적으로 발생하지 않는다.
everything-plus는 `modern_sqlite` 기능이 이미 켜져 있어 `RETURNING`을 쓸 수 있다.

**5. 배치 단위로 락을 반납한다.**

가장 오래 걸리는 `collect()`(walkdir 순회)를 **락 밖에서** 수행하고, 삽입은 파일
500개 단위로 트랜잭션을 끊어 커밋한 뒤 락을 놓는다. `index_status`가 그 틈에
응답하면서 진행률이 실제로 갱신된다. 진행 카운터는 `store()` 대신 `fetch_add()`로
배치마다 누적한다.

### 테스트

전부 in-memory DB로 `cargo test`에서 검증한다 (기존 `core/db.rs` 테스트 패턴 유지).

- 루트 2개 등록 후 하나를 추가로 인덱싱해도 기존 루트의 인덱스가 남아 있다
- `remove_root` 후 해당 루트의 파일이 검색 결과에서 사라진다 (`\`·`/` 양쪽 입력)
- `remove_root` 후 해당 루트의 `file_content` 행이 남지 않는다
- `normalize_path`가 두 구분자를 같은 결과로 만들고 끝의 구분자를 제거한다
- 같은 경로를 재인덱싱해도 `files.id`가 유지된다

### 범위 밖

- 파일 watcher를 통한 실시간 증분 인덱싱 (roadmap의 별도 항목)
- 내용 인덱싱 성능 최적화
- 인덱싱 취소 기능
