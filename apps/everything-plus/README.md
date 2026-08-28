# everything-plus — Everything+ 개인 검색기

로컬 파일을 이름·내용으로 초고속 검색하는 앱. Rust 인덱싱·FTS5·파일 감시를 다루는 성능 중심 프로젝트.
산출물: `EverythingPlus.exe` (`apps/everything-plus`).

## 주요 기능

- **파일명 검색** — FTS5 인덱스, 파일명·경로 부분 일치, 밀리초 응답
- **내용 검색** — 텍스트 파일(TXT/MD/JSON/CSV/소스코드), PDF/DOCX text, XLS/XLSX/ODS 셀 값 내용 FTS5, 스니펫 하이라이트
- **정규식 모드** — regex 검색
- **인덱스 관리** — 검색 루트(드라이브/폴더) 추가·제외 규칙, re-index 진행률 표시, 파일 감시로 최신성 유지
- **결과 작업** — 열기·폴더에서 보기·경로/파일명 복사와 설치된 `path` capability 앱으로 열기 context menu
- **앱 간 검색** — catalog `Query`를 cold start와 실행 중 재호출에서 수신해 name/non-regex 검색으로 즉시 연결
- **고급 검색 필터** — 확장자, 수정 시각, 파일 크기, 등록된 검색 루트(source), 내용 인덱스 상태를
  native SQLite query에서 조합해 적용한다. 필터는 renderer에서 결과를 다시 걸러내는 방식이 아니라
  bounded FTS projection의 SQL parameter로 전달된다.
- **저장된 검색** — 이름·query·filter 정의를 local SQLite에 CRUD로 보관하고, 현재 결과를 저장하지
  않는다. 명시적으로 저장한 정의만 Launcher가 읽는 versioned snapshot으로 원자 발행한다.

## 고급 필터 계약

검색 필터는 다음 선택 항목을 조합할 수 있다.

- `extensions`: 최대 64개, 각 16바이트 이하의 확장자(점은 허용하지만 저장 시 제거)를 대소문자
  구분 없이 비교한다.
- `modifiedAfter`/`modifiedBefore`: 0 이상 epoch milliseconds의 양 끝 포함 범위. 역전된 범위는 거부한다.
- `minSize`/`maxSize`: 0 이상의 byte 범위. 음수·역전된 범위는 거부한다.
- `sourceRootId`: 사용자가 등록한 `roots.id`를 저장한다. raw path를 filter 값이나 외부 snapshot에
  넣지 않으며, root 삭제 뒤에도 오래된 positive id를 다른 root로 재해석하지 않고 안전하게 0건이
  된다.
- `contentStatus`: `indexed`, `truncated`(`partial` 별칭), `failed`, `not_indexed` 또는
  extractor의 고정 실패 코드(`too_large`, `unsupported_encoding`, `read_error`, `timeout`,
  `changed_during_read`, `skipped_sensitive`, `no_text`, `unsupported_encrypted`,
  `extract_error`) 중 하나다. 기본값은 필터 없음이며 content 검색은 기존처럼 indexed body만
  반환한다.

파일명 결과에는 `root_id`, `content_status`, `content_truncated` projection을 함께 반환하고,
내용 결과에는 확장자·size·mtime·root·extractor version·indexed 시각·encoding·text 문자 수와
고정 error code를 함께 반환한다. 따라서 truncated/실패/미인덱스 파일을 단순한 “검색 결과 없음”과
혼동하지 않는다. DB의 `files.root_id`는 재인덱싱 시 실제 등록 root id를 기록하며, 기존 v0.4.x
행은 migrate 시 가장 긴 등록 root를 선택해 보정한다. 등록 root가 없는 orphan row나
경계가 맞지 않는 stale row는 repair 때 제거하고, 방어적으로 일반 검색 projection에서도
현재 등록 root에 속한 행만 통과시킨다. nested root는 scan 순서와 무관하게 가장 깊은 root가
소유하며, parent/child 제거 시 남은 가장 깊은 root로만 재할당한다. root id는 삭제 후에도
재사용하지 않아 오래된 `sourceRootId`가 새 root를 가리키지 않는다. 제거로 content-enabled
ancestor가 새 owner가 되면 해당 ancestor의 bounded re-index를 예약해 content policy가
나중에 활성화된 파일도 stale 상태로 남지 않게 한다.

필터 요청은 확장자 64개, 날짜/size의 유한 정수와 양의 root id, 고정 상태 목록으로 native에서
재검증된다. FTS query의 4KiB/control-character 경계, 파일명 결과 기본 200·regex prefilter
2,000·내용 결과 200 상한은 그대로 유지한다. frontend는 검색 generation/sequence와 unmount
guard로 늦게 도착한 응답을 폐기하고, native는 SQL parameter·result cap만 사용해 임의 SQL·path
I/O를 수행하지 않는다. 검색 결과의 open/reveal도 실행 직전 absolute path·final file identity를
재확인하고 symlink/reparse point·directory·경로 traversal을 거부하며, opener/OS 오류 원문은 UI나
로그로 전달하지 않는다.

## 저장된 검색과 Launcher snapshot

저장된 검색은 최대 2,048개이며 label은 128 UTF-8 bytes, query는 Launcher가 재생할 수 있도록
512 UTF-8 bytes로 제한한다. 빈 query/label, control character, credential-like query(`Bearer`,
`Basic`, provider token, `password=`, `secret=`, `api_key=` 및 private-key marker)는 저장하지
않는다. 이 정책은 DB 저장 전과 snapshot 생성 전에 적용된다. 저장된 정의는 다음과 같이
`%LOCALAPPDATA%\devbox\integration\everything-plus\v1\summary.json`에 기록된다.

```json
{
  "schemaVersion": 1,
  "producer": "everything-plus",
  "producerVersion": "0.3.1",
  "generatedAt": "2026-08-28T00:00:00Z",
  "data": {
    "views": {
      "saved-queries": {
        "schemaVersion": 1,
        "freshnessMs": 0,
        "entries": [
          {
            "id": "saved-query-7",
            "label": "Rust sources",
            "detail": "Everything+ · saved query",
            "targetApp": "everything-plus",
            "targetKind": "query",
            "payloadVersion": 1,
            "payload": { "text": "cargo", "filter": { "extensions": ["rs"] } }
          }
        ]
      }
    }
  }
}
```

snapshot에는 query/filter 정의와 표시 metadata만 있으며 raw result list, full content, result path,
환경변수·credential은 없다. `crates/integration`의 producer/view/schema 검사와 unique temp file
생성·atomic replace를 사용하므로 partial JSON이 Launcher에 보이지 않는다. 앱 시작과 각 CRUD 뒤
전체 view를 다시 발행하며, snapshot 디렉터리 권한·symlink/reparse·쓰기 오류는 검색 자체를
막지 않고 고정 오류로 격리한다. 현재 Launcher는 이 producer capability
`snapshot:everything-plus/saved-queries/v1`를 통해서만 snapshot을 발견하고, 실행 시 현재
catalog와 payload version을 재검증한 뒤 optional `query-filter-v1`을 포함한 Query AppLink를
보낸다. filter를 모르는 구버전 수신기는 canonical `--query` text만 받아 하위 호환되고,
Everything+ 수신기는 filter를 normalize한 뒤 native search에 적용한다. 현재 payload version의
unknown field나 잘못된 filter는 text-only 요청으로 조용히 강등하지 않고 source/action을
fail-closed한다.

저장된 검색을 다시 열면 현재 index에 query/filter를 재실행한다. 인덱싱 중 Cancel로 남은
partial/truncated 상태나 root 삭제는 saved query를 변경하지 않으며, 다음 검색 시 최신 local
index 상태가 반영된다. DB 변경 후 snapshot 준비/교체가 실패하면 이전 saved definition set을
bounded SQLite transaction으로 복구하고, 다음 startup publication에서 다시 시도한다. DB lock은
snapshot filesystem I/O 전에 반납한다. root 추가·삭제와 deepest ownership repair도 하나의
`BEGIN IMMEDIATE` transaction으로 묶어 repair 실패가 root 목록과 file ownership을 반쪽만
갱신하지 못하게 한다.

## 내용 인덱스 경계

`index content`를 켠 검색 루트만 내용 인덱싱 대상이 된다. Everything+는 파일을 전부
읽지 않고 `src-tauri/src/core/content.rs`의 명시적 source/Markdown/plain-text 확장자와
PDF, Word `.docx`, Excel `.xls`/`.xlsx`, OpenDocument `.ods`, `README`, `LICENSE`,
`Dockerfile`, `.gitignore` 같은 소수의 이름만 선택한다. plain-text/PDF/DOCX/XLS/XLSX/ODS
extractor 버전은 각각 `text-v1`/`pdf-v1`/`docx-v1`/`xls-v1`/`xlsx-v1`/`ods-v1`이다.
`meta`의 독립적인 format marker가 첫 설치와 각 parser 버전 전환을 감지한다. legacy DOC,
macro-enabled DOCM, OCR, semantic search는 이 기능에 포함하지 않는다.

- UTF-8(선택적 BOM)과 UTF-16 little/big endian(BOM, 안정적으로 식별 가능한 BOM 없는
  텍스트)을 strict decode한다. invalid UTF-8/UTF-16, binary/NUL 데이터는 검색 hit가 되지
  않는 `unsupported_encoding` 상태로 남는다.
- 파일 하나는 최대 20 MiB, 보관 text는 최대 2,000,000 Unicode scalar characters,
  후보 하나의 cooperative processing budget은 10초다. PDF는 parser가 inflate하는
  page/object stream을 16 MiB, parsed indirect object를 100,000개, page를 10,000개로
  제한한다. object/page 구조 상한을 넘으면 `content_status=extract_error`와 고정
  `error_code=resource_limit`을 기록하고, 초과 text는 앞부분만 저장해
  `truncated=true`, `error_code=text_limit`을 기록한다. oversized/read/race/timeout/sensitive
  파일도 filename index는 유지하면서 고정 `content_status`와 `error_code`만 기록한다.
- PDF는 MIT `lopdf`로 text object만 오프라인 추출하며 page rendering, OCR, image/format
  extraction, 외부 resource 접근은 하지 않는다. image-only scanned PDF는 `no_text`,
  password/encrypted PDF는 `unsupported_encrypted`, corrupt PDF와 parser/decompression
  실패는 `extract_error`, object/page resource bound 초과는 `extract_error`와
  `resource_limit`으로 격리한다. 해당 상태에서도 filename search는 계속 제공된다.
- DOCX는 이미 앱에 고정된 MIT `zip`과 `quick-xml`만 사용해 canonical
  `[Content_Types].xml`, `_rels/.rels`, `word/document.xml`을 메모리 안에서 bounded read하고,
  main document의 `w:t` text와 paragraph/tab/line-break 구조만 오프라인 추출한다. field
  instruction, header/footer/footnote/comment, image/style/embedded object, macro와 외부 resource는
  읽거나 실행하지 않는다. package root는 유일한 internal `word/document.xml` target이어야 하며,
  unsafe/중복 ZIP path, external package relationship, DTD와 macro-enabled main content type은
  fail-closed한다. ZIP은 선언/실제 entry 각각 4,096개, entry 32 MiB, 전체 uncompressed
  64 MiB, XML depth 128/event 1,000,000개/text Unicode scalar와 raw attribute byte를 합산한
  8,000,000 budget, relationship 4,096개를 제한한다. password-protected OOXML CFB와
  encrypted ZIP entry는
  `unsupported_encrypted`, 빈 본문은 `no_text`, 손상/limit 문서는 고정 `extract_error`로
  격리하며 filename search는 유지한다.
- legacy XLS는 MIT `calamine`의 pure-Rust `Xls` reader로 worksheet 셀 값만 오프라인
  추출한다. 수식 재계산·VBA/macro·이미지·서식·외부 resource는 사용하지 않으며, XLSX와
  ODS는 확장자 dispatch에서 이 경로로 들어오지 않는다. password/encrypted workbook은
  `unsupported_encrypted`, 손상 workbook은 `extract_error`, resource-limit workbook은
  `extract_error`와 `resource_limit`으로
  격리하고 filename search는 계속 제공한다. workbook은 최대 256개 sheet와 4,000,000개
  logical cell을 허용한다. parser 진입 전 fail-closed BIFF preflight가 record 1,000,000개,
  formula 100,000개, shared string 200,000개·8,000,000자, 반복 참조로 확장되는 string
  16,000,000자와 추정 peak memory 256 MiB를 함께 제한한다.
- XLSX는 같은 MIT `calamine`의 streaming cell reader를 사용해 dense worksheet range를
  만들지 않는다. parser 진입 전에 ZIP end record의 선언 entry 수와 실제 central directory를
  각각 최대 4,096개로 확인하고, entry 32 MiB·전체 uncompressed 64 MiB, sheet 256개,
  logical/visited cell 4,000,000개, row 1,048,576개, column 16,384개를 제한한다. 표준
  `_rels/.rels`, `xl/workbook.xml`, `xl/_rels/workbook.xml.rels`만 authoritative package root로
  인정하며, 중복/unsafe ZIP path, external relationship, DTD, XML depth 128/event 1,000,000개,
  shared string 1,000,000개·8,000,000자를 calamine보다 먼저 검사한다.
- ODS도 pure-Rust `calamine::Ods`로 셀 값만 오프라인 추출한다. ZIP 선언/실제 entry 4,096개,
  entry 32 MiB·전체 uncompressed 64 MiB, sheet/row/column/cell 및 XML depth/event 상한을
  XLSX와 같은 순서로 검사한다. calamine이 dense range를 만들기 전에 row/column repeat를
  펼친 logical cell, 반복된 non-empty text/formula 16,000,000자, 기존/신규 Data·formula
  vector가 겹치는 peak memory 256 MiB를 보수적으로 계산해 repeat/clone bomb를 차단한다.
  암호화·외부 DTD·손상·ZIP/XML/resource-limit 문서는 고정 실패 metadata로 격리한다.
- 모든 spreadsheet extractor는 formula를 실행하거나 재계산하지 않는다. parser가 제공하는
  cached cell value만 일반 값처럼 취급하고 formula 원문은 FTS에 넣지 않는다. 10초 budget은
  ZIP/XML/BIFF 순회와 셀 누적에서 협력적으로 검사하며, 동기 calamine 호출 자체를 안전하게
  강제 중단하지는 않으므로 parser 진입 전 구조·메모리 상한으로 입력 작업량을 제한한다.
- `.env`, credential/netrc/npmrc/secrets 계열 파일과 private-key 확장자/이름은 읽지 않고
  `skipped_sensitive`로 기록한다. FTS snippet은 Authorization/Bearer/password/token/
  secret/API-key/private-key 형태와 독립적으로 노출된 common provider token·AWS access key·
  JWT 형태를 다시 redaction한 뒤 최대 4,096자까지만 UI에 보낸다.
  원문 내용은 app-local SQLite FTS에만 있고 network, log, telemetry, 다른 app snapshot으로
  복제하지 않는다.
- 전체/루트 재인덱스는 250개 파일 배치로 DB lock을 양보하고 UI에서 진행률·indexed/
  truncated/failed 수·마지막 시각을 확인할 수 있다. 실행 중 `Cancel`은 안전한 배치와
  파일 경계에서 협력적으로 중지하며, 이미 커밋된 부분 인덱스는 다음 `Re-index`로
  수렴한다. watcher 증분 반영도 동일 extractor와 bounds를 사용하고 읽기 전후의 크기와
  수정 시각을 열린 파일 및 경로 양쪽에서 다시 확인해 변경된 읽기는
  `changed_during_read`로 폐기한다.
- schema v2 migration은 사용자가 등록한 roots는 보존하고 파생 files/content FTS와
  metadata를 재생성한다. `content_status`, `extractor_version`, `truncated`,
  `indexed_at`, `error_code`, `encoding`, `text_chars`를 함께 저장하므로 검색 결과와
  상태 화면이 단순히 "없음"과 실패를 혼동하지 않는다.
- text/PDF/DOCX/XLS/XLSX/ODS extractor 버전은 서로 독립적으로 기록된다. parser 또는 cell
  normalization 규칙이 바뀌면 stale format row만 지우고 해당 확장자 후보만 다시 읽어 다른
  형식 인덱스를 보존한다. 각 marker가 없거나 현재 버전과 달라도 해당 형식 scan을 수행하며,
  성공한 full/format-only scan만 marker를 갱신한다. partial/cancelled scan은 marker를 남기지
  않고, format-only worker 중 새 root/index 요청이 들어오면 다음 실행을 `All`로 승격한다.

## 기술

- 공용 크레이트 `crates/filesystem`(제한 순회)·`crates/search`(FTS5) 사용
- legacy XLS reader는 `calamine = 0.36.1`(MIT, pure Rust, `Cargo.lock` 고정)을 사용하고,
  `cfb = 0.7.3`(MIT, pure Rust) preflight로 calamine의 eager range allocation 전에 CFB
  Workbook stream과 BIFF record/dimension/SST 참조를 fail-closed로 확인한다. 같은 CFB reader는
  password-protected DOCX의 표준 `EncryptedPackage` stream 식별에만 재사용한다. 설치물 고지에는
  lockfile 기반 transitive dependency inventory가 포함된다.
- DOCX/XLSX/ODS 컨테이너 preflight는 exact `zip = 8.6.0`(MIT)과
  `quick-xml = 0.41.0`(MIT)을 default feature 없이 사용한다. 모두 앱에 정적으로 포함되어
  별도 다운로드·Office/LibreOffice 설치·network 연결이 필요하지 않다.
- 백그라운드 watcher + 주기 재스캔
- inbound Query와 search input은 UTF-8 4 KiB 및 control-character 경계를 적용하며 명시적으로
  저장된 검색 정의를 제외한 원문을 로그·오류·지속 저장소에 남기지 않는다
- 파일명 검색은 기본 200개·정규식 prefilter 최대 2,000개, 내용 검색은 최대 200개로
  서로 다른 bounded result 계약을 유지한다
- 다른 앱으로 열기는 catalog capability와 설치 manifest를 모두 통과한 대상만 표시하고, 실행 직전 기존 절대 파일 경로를 재검증한다

## 데이터

- 인덱스: `%LOCALAPPDATA%\com.devbox.everythingplus\data.db`

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`
